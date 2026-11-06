//! Single sign-on routes: the round trip to the provider (`/auth/oidc/login` and `/auth/oidc/callback`),
//! and the super admin's settings (`/admin/oidc`). The flow itself (settings in force, provider metadata,
//! the login cookie) is in `crate::oidc`.
//!
//! The browser goes to `/auth/oidc/login` itself (a plain link, not a fetch), which sends it on to the
//! provider; the provider sends it back to `/auth/oidc/callback`, which answers with a redirect to `/` or
//! to `/login?error=<code>`, one of the fixed [`SsoError`] codes. The provider's own error text is never
//! shown.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{CookieJar, SignedCookieJar};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClaimsVerificationError, ClientId, ClientSecret, CsrfToken,
    EndpointMaybeSet, EndpointNotSet, EndpointSet, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, SignatureVerificationError, TokenResponse,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::config;
use crate::db::prelude::*;
use crate::error::{ApiError, ErrorBody};
use crate::extract::ApiJson;
use crate::oidc::{self, CALLBACK_PATH, Config, LOGIN_TTL_SECS, Pending};
use crate::state::AppState;
use crate::time;
use crate::totp;
use entity::{User, oidc_settings, user_identities, user_invites, users};

/// The round trip to the provider; no session.
pub fn public_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(oidc_login))
        .routes(routes!(oidc_callback))
}

/// The settings; the super admin only.
pub fn protected_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(admin_get_oidc, admin_update_oidc))
}

/// Why a single sign-on did not sign anyone in: the `error` of `/login?error=...`. The frontend has a
/// message for each (`frontend/src/lib/sso.ts`); keep the two lists in step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SsoError {
    /// Single sign-on is off, or the provider cannot be reached.
    Unavailable,
    /// The callback URL and the cookie settings disagree (see the server log).
    Misconfigured,
    /// The login was not started in this browser, took too long, or the settings changed meanwhile.
    State,
    /// The provider refused, or the person cancelled.
    Denied,
    /// The provider's answer could not be checked.
    Failed,
    /// No account is linked, and the provider vouched for no email to match one by.
    Email,
    /// No account has that email, and accounts are not created automatically.
    Unknown,
    /// The account with that email is already linked to another account at this provider.
    Linked,
    /// The account is disabled.
    Disabled,
    /// Nobody has set up the site yet (the first account is made with a password).
    Setup,
}

impl SsoError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Misconfigured => "misconfigured",
            Self::State => "state",
            Self::Denied => "denied",
            Self::Failed => "failed",
            Self::Email => "email",
            Self::Unknown => "unknown",
            Self::Linked => "linked",
            Self::Disabled => "disabled",
            Self::Setup => "setup",
        }
    }

    fn redirect(self) -> Redirect {
        Redirect::to(&format!("/login?error={}", self.code()))
    }
}

/// The client as `from_provider_metadata` makes it.
type Client = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

fn client(metadata: &CoreProviderMetadata, config: &Config, callback: RedirectUrl) -> Client {
    CoreClient::from_provider_metadata(
        metadata.clone(),
        ClientId::new(config.client_id.clone()),
        config.client_secret.clone().map(ClientSecret::new),
    )
    .set_redirect_uri(callback)
}

/// The callback URL to register at the provider: the configured one, else `PUBLIC_URL`'s, else this
/// request's host over plain http (development; forwarded headers are not trusted).
pub fn callback_url(config: Option<&Config>, headers: &HeaderMap) -> Option<String> {
    if let Some(url) = config.and_then(|c| c.redirect_url.clone()) {
        return Some(url);
    }
    if let Some(base) = config::public_url() {
        return Some(format!("{base}{CALLBACK_PATH}"));
    }
    let host = headers.get(header::HOST)?.to_str().ok()?;
    let url = openidconnect::url::Url::parse(&format!("http://{host}{CALLBACK_PATH}")).ok()?;
    // Only a plain host (and port): nothing in the header may change the URL's shape.
    (url.path() == CALLBACK_PATH && url.username().is_empty() && url.query().is_none())
        .then(|| url.to_string())
}

