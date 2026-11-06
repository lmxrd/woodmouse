//! Single sign-on through an OpenID Connect provider (Keycloak, Authentik, Authelia, Google, ...): the
//! settings in force, the HTTP client that talks to the provider, the cache of its metadata, and the
//! login in progress, which lives in a signed cookie (never in server memory, so a flood of unfinished
//! logins cannot push out real ones). The routes are in `routes::oidc`.
//!
//! What `openidconnect` checks on the ID token, and is not repeated here: the issuer, the audience (an
//! extra audience is refused), the expiry, the signature against the provider's keys, and that it is
//! signed at all (`alg=none` is refused). It does not check `iat`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use std::time::{Duration, Instant};

use axum_extra::extract::cookie::{Cookie, Key, SameSite};
use entity::OidcSettings;
use openidconnect::core::CoreProviderMetadata;
use openidconnect::{AsyncHttpClient, HttpRequest, HttpResponse, IssuerUrl, JsonWebKeySet};
use serde::{Deserialize, Serialize};

use crate::config;

/// Where the provider sends the browser back to.
pub const CALLBACK_PATH: &str = "/api/auth/oidc/callback";
/// How long a login may take, from leaving for the provider to coming back.
pub const LOGIN_TTL_SECS: u64 = 10 * 60;
/// The longest a request to the provider may take.
const TIMEOUT: Duration = Duration::from_secs(10);
/// The largest response read from the provider (discovery, keys, tokens are a few KB).
const MAX_BODY: usize = 1024 * 1024;
/// How long the provider's metadata (endpoints and signing keys) is trusted before it is fetched again.
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);
/// Providers kept: the one in use, and one an admin is checking, so the two do not evict each other.
const CACHE_SIZE: usize = 2;
/// After a failed fetch, logins fail at once for this long instead of waiting on a provider that is down.
const BACKOFF: Duration = Duration::from_secs(30);

/// The settings from the environment (`OIDC_*`). Each one that is set wins over the stored field, and
/// the admin page cannot change it.
#[derive(Clone, Debug, Default)]
pub struct EnvSettings {
    pub enabled: Option<bool>,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub display_name: Option<String>,
    pub redirect_url: Option<String>,
    pub auto_create: Option<bool>,
    /// `OIDC_ONLY`: single sign-on is the only way in; password sign-in, invites by password and password
    /// changes are refused. Needs the provider from the environment, and keeps single sign-on on.
    pub only: Option<bool>,
}

impl EnvSettings {
    /// Read `OIDC_*`. A yes/no variable with any other value is an error (the server does not start).
    pub fn from_env() -> Result<Self, String> {
        let env = Self {
            enabled: config::flag("OIDC_ENABLED")?,
            issuer: config::var("OIDC_ISSUER"),
            client_id: config::var("OIDC_CLIENT_ID"),
            client_secret: config::var("OIDC_CLIENT_SECRET"),
            display_name: config::var("OIDC_DISPLAY_NAME"),
            redirect_url: config::var("OIDC_REDIRECT_URL"),
            auto_create: config::flag("OIDC_AUTO_CREATE")?,
            only: config::flag("OIDC_ONLY")?,
        };
        env.checked()
    }

    /// Refuse what cannot be safely put in force. The secret goes to the issuer's token endpoint: with
    /// only the secret from the environment, an issuer stored on the admin page (now or earlier) would
    /// receive it.
    pub fn checked(mut self) -> Result<Self, String> {
        if self.only == Some(true) {
            // Nobody could sign in if single sign-on were off or half set up, so with OIDC_ONLY the
            // provider comes from the environment (the admin page cannot change it) and it stays on.
            if self.issuer.is_none() || self.client_id.is_none() {
                return Err(
                    "OIDC_ONLY needs OIDC_ISSUER and OIDC_CLIENT_ID to be set too".to_string(),
                );
            }
            if self.enabled == Some(false) {
                return Err("OIDC_ONLY cannot be set with OIDC_ENABLED off".to_string());
            }
            self.enabled = Some(true);
        }
        if self.client_secret.is_some() && self.issuer.is_none() {
            return Err("OIDC_CLIENT_SECRET needs OIDC_ISSUER to be set too".to_string());
        }
        if let Some(url) = &self.redirect_url {
            check_redirect_url(url).map_err(|e| format!("OIDC_REDIRECT_URL: {e}"))?;
        }
        Ok(self)
    }
}