/// Leave for the provider. Sets the signed login cookie and redirects there, or back to the login page
/// with an error.
#[utoipa::path(
    get,
    path = "/auth/oidc/login",
    tag = "auth",
    responses(
        (status = 303, description = "To the provider, or to `/login?error=<code>`")
    )
)]
pub async fn oidc_login(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match start_login(&state, &headers).await {
        Ok(response) => response,
        Err(error) => error.redirect().into_response(),
    }
}

async fn start_login(state: &AppState, headers: &HeaderMap) -> Result<Response, SsoError> {
    let config = state.oidc.config().ok_or(SsoError::Unavailable)?;
    let callback = callback_url(Some(&config), headers).ok_or(SsoError::Misconfigured)?;
    // The cookie is `__Host-` and `Secure` exactly when the site is https. A plain cookie on an https
    // site would undo that protection, so mixed settings are refused rather than half supported.
    let https = callback.starts_with("https://");
    if https != state.secure_cookies {
        tracing::warn!(
            callback,
            secure_cookies = state.secure_cookies,
            "single sign-on is refused: the callback URL is {} but PUBLIC_URL makes cookies {}; \
             check PUBLIC_URL and the redirect URL setting",
            if https { "https" } else { "http" },
            if state.secure_cookies {
                "secure"
            } else {
                "plain"
            },
        );
        return Err(SsoError::Misconfigured);
    }
    let redirect = RedirectUrl::new(callback.clone()).map_err(|_| SsoError::Misconfigured)?;
    let metadata = state
        .oidc
        .metadata(&config.issuer)
        .await
        .map_err(|_| SsoError::Unavailable)?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, csrf, nonce) = client(&metadata, &config, redirect)
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        // `openid` is added by the client itself.
        .add_scopes(
            config
                .scopes
                .iter()
                .filter(|s| *s != "openid")
                .map(|s| Scope::new(s.clone())),
        )
        .set_pkce_challenge(challenge)
        .url();
    let pending = Pending {
        state: csrf.secret().clone(),
        nonce: nonce.secret().clone(),
        pkce_verifier: verifier.secret().clone(),
        callback,
        issuer: config.issuer.clone(),
        client_id: config.client_id.clone(),
        started: state.oidc.now(),
    };
    let value = serde_json::to_string(&pending).map_err(|_| SsoError::Failed)?;
    let jar = SignedCookieJar::new(state.oidc.key().clone())
        .add(oidc::login_cookie(value, state.secure_cookies));
    Ok((jar, Redirect::to(url.as_str())).into_response())
}

#[derive(Deserialize, IntoParams)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    /// Set by the provider when it refused (or the person cancelled).
    pub error: Option<String>,
}

/// Back from the provider: check its answer, find or link (or create) the account, and sign in.
/// Redirects to `/` with the session cookie, or to `/login?error=<code>`. The login cookie is cleared
/// either way.
#[utoipa::path(
    get,
    path = "/auth/oidc/callback",
    tag = "auth",
    params(CallbackParams),
    responses(
        (status = 303, description = "To `/` signed in, or to `/login?error=<code>`")
    )
)]
pub async fn oidc_callback(
    State(state): State<AppState>,
    mut auth_session: crate::sessions::AuthSession,
    headers: HeaderMap,
    params: Result<Query<CallbackParams>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let secure = state.secure_cookies;
    // The signed jar drops a cookie whose signature does not verify (one from before a restart, or a
    // forged one); the removal below is sent regardless, so it is cleared too.
    let cookie = SignedCookieJar::from_headers(&headers, state.oidc.key().clone())
        .get(oidc::cookie_name(secure))
        .map(|c| c.value().to_string());
    // A query that does not parse still gets the redirect and the cleared cookie, not a bare 400.
    let outcome = match params {
        Ok(Query(params)) => finish_login(&state, params, cookie).await,
        Err(_) => Err(unaudited(SsoError::State)),
    };
    let removal = oidc::login_cookie_removal(secure);
    match outcome {
        // The account's transaction is committed (and the writer turn given back): now the session.
        Ok(SignedIn::User(user)) => {
            match crate::sessions::sign_in(&mut auth_session, &state, &user).await {
                Ok(session) => (session.add(removal), Redirect::to("/")).into_response(),
                Err(e) => {
                    tracing::error!(error = %e, "single sign-on could not start the session");
                    (CookieJar::new().add(removal), SsoError::Failed.redirect()).into_response()
                }
            }
        }
        // Two-factor first: the challenge cookie, and the login page's code (or setup) step.
        Ok(SignedIn::Challenge(challenge, to)) => {
            (challenge, CookieJar::new().add(removal), Redirect::to(to)).into_response()
        }
        Err(refused) => {
            if refused.audited {
                record_refusal(&state, &refused).await;
            }
            (CookieJar::new().add(removal), refused.error.redirect()).into_response()
        }
    }
}

/// What the provider vouches for.
struct Identity {
    issuer: String,
    subject: String,
    /// Only an email the provider says is verified, lowercased as the app stores it.
    email: Option<String>,
    name: Option<String>,
}

/// Why a callback did not sign anyone in.
struct Refused {
    error: SsoError,
    /// The account it was about, when one was found (`linked`, `disabled`).
    user: Option<uuid::Uuid>,
    /// Recorded in the audit log: the browser came back with a login cookie this server signed, so a
    /// sign-in really was attempted. A bare request to the callback is noise anyone can make, and is not.
    audited: bool,
}

impl From<SsoError> for Refused {
    fn from(error: SsoError) -> Self {
        Self {
            error,
            user: None,
            audited: true,
        }
    }
}

fn unaudited(error: SsoError) -> Refused {
    Refused {
        audited: false,
        ..error.into()
    }
}

async fn finish_login(
    state: &AppState,
    params: CallbackParams,
    cookie: Option<String>,
) -> Result<SignedIn, Refused> {
    let pending: Pending = cookie
        .and_then(|value| serde_json::from_str(&value).ok())
        .ok_or(unaudited(SsoError::State))?;
    let age = state.oidc.now().checked_sub(pending.started);
    if !age.is_some_and(|age| age < LOGIN_TTL_SECS) {
        return Err(unaudited(SsoError::State));
    }
    if params.error.is_some() {
        return Err(SsoError::Denied.into());
    }
    let (Some(code), Some(returned_state)) = (params.code, params.state) else {
        return Err(SsoError::State.into());
    };
    if returned_state != pending.state {
        return Err(SsoError::State.into());
    }
    let config = state.oidc.config().ok_or(SsoError::Unavailable)?;
    if config.issuer != pending.issuer || config.client_id != pending.client_id {
        return Err(SsoError::State.into());
    }
    let identity = verify(state, &config, pending, code).await?;
    sign_in(state, &config, identity)
        .await
        .map_err(|e| match e {
            SignInError::Sso(error, user) => Refused {
                user,
                ..error.into()
            },
            SignInError::Api(e) => {
                tracing::error!(error = ?e, "single sign-on failed in the database");
                SsoError::Failed.into()
            }
        })
}

/// Record a refused sign-in, as a failed password login is (`login_failed`), outside the transaction
/// that was rolled back.
async fn record_refusal(state: &AppState, refused: &Refused) {
    let mut entry = Entry::admin(None, "login_failed")
        .data(serde_json::json!({ "method": "sso", "reason": refused.error.code() }));
    if let Some(user) = refused.user {
        entry = entry.target("user", user);
    }
    let recorded = match state.writer().await {
        Ok(db) => audit::record(&*db, entry).await,
        Err(e) => Err(e),
    };
    if let Err(e) = recorded {
        tracing::error!(error = ?e, "could not record a refused single sign-on");
    }
}