/// A callback URL that reaches `/api/auth/oidc/callback` here.
pub fn check_redirect_url(url: &str) -> Result<(), String> {
    let parsed = openidconnect::url::Url::parse(url).map_err(|_| "not a URL".to_string())?;
    if !matches!(parsed.scheme(), "https" | "http") || parsed.path() != CALLBACK_PATH {
        return Err(format!("must be http(s) and end in {CALLBACK_PATH}"));
    }
    Ok(())
}

/// The settings in force: the environment over the stored row. Present only while single sign-on is
/// enabled and has an issuer and a client id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub display_name: String,
    pub redirect_url: Option<String>,
    pub auto_create: bool,
}

/// What is in force for `env` over `row`. Enabled defaults to on when the environment names an issuer.
pub fn effective(env: &EnvSettings, row: Option<&OidcSettings>) -> Option<Config> {
    let stored = |f: fn(&OidcSettings) -> &Option<String>| row.and_then(|r| f(r).clone());
    let enabled = env
        .enabled
        .or(row.map(|r| r.enabled))
        .unwrap_or(env.issuer.is_some());
    if !enabled {
        return None;
    }
    Some(Config {
        issuer: env.issuer.clone().or(stored(|r| &r.issuer))?,
        client_id: env.client_id.clone().or(stored(|r| &r.client_id))?,
        client_secret: env.client_secret.clone().or(stored(|r| &r.client_secret)),
        display_name: env
            .display_name
            .clone()
            .or(stored(|r| &r.display_name))
            .unwrap_or_else(|| "single sign-on".to_string()),
        redirect_url: env.redirect_url.clone().or(stored(|r| &r.redirect_url)),
        auto_create: env
            .auto_create
            .or(row.map(|r| r.auto_create))
            .unwrap_or(false),
    })
}

/// Whether the provider may be reached at `url`: https, or plain http on this machine (development and
/// the tests).
pub fn secure_url(url: &str) -> bool {
    let Ok(url) = openidconnect::url::Url::parse(url) else {
        return false;
    };
    match url.scheme() {
        "https" => url.host().is_some(),
        "http" => matches!(url.host_str(), Some("localhost" | "127.0.0.1")),
        _ => false,
    }
}

/// Why a request to the provider failed.
#[derive(Debug)]
pub enum HttpError {
    /// Not https (and not this machine).
    Insecure(String),
    /// The response was larger than [`MAX_BODY`].
    TooLarge,
    Request(reqwest::Error),
    Response(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Insecure(url) => write!(f, "{url} is not https"),
            Self::TooLarge => write!(f, "the response is larger than {MAX_BODY} bytes"),
            Self::Request(e) => write!(f, "{e}"),
            Self::Response(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for HttpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Request(e) => Some(e),
            _ => None,
        }
    }
}

/// The client for the provider: no redirects (the crate's advice), a timeout, a cap on the size of a
/// response, https only. A type of its own because `openidconnect` takes any `AsyncHttpClient`, and a
/// closure's future is "not general enough" to be `Send` inside an axum handler.
#[derive(Clone)]
pub struct Http(reqwest::Client);

impl Default for Http {
    fn default() -> Self {
        // The TLS stack lettre already brings: rustls with ring and the webpki roots (no system store,
        // so the image needs no CA bundle).
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        let client = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TIMEOUT)
            .build()
            .expect("build the HTTP client");
        Self(client)
    }
}

impl Http {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let url = request.uri().to_string();
        if !secure_url(&url) {
            return Err(HttpError::Insecure(url));
        }
        let request = reqwest::Request::try_from(request).map_err(HttpError::Request)?;
        let mut response = self.0.execute(request).await.map_err(HttpError::Request)?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(HttpError::Request)? {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(HttpError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let mut out = openidconnect::http::Response::builder().status(response.status());
        for (name, value) in response.headers() {
            out = out.header(name, value);
        }
        out.body(body)
            .map_err(|e| HttpError::Response(e.to_string()))
    }
}

impl<'c> AsyncHttpClient<'c> for Http {
    type Error = HttpError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, HttpError>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(self.send(request))
    }
}