/// Trade the code for tokens at the provider and check the ID token. Network calls only: no database.
async fn verify(
    state: &AppState,
    config: &Config,
    pending: Pending,
    code: String,
) -> Result<Identity, SsoError> {
    let failed = |what: &str, error: &dyn std::error::Error| {
        tracing::warn!(error = oidc::error_chain(error), "single sign-on: {what}");
        SsoError::Failed
    };
    let metadata = state
        .oidc
        .metadata(&config.issuer)
        .await
        .map_err(|_| SsoError::Unavailable)?;
    let callback = RedirectUrl::new(pending.callback).map_err(|_| SsoError::State)?;
    let client = client(&metadata, config, callback.clone());
    let tokens = client
        .exchange_code(AuthorizationCode::new(code))
        .map_err(|e| failed("no token endpoint", &e))?
        .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier))
        .request_async(&state.oidc.http)
        .await
        .map_err(|e| failed("the code was not accepted", &e))?;
    let id_token = tokens.id_token().ok_or_else(|| {
        tracing::warn!("single sign-on: the provider sent no ID token");
        SsoError::Failed
    })?;
    let nonce = Nonce::new(pending.nonce);
    let claims = match id_token.claims(&client.id_token_verifier(), &nonce) {
        Ok(claims) => claims.clone(),
        // Signed with a key the cached set does not have: the provider rotated its keys.
        Err(ClaimsVerificationError::SignatureVerification(
            SignatureVerificationError::NoMatchingKey,
        )) => {
            let metadata = state
                .oidc
                .refresh(&config.issuer)
                .await
                .map_err(|_| SsoError::Unavailable)?;
            let client = self::client(&metadata, config, callback);
            id_token
                .claims(&client.id_token_verifier(), &nonce)
                .map_err(|e| failed("the ID token did not verify", &e))?
                .clone()
        }
        Err(e) => return Err(failed("the ID token did not verify", &e)),
    };
    let email = claims
        .email()
        .filter(|_| claims.email_verified() == Some(true))
        .map(|email| email.as_str().trim().to_lowercase())
        // The same rule as an email typed into the app, and no longer than an address can be.
        .filter(|email| email.len() <= 254 && super::auth::validate_identity(email).is_ok());
    Ok(Identity {
        issuer: claims.issuer().as_str().to_string(),
        subject: claims.subject().as_str().to_string(),
        email,
        name: claims
            .name()
            .and_then(|name| name.get(None))
            .map(|name| name.as_str().trim().chars().take(100).collect()),
    })
}

enum SignInError {
    /// Refused, and about which account if one was found.
    Sso(SsoError, Option<uuid::Uuid>),
    Api(ApiError),
}

impl From<ApiError> for SignInError {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}

impl From<sea_orm::DbErr> for SignInError {
    fn from(e: sea_orm::DbErr) -> Self {
        Self::Api(e.into())
    }
}

/// How a single sign-on ends.
enum SignedIn {
    /// Signed in as this user (the session is started once the transaction is committed).
    User(entity::User),
    /// With a two-factor challenge, and where the login page asks for the code or the setup.
    Challenge(SignedCookieJar, &'static str),
}

/// Find the account linked to the identity, or link the one with its verified email, or create one;
/// then start a session, or a two-factor challenge (`totp::second_step`). One transaction, and no network
/// call inside it.
async fn sign_in(
    state: &AppState,
    config: &Config,
    identity: Identity,
) -> Result<SignedIn, SignInError> {
    let db = state.writer().await?;
    let tx = db.begin().await?;
    // Before setup, an account made here would be a regular one, setup would then be closed ("a user
    // already exists"), and the site would have no admin.
    if users::Entity::find().one(&tx).await?.is_none() {
        return Err(SignInError::Sso(SsoError::Setup, None));
    }
    let linked = user_identities::Entity::find()
        .filter(user_identities::Column::Issuer.eq(&identity.issuer))
        .filter(user_identities::Column::Subject.eq(&identity.subject))
        .one(&tx)
        .await?;
    let user = match linked {
        Some(link) => {
            users::Entity::find_by_id(link.user_id)
                .one_or_not_found(&tx)
                .await?
        }
        None => {
            let email = identity
                .email
                .clone()
                .ok_or(SignInError::Sso(SsoError::Email, None))?;
            let existing = users::Entity::find()
                .filter(users::Column::Email.eq(&email))
                .one(&tx)
                .await?;
            let user = match existing {
                Some(user) => {
                    // A recycled address at the provider is not the same person as the account
                    // already linked there.
                    let other = user_identities::Entity::find()
                        .filter(user_identities::Column::UserId.eq(user.id))
                        .filter(user_identities::Column::Issuer.eq(&identity.issuer))
                        .one(&tx)
                        .await?;
                    if other.is_some() {
                        return Err(SignInError::Sso(SsoError::Linked, Some(user.id)));
                    }
                    user
                }
                None if config.auto_create => create_user(&tx, &email, &identity).await?,
                None => return Err(SignInError::Sso(SsoError::Unknown, None)),
            };
            link(&tx, &user, &identity).await?;
            user
        }
    };
    if !user.active {
        return Err(SignInError::Sso(SsoError::Disabled, Some(user.id)));
    }
    if let Some(mode) = totp::second_step(&tx, state.totp_required, user.id).await? {
        // The link (if just made) is kept; the session waits for the code, which records the login.
        let jar = totp::challenge_jar(&state.challenge_key, state.secure_cookies, user.id, mode)?;
        tx.commit().await?;
        let to = match mode {
            totp::ChallengeMode::Code => "/login?step=totp",
            totp::ChallengeMode::Enroll => "/login?step=totp-setup",
        };
        return Ok(SignedIn::Challenge(jar, to));
    }
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "login")
            .target("user", user.id)
            .data(serde_json::json!({ "method": "sso" })),
    )
    .await?;
    tx.commit().await?;
    Ok(SignedIn::User(user))
}

async fn create_user(
    tx: &impl crate::db::Db,
    email: &str,
    identity: &Identity,
) -> Result<User, SignInError> {
    let user = users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set(email.to_string()),
        name: Set(identity.name.clone().unwrap_or_default()),
        password_hash: Set(None),
        super_admin: Set(false),
        active: Set(true),
        ..Default::default()
    }
    .insert(tx)
    .await?;
    audit::record(
        tx,
        Entry::admin(Some(user.id), "user_created")
            .target("user", user.id)
            .data(serde_json::json!({ "method": "sso" })),
    )
    .await?;
    Ok(user)
}

/// Link the identity to `user`. Someone still invited has now signed in: their invite links stop working.
async fn link(
    tx: &impl crate::db::Db,
    user: &User,
    identity: &Identity,
) -> Result<(), SignInError> {
    user_identities::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        user_id: Set(user.id),
        issuer: Set(identity.issuer.clone()),
        subject: Set(identity.subject.clone()),
        created_at: Set(time::now()),
    }
    .insert(tx)
    .await?;
    user_invites::Entity::delete_many()
        .filter(user_invites::Column::UserId.eq(user.id))
        .exec(tx)
        .await?;
    audit::record(
        tx,
        Entry::admin(Some(user.id), "sso_linked")
            .target("user", user.id)
            .data(serde_json::json!({ "issuer": identity.issuer })),
    )
    .await?;
    Ok(())
}

/// The single sign-on settings, as the super admin sees them. Each value is the one in force: the
/// environment's where it sets one (`locked`), else the stored one.
#[derive(Serialize, ToSchema)]
pub struct OidcSettingsView {
    pub enabled: bool,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    /// Whether a client secret is set; the secret itself is never sent.
    pub has_secret: bool,
    /// The provider's name on the sign-in button.
    pub display_name: Option<String>,
    /// The callback URL to use instead of the derived one.
    pub redirect_url: Option<String>,
    /// Create an account for someone unknown whose provider vouches for their email.
    pub auto_create: bool,
    pub locked: OidcLocked,
    /// The callback URL to register at the provider.
    pub callback_url: Option<String>,
    /// Single sign-on is on and complete (enabled, with an issuer and a client id).
    pub active: bool,
}

/// Which settings the environment sets (`OIDC_*`): they win, and cannot be changed here.
#[derive(Serialize, ToSchema)]
pub struct OidcLocked {
    pub enabled: bool,
    pub issuer: bool,
    pub client_id: bool,
    pub client_secret: bool,
    pub display_name: bool,
    pub redirect_url: bool,
    pub auto_create: bool,
}

async fn oidc_view(state: &AppState, headers: &HeaderMap) -> Result<OidcSettingsView, ApiError> {
    let row = oidc_settings::Entity::find_by_id(1).one(state.db()).await?;
    let env = state.oidc.env();
    let stored =
        |f: fn(&entity::OidcSettings) -> &Option<String>| row.as_ref().and_then(|r| f(r).clone());
    let config = state.oidc.config();
    Ok(OidcSettingsView {
        enabled: oidc::switched_on(env, row.as_ref()),
        issuer: env.issuer.clone().or(stored(|r| &r.issuer)),
        client_id: env.client_id.clone().or(stored(|r| &r.client_id)),
        has_secret: env.client_secret.is_some() || stored(|r| &r.client_secret).is_some(),
        display_name: env.display_name.clone().or(stored(|r| &r.display_name)),
        redirect_url: env.redirect_url.clone().or(stored(|r| &r.redirect_url)),
        auto_create: env
            .auto_create
            .or(row.as_ref().map(|r| r.auto_create))
            .unwrap_or(false),
        locked: OidcLocked {
            // With OIDC_ONLY, single sign-on cannot be switched off.
            enabled: env.only == Some(true),
            issuer: env.issuer.is_some(),
            client_id: env.client_id.is_some(),
            client_secret: env.client_secret.is_some(),
            display_name: env.display_name.is_some(),
            redirect_url: env.redirect_url.is_some(),
            auto_create: env.auto_create.is_some(),
        },
        callback_url: callback_url(config.as_deref(), headers),
        active: config.is_some(),
    })
}