/// The provider's metadata as fetched: its endpoints and current signing keys.
pub type Metadata = Arc<CoreProviderMetadata>;

#[derive(Default)]
struct Cache {
    /// Newest first, at most [`CACHE_SIZE`].
    entries: Vec<(String, Instant, Metadata)>,
    /// Issuers whose last fetch failed, and when.
    failed: HashMap<String, Instant>,
}

/// A login in progress, carried by the browser in a signed cookie from `/login` to `/callback`.
#[derive(Serialize, Deserialize)]
pub struct Pending {
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub callback: String,
    /// The provider and client it was started with: a login started before the settings changed fails.
    pub issuer: String,
    pub client_id: String,
    /// Seconds since this process started ([`Oidc::now`]). The signing key is made at start too, so a
    /// cookie from before a restart does not verify at all.
    pub started: u64,
}

/// Single sign-on state, shared by every request.
pub struct Oidc {
    pub http: Http,
    env: EnvSettings,
    /// Signs the login cookie. Made at start: a restart abandons logins in progress, nothing else.
    key: Key,
    started: Instant,
    config: RwLock<Option<Arc<Config>>>,
    /// Held by a settings save from reading the stored row until the new settings are in force.
    pub save: tokio::sync::Mutex<()>,
    /// One fetch from a provider at a time.
    fetching: tokio::sync::Mutex<()>,
    cache: StdMutex<Cache>,
}

impl Oidc {
    pub fn new(env: EnvSettings) -> Self {
        let config = effective(&env, None).map(Arc::new);
        Self {
            http: Http::default(),
            env,
            key: Key::generate(),
            started: Instant::now(),
            config: RwLock::new(config),
            save: Default::default(),
            fetching: Default::default(),
            cache: Default::default(),
        }
    }

    /// The same, as if this process had started `ago` earlier (tests of a login's age). Unchanged if the
    /// machine has not been up that long.
    pub fn started_ago(mut self, ago: Duration) -> Self {
        self.started = self.started.checked_sub(ago).unwrap_or(self.started);
        self
    }

    pub fn env(&self) -> &EnvSettings {
        &self.env
    }

    /// Whether signing in with a password is allowed (it is not with `OIDC_ONLY`).
    pub fn password_login(&self) -> bool {
        self.env.only != Some(true)
    }