/// The single sign-on settings.
#[utoipa::path(
    get,
    path = "/admin/oidc",
    tag = "admin",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "Single sign-on settings", body = OidcSettingsView),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Not allowed: an API key's scope, or a cross-site request", body = ErrorBody),
        (status = 404, description = "Not the super admin", body = ErrorBody)
    )
)]
pub async fn admin_get_oidc(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<OidcSettingsView>, ApiError> {
    access::require_super_admin(&user)?;
    Ok(Json(oidc_view(&state, &headers).await?))
}

/// The stored settings, replaced as a whole (a text field left out is cleared; a yes/no left out keeps
/// its value). A field the environment sets must be left out (null).
#[derive(Deserialize, ToSchema)]
pub struct UpdateOidcSettings {
    pub enabled: Option<bool>,
    /// The provider's issuer URL (https).
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    /// A new client secret; absent (or null) keeps the stored one.
    pub client_secret: Option<String>,
    /// Remove the stored client secret (a public client).
    #[serde(default)]
    pub remove_secret: bool,
    pub display_name: Option<String>,
    /// The callback URL, when the derived one is wrong (it must end in `/api/auth/oidc/callback`).
    pub redirect_url: Option<String>,
    pub auto_create: Option<bool>,
}

/// Replace the stored single sign-on settings. When they turn single sign-on on, the provider's
/// discovery document is fetched first, and the save is refused if that fails. They apply at once.
#[utoipa::path(
    put,
    path = "/admin/oidc",
    tag = "admin",
    security(("session_cookie" = [])),
    request_body = UpdateOidcSettings,
    responses(
        (status = 200, description = "Saved settings", body = OidcSettingsView),
        (status = 400, description = "Invalid value, a field set by the environment, or the provider could not be reached", body = ErrorBody),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Not allowed: an API key's scope, or a cross-site request", body = ErrorBody),
        (status = 404, description = "Not the super admin", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody)
    )
)]
pub async fn admin_update_oidc(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser { user, .. }: CurrentUser,
    ApiJson(input): ApiJson<UpdateOidcSettings>,
) -> Result<Json<OidcSettingsView>, ApiError> {
    access::require_super_admin(&user)?;
    // Spawned and awaited: a request dropped halfway (the admin closes the tab) cannot leave the
    // settings stored but not in force.
    let task = tokio::spawn({
        let state = state.clone();
        async move { save(&state, user.id, input).await }
    });
    task.await
        .map_err(|_| ApiError::internal("Saving the settings failed"))??;
    Ok(Json(oidc_view(&state, &headers).await?))
}

async fn save(
    state: &AppState,
    actor: uuid::Uuid,
    input: UpdateOidcSettings,
) -> Result<(), ApiError> {
    let env = state.oidc.env();
    let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    for (locked, given, variable) in [
        (env.only == Some(true), input.enabled.is_some(), "OIDC_ONLY"),
        (env.issuer.is_some(), input.issuer.is_some(), "OIDC_ISSUER"),
        (
            env.client_id.is_some(),
            input.client_id.is_some(),
            "OIDC_CLIENT_ID",
        ),
        (
            env.client_secret.is_some(),
            input.client_secret.is_some() || input.remove_secret,
            "OIDC_CLIENT_SECRET",
        ),
        (
            env.display_name.is_some(),
            input.display_name.is_some(),
            "OIDC_LABEL",
        ),
        (
            env.redirect_url.is_some(),
            input.redirect_url.is_some(),
            "OIDC_REDIRECT_URL",
        ),
        (
            env.auto_create.is_some(),
            input.auto_create.is_some(),
            "OIDC_AUTO_CREATE",
        ),
    ] {
        if locked && given {
            return Err(ApiError::bad_request(format!(
                "This setting comes from {variable} and cannot be changed here"
            )));
        }
    }
    let issuer = clean(input.issuer);
    if let Some(issuer) = &issuer
        && !(oidc::secure_url(issuer) && openidconnect::IssuerUrl::new(issuer.clone()).is_ok())
    {
        return Err(ApiError::bad_request(
            "The issuer must be an https URL (http only on localhost)",
        ));
    }
    let redirect_url = clean(input.redirect_url);
    if let Some(url) = &redirect_url {
        oidc::check_redirect_url(url)
            .map_err(|e| ApiError::bad_request(format!("The redirect URL {e}")))?;
    }
    let display_name = clean(input.display_name);
    if display_name
        .as_ref()
        .is_some_and(|n| n.chars().count() > 100)
    {
        return Err(ApiError::bad_request(
            "The display name is longer than 100 characters",
        ));
    }
    let new_secret = clean(input.client_secret);

    // From reading the stored row until the new settings are in force: concurrent saves would undo each
    // other's fields, and memory and the database would drift apart.
    let _save = state.oidc.save.lock().await;
    let stored = oidc_settings::Entity::find_by_id(1).one(state.db()).await?;
    // A field the environment sets keeps what is stored under it (it is not in force, but comes back if
    // the variable is removed), so it neither changes nor clears anything here.
    let keep =
        |locked: bool, given: Option<String>, f: fn(&entity::OidcSettings) -> &Option<String>| {
            if locked {
                stored.as_ref().and_then(|r| f(r).clone())
            } else {
                given
            }
        };
    let issuer = keep(env.issuer.is_some(), issuer, |r| &r.issuer);
    let issuer_changed = stored.as_ref().and_then(|r| r.issuer.clone()) != issuer;
    // The secret goes to the issuer's token endpoint: an admin who could point the environment's secret
    // at another issuer could send it to a server of theirs.
    if issuer_changed && env.client_secret.is_some() && env.issuer.is_none() {
        return Err(ApiError::bad_request(
            "The client secret comes from OIDC_CLIENT_SECRET, so the issuer must come from OIDC_ISSUER",
        ));
    }
    let client_secret = if env.client_secret.is_some() {
        stored.as_ref().and_then(|r| r.client_secret.clone())
    } else if new_secret.is_some() {
        new_secret
    } else if input.remove_secret || issuer_changed {
        // A secret belongs to its issuer: a new issuer needs its own.
        None
    } else {
        stored.as_ref().and_then(|r| r.client_secret.clone())
    };
    let row = entity::OidcSettings {
        id: 1,
        enabled: input
            .enabled
            .or(stored.as_ref().map(|r| r.enabled))
            .unwrap_or(true),
        issuer,
        client_id: keep(env.client_id.is_some(), clean(input.client_id), |r| {
            &r.client_id
        }),
        client_secret,
        display_name: keep(env.display_name.is_some(), display_name, |r| {
            &r.display_name
        }),
        redirect_url: keep(env.redirect_url.is_some(), redirect_url, |r| {
            &r.redirect_url
        }),
        auto_create: input
            .auto_create
            .or(stored.as_ref().map(|r| r.auto_create))
            .unwrap_or(false),
    };

    // Ask the provider before writing anything, and outside any transaction.
    if let Some(config) = oidc::effective(env, Some(&row)) {
        state.oidc.check(&config.issuer).await.map_err(|e| {
            ApiError::bad_request(format!("The provider could not be checked: {e}"))
        })?;
    }

    let db = state.writer().await?;
    let tx = db.begin().await?;
    let active = oidc_settings::ActiveModel {
        id: Set(1),
        enabled: Set(row.enabled),
        issuer: Set(row.issuer.clone()),
        client_id: Set(row.client_id.clone()),
        client_secret: Set(row.client_secret.clone()),
        display_name: Set(row.display_name.clone()),
        redirect_url: Set(row.redirect_url.clone()),
        auto_create: Set(row.auto_create),
    };
    if stored.is_some() {
        active.update(&tx).await?;
    } else {
        active.insert(&tx).await?;
    }
    audit::record(&tx, Entry::admin(Some(actor), "oidc_settings_changed")).await?;
    tx.commit().await?;
    state.oidc.apply(Some(&row));
    Ok(())
}