    /// Refuse a password sign-in, invite or change while `OIDC_ONLY` is set.
    pub fn require_password_login(&self) -> Result<(), crate::error::ApiError> {
        if self.password_login() {
            Ok(())
        } else {
            Err(crate::error::ApiError::forbidden(
                "Password sign-in is turned off; use single sign-on",
            ))
        }
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    /// Seconds since this process started: the clock of [`Pending::started`], which wall-clock changes
    /// cannot move.
    pub fn now(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// The settings in force, if single sign-on is on.
    pub fn config(&self) -> Option<Arc<Config>> {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Put the stored settings in force (over the environment).
    pub fn apply(&self, row: Option<&OidcSettings>) {
        *self.config.write().unwrap_or_else(|e| e.into_inner()) =
            effective(&self.env, row).map(Arc::new);
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cached(&self, issuer: &str) -> Option<Metadata> {
        self.lock_cache()
            .entries
            .iter()
            .find(|(i, at, _)| i == issuer && at.elapsed() < CACHE_TTL)
            .map(|(_, _, m)| m.clone())
    }

    fn backing_off(&self, issuer: &str) -> bool {
        self.lock_cache()
            .failed
            .get(issuer)
            .is_some_and(|at| at.elapsed() < BACKOFF)
    }

    fn store(&self, issuer: &str, metadata: Metadata) {
        let mut cache = self.lock_cache();
        cache.failed.remove(issuer);
        cache.entries.retain(|(i, _, _)| i != issuer);
        cache
            .entries
            .insert(0, (issuer.to_string(), Instant::now(), metadata));
        cache.entries.truncate(CACHE_SIZE);
    }

    fn failed(&self, issuer: &str) {
        let mut cache = self.lock_cache();
        cache.failed.retain(|_, at| at.elapsed() < BACKOFF);
        cache.failed.insert(issuer.to_string(), Instant::now());
    }

    /// The provider's metadata, from the cache or fetched. Within [`BACKOFF`] of a failed fetch it fails
    /// at once.
    pub async fn metadata(&self, issuer: &str) -> Result<Metadata, String> {
        if let Some(metadata) = self.cached(issuer) {
            return Ok(metadata);
        }
        if self.backing_off(issuer) {
            return Err("the provider was unreachable moments ago".to_string());
        }
        let _turn = self.fetching.lock().await;
        // Someone else may have fetched (or failed) while this waited.
        if let Some(metadata) = self.cached(issuer) {
            return Ok(metadata);
        }
        if self.backing_off(issuer) {
            return Err("the provider was unreachable moments ago".to_string());
        }
        self.fetch_and_store(issuer).await
    }

    /// Fetch the metadata again, for keys the cached set does not have (the provider rotated them).
    pub async fn refresh(&self, issuer: &str) -> Result<Metadata, String> {
        self.lock_cache().entries.retain(|(i, _, _)| i != issuer);
        self.metadata(issuer).await
    }

    /// Fetch the metadata now, whatever is cached or backing off: the admin's check before saving.
    pub async fn check(&self, issuer: &str) -> Result<Metadata, String> {
        let _turn = self.fetching.lock().await;
        self.fetch_and_store(issuer).await
    }

    async fn fetch_and_store(&self, issuer: &str) -> Result<Metadata, String> {
        match self.fetch(issuer).await {
            Ok(metadata) => {
                let metadata = Arc::new(metadata);
                self.store(issuer, metadata.clone());
                Ok(metadata)
            }
            Err(e) => {
                tracing::warn!(issuer, error = %e, "could not read the single sign-on provider's metadata");
                self.failed(issuer);
                Err(e)
            }
        }
    }

    /// Discovery by hand rather than `discover_async`, which fetches the key set before its URL can be
    /// checked: every endpoint must be https before anything is fetched from or sent to it.
    async fn fetch(&self, issuer: &str) -> Result<CoreProviderMetadata, String> {
        if !secure_url(issuer) {
            return Err(format!("the issuer {issuer} is not https"));
        }
        let issuer_url = IssuerUrl::new(issuer.to_string()).map_err(|e| e.to_string())?;
        let discovery = issuer_url
            .join(".well-known/openid-configuration")
            .map_err(|e| e.to_string())?;
        let request = openidconnect::http::Request::builder()
            .uri(discovery.as_str())
            .header("accept", "application/json")
            .body(Vec::new())
            .map_err(|e| e.to_string())?;
        let response = self.http.send(request).await.map_err(|e| e.to_string())?;
        if response.status() != openidconnect::http::StatusCode::OK {
            return Err(format!("{discovery} answered {}", response.status()));
        }
        let metadata: CoreProviderMetadata = serde_json::from_slice(response.body())
            .map_err(|e| format!("{discovery} is not a discovery document: {e}"))?;
        // The discovery document must name the issuer exactly as configured (OpenID Connect Discovery
        // 4.3), or tokens would be checked against another issuer than the one asked.
        if metadata.issuer() != &issuer_url {
            return Err(format!(
                "the provider calls itself {}, not {issuer}",
                metadata.issuer().as_str()
            ));
        }
        let token = metadata
            .token_endpoint()
            .ok_or("the provider has no token endpoint")?;
        for (what, url) in [
            (
                "authorization endpoint",
                metadata.authorization_endpoint().as_str(),
            ),
            ("token endpoint", token.as_str()),
            ("key set", metadata.jwks_uri().as_str()),
        ] {
            if !secure_url(url) {
                return Err(format!("the provider's {what} {url} is not https"));
            }
        }
        let keys = JsonWebKeySet::fetch_async(metadata.jwks_uri(), &self.http)
            .await
            .map_err(|e| format!("could not read the provider's keys: {}", error_chain(&e)))?;
        Ok(metadata.set_jwks(keys))
    }
}

/// An error and its sources, for the log.
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(e) = source {
        out.push_str(": ");
        out.push_str(&e.to_string());
        source = e.source();
    }
    out
}

/// The login cookie's name. Over https it has the `__Host-` prefix, which browsers accept only with
/// `Secure`, `Path=/` and no `Domain`, so a sibling subdomain cannot plant one (login CSRF).
pub fn cookie_name(secure: bool) -> &'static str {
    if secure {
        "__Host-woodmouse_oidc"
    } else {
        "woodmouse_oidc"
    }
}

/// The login cookie. `SameSite=Lax`, not Strict: the callback is a cross-site navigation from the
/// provider. Over plain http (development) it is scoped to the callback path instead.
pub fn login_cookie(value: String, secure: bool) -> Cookie<'static> {
    let mut cookie = Cookie::new(cookie_name(secure), value);
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_max_age(::time::Duration::seconds(LOGIN_TTL_SECS as i64));
    if secure {
        cookie.set_secure(true);
        cookie.set_path("/");
    } else {
        cookie.set_path(CALLBACK_PATH);
    }
    cookie
}

/// The removal of the login cookie, with the same attributes it was set with: a browser ignores a
/// `__Host-` removal without `Secure`.
pub fn login_cookie_removal(secure: bool) -> Cookie<'static> {
    let mut cookie = login_cookie(String::new(), secure);
    cookie.make_removal();
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_this_machine() {
        assert!(secure_url("https://id.example.com/realms/x"));
        assert!(secure_url("http://localhost:8080/"));
        assert!(secure_url("http://127.0.0.1:9000/x"));
        assert!(!secure_url("http://id.example.com/"));
        assert!(!secure_url("http://localhost.example.com/"));
        assert!(!secure_url("http://10.0.0.1/"));
        assert!(!secure_url("ftp://localhost/"));
        assert!(!secure_url("not a url"));
    }

    #[tokio::test]
    async fn the_client_refuses_plain_http_elsewhere() {
        let request = openidconnect::http::Request::builder()
            .uri("http://id.example.com/token")
            .body(Vec::new())
            .unwrap();
        assert!(matches!(
            Http::default().call(request).await,
            Err(HttpError::Insecure(_))
        ));
    }

    #[test]
    fn the_environments_secret_needs_its_issuer() {
        let secret_only = EnvSettings {
            client_secret: Some("s".into()),
            ..Default::default()
        };
        assert!(secret_only.checked().is_err());
        let both = EnvSettings {
            client_secret: Some("s".into()),
            issuer: Some("https://id.example.com".into()),
            ..Default::default()
        };
        assert!(both.checked().is_ok());
        let bad_redirect = EnvSettings {
            redirect_url: Some("https://woodmouse.example.com/callback".into()),
            ..Default::default()
        };
        assert!(bad_redirect.checked().is_err());
    }

    #[test]
    fn the_environment_wins_field_by_field() {
        let row = OidcSettings {
            id: 1,
            enabled: true,
            issuer: Some("https://stored".into()),
            client_id: Some("stored-client".into()),
            client_secret: Some("stored-secret".into()),
            display_name: None,
            redirect_url: None,
            auto_create: true,
        };
        let env = EnvSettings {
            client_id: Some("env-client".into()),
            auto_create: Some(false),
            ..Default::default()
        };
        let config = effective(&env, Some(&row)).unwrap();
        assert_eq!(config.issuer, "https://stored");
        assert_eq!(config.client_id, "env-client");
        assert_eq!(config.client_secret.as_deref(), Some("stored-secret"));
        assert!(!config.auto_create);
        // Off unless enabled; the environment's issuer alone turns it on.
        assert_eq!(effective(&EnvSettings::default(), None), None);
        let env = EnvSettings {
            issuer: Some("https://env".into()),
            client_id: Some("c".into()),
            ..Default::default()
        };
        assert!(effective(&env, None).is_some());
        let off = EnvSettings {
            enabled: Some(false),
            ..env
        };
        assert_eq!(effective(&off, Some(&row)), None);
    }

    #[test]
    fn the_removal_keeps_the_attributes() {
        let removal = login_cookie_removal(true).to_string();
        assert!(removal.starts_with("__Host-woodmouse_oidc=;"), "{removal}");
        for part in ["Secure", "HttpOnly", "Path=/", "SameSite=Lax", "Max-Age=0"] {
            assert!(removal.contains(part), "{removal} lacks {part}");
        }
        let plain = login_cookie_removal(false).to_string();
        assert!(plain.contains(&format!("Path={CALLBACK_PATH}")), "{plain}");
        assert!(!plain.contains("Secure"), "{plain}");
    }
}
