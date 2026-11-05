//! Single sign-on, against a stand-in provider served on 127.0.0.1: discovery, a key set and a token
//! endpoint, signing real RS256 ID tokens with the crate's own signing key. Requests go through the
//! router without following redirects, so the `Location` and `Set-Cookie` headers are checked here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Form, State as AxumState};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum_extra::extract::cookie::{Cookie, SignedCookieJar};
use base64::Engine;
use openidconnect::core::{
    CoreIdToken, CoreIdTokenClaims, CoreJsonWebKeySet, CoreJwsSigningAlgorithm,
    CoreRsaPrivateSigningKey,
};
use openidconnect::{JsonWebKeyId, PrivateSigningKey};
use serde_json::{Value, json};
use sha2::Digest;

use api::db::prelude::*;
use api::oidc::{EnvSettings, Oidc};
use api::state::AppState;

use crate::common::*;

const CLIENT_ID: &str = "woodmouse";
const CLIENT_SECRET: &str = "s3cret";
const HOST: &str = "woodmouse.test";
const CALLBACK: &str = "http://woodmouse.test/api/auth/oidc/callback";

/// The provider's keys: `a` and `b` are its own (published as it chooses), `foreign` never is.
const KEY_A: &str = include_str!("fixtures/oidc_key_a.pem");
const KEY_B: &str = include_str!("fixtures/oidc_key_b.pem");
const KEY_FOREIGN: &str = include_str!("fixtures/oidc_key_foreign.pem");

fn key(kid: &str) -> CoreRsaPrivateSigningKey {
    let pem = match kid {
        "a" => KEY_A,
        "b" => KEY_B,
        _ => KEY_FOREIGN,
    };
    CoreRsaPrivateSigningKey::from_pem(pem, Some(JsonWebKeyId::new(kid.to_string()))).unwrap()
}

/// What the stand-in provider does, changeable while a test runs.
struct Shared {
    issuer: String,
    discovery_hits: usize,
    jwks_hits: usize,
    /// Discovery answers 503.
    down: bool,
    /// Discovery answers with more than a megabyte.
    oversized: bool,
    /// The kids in the published key set.
    published: Vec<&'static str>,
    /// Discovery points the key set somewhere else (not https).
    jwks_uri: Option<String>,
    /// Discovery sends the browser somewhere else (not https).
    authorization_uri: Option<String>,
    /// Discovery calls the provider by another name.
    claimed_issuer: Option<String>,
    /// Code -> (PKCE challenge, redirect URI, ID token). Each works once.
    codes: HashMap<String, (String, String, String)>,
    /// The `Authorization` header the token endpoint requires; `None` for a public client.
    client_auth: Option<String>,
}

#[derive(Clone)]
struct Provider {
    issuer: String,
    shared: Arc<Mutex<Shared>>,
}

impl Provider {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let basic = base64::engine::general_purpose::STANDARD
            .encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"));
        let shared = Arc::new(Mutex::new(Shared {
            issuer: issuer.clone(),
            discovery_hits: 0,
            jwks_hits: 0,
            down: false,
            oversized: false,
            published: vec!["a"],
            jwks_uri: None,
            authorization_uri: None,
            claimed_issuer: None,
            codes: HashMap::new(),
            client_auth: Some(format!("Basic {basic}")),
        }));
        let app = axum::Router::new()
            .route(
                "/.well-known/openid-configuration",
                axum::routing::get(discovery),
            )
            .route("/jwks", axum::routing::get(jwks))
            .route("/token", axum::routing::post(token))
            .with_state(shared.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { issuer, shared }
    }

    fn with<T>(&self, f: impl FnOnce(&mut Shared) -> T) -> T {
        f(&mut self.shared.lock().unwrap())
    }

    /// The claims of a good ID token for this login.
    fn claims(&self, login: &Login, sub: &str, email: Option<&str>) -> Value {
        let now = jiff::Timestamp::now().as_second();
        let mut claims = json!({
            "iss": self.issuer,
            "aud": [CLIENT_ID],
            "exp": now + 300,
            "iat": now,
            "sub": sub,
            "nonce": login.nonce,
        });
        if let Some(email) = email {
            claims["email"] = json!(email);
            claims["email_verified"] = json!(true);
        }
        claims
    }

    /// The person signs in at the provider: it remembers the code with this ID token (signed with
    /// `kid`'s key, under `as_kid`) and the login's PKCE challenge.
    fn authorize_signed(&self, login: &Login, claims: Value, kid: &str, as_kid: &str) -> String {
        let claims: CoreIdTokenClaims = serde_json::from_value(claims).unwrap();
        let signing = CoreRsaPrivateSigningKey::from_pem(
            match kid {
                "a" => KEY_A,
                "b" => KEY_B,
                _ => KEY_FOREIGN,
            },
            Some(JsonWebKeyId::new(as_kid.to_string())),
        )
        .unwrap();
        let token = CoreIdToken::new(
            claims,
            &signing,
            CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
            None,
            None,
        )
        .unwrap()
        .to_string();
        let code = uuid::Uuid::new_v4().to_string();
        self.with(|s| {
            s.codes.insert(
                code.clone(),
                (login.challenge.clone(), login.redirect_uri.clone(), token),
            )
        });
        code
    }

    fn authorize(&self, login: &Login, claims: Value) -> String {
        self.authorize_signed(login, claims, "a", "a")
    }
}

async fn discovery(AxumState(shared): AxumState<Arc<Mutex<Shared>>>) -> axum::response::Response {
    let mut s = shared.lock().unwrap();
    s.discovery_hits += 1;
    if s.down {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let issuer = s.issuer.clone();
    let mut doc = json!({
        "issuer": s.claimed_issuer.clone().unwrap_or(issuer.clone()),
        "authorization_endpoint": s.authorization_uri.clone().unwrap_or(format!("{issuer}/authorize")),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": s.jwks_uri.clone().unwrap_or(format!("{issuer}/jwks")),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    });
    if s.oversized {
        doc["padding"] = json!("x".repeat(1_100_000));
    }
    axum::Json(doc).into_response()
}

async fn jwks(AxumState(shared): AxumState<Arc<Mutex<Shared>>>) -> axum::response::Response {
    let mut s = shared.lock().unwrap();
    s.jwks_hits += 1;
    let keys = CoreJsonWebKeySet::new(
        s.published
            .iter()
            .map(|kid| key(kid).as_verification_key())
            .collect(),
    );
    axum::Json(keys).into_response()
}

async fn token(
    AxumState(shared): AxumState<Arc<Mutex<Shared>>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> axum::response::Response {
    let mut s = shared.lock().unwrap();
    let refuse = || {
        (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "invalid_grant"})),
        )
            .into_response()
    };
    let auth = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .map(str::to_string);
    if auth != s.client_auth {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"error": "invalid_client"})),
        )
            .into_response();
    }
    // A code works once: a replayed callback fails here.
    let Some((challenge, redirect_uri, id_token)) =
        form.get("code").and_then(|code| s.codes.remove(code))
    else {
        return refuse();
    };
    let verifier = form.get("code_verifier").cloned().unwrap_or_default();
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    if computed != challenge || form.get("redirect_uri") != Some(&redirect_uri) {
        return refuse();
    }
    axum::Json(json!({
        "access_token": "an-access-token",
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": id_token,
    }))
    .into_response()
}

fn env_for(provider: &Provider) -> EnvSettings {
    EnvSettings {
        issuer: Some(provider.issuer.clone()),
        client_id: Some(CLIENT_ID.to_string()),
        client_secret: Some(CLIENT_SECRET.to_string()),
        ..Default::default()
    }
}

async fn setup_with(oidc: Oidc, secure_cookies: bool) -> (axum::Router, AppState) {
    let mut state = AppState::new(api::db::temporary().await).with_oidc(oidc);
    state.secure_cookies = secure_cookies;
    (router(state.clone()), state)
}

async fn setup(provider: &Provider) -> (axum::Router, AppState) {
    setup_with(Oidc::new(env_for(provider)), false).await
}

/// A login as the app started it: what it sent the browser to the provider with, and its cookie.
struct Login {
    state: String,
    nonce: String,
    challenge: String,
    redirect_uri: String,
    /// `name=value` of the login cookie.
    cookie: String,
}

fn set_cookies(res: &axum::http::Response<Body>) -> Vec<String> {
    res.headers()
        .get_all("set-cookie")
        .iter()
        .map(|h| h.to_str().unwrap().to_string())
        .collect()
}

fn location(res: &axum::http::Response<Body>) -> String {
    res.headers()
        .get("location")
        .expect("a redirect")
        .to_str()
        .unwrap()
        .to_string()
}

async fn start(app: &axum::Router) -> axum::http::Response<Body> {
    request(
        app,
        Request::builder()
            .uri("/api/auth/oidc/login")
            .header("host", HOST)
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

async fn login(app: &axum::Router) -> Login {
    let res = start(app).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let url = openidconnect::url::Url::parse(&location(&res)).expect("to the provider");
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], CLIENT_ID);
    assert_eq!(query["code_challenge_method"], "S256");
    assert!(query["scope"].contains("openid") && query["scope"].contains("email"));
    let cookies = set_cookies(&res);
    assert_eq!(cookies.len(), 1, "{cookies:?}");
    Login {
        state: query["state"].clone(),
        nonce: query["nonce"].clone(),
        challenge: query["code_challenge"].clone(),
        redirect_uri: query["redirect_uri"].clone(),
        cookie: cookies[0].split(';').next().unwrap().to_string(),
    }
}

async fn callback(
    app: &axum::Router,
    query: &str,
    cookie: Option<&str>,
) -> axum::http::Response<Body> {
    request(
        app,
        get(&format!("/api/auth/oidc/callback?{query}"), cookie),
    )
    .await
}

/// The callback's answer: where it sends the browser, and the session cookie if it signed someone in.
async fn finish(app: &axum::Router, login: &Login, code: &str) -> (String, Option<String>) {
    let res = callback(
        app,
        &format!("code={code}&state={}", login.state),
        Some(&login.cookie),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let cookies = set_cookies(&res);
    // The login cookie is cleared, whatever the outcome.
    assert!(
        cookies
            .iter()
            .any(|c| c.starts_with("woodmouse_oidc=;") && c.contains("Max-Age=0")),
        "the login cookie is not cleared: {cookies:?}"
    );
    let session = cookies
        .iter()
        .find(|c| c.starts_with(&format!("{}=", api::auth::SESSION_COOKIE)))
        .map(|c| c.split(';').next().unwrap().to_string());
    (location(&res), session)
}

/// Sign in at the provider as `sub` with `email` (verified), and come back.
async fn sign_in(
    app: &axum::Router,
    provider: &Provider,
    sub: &str,
    email: Option<&str>,
) -> (String, Option<String>) {
    let login = login(app).await;
    let code = provider.authorize(&login, provider.claims(&login, sub, email));
    finish(app, &login, &code).await
}

async fn me(app: &axum::Router, session: &str) -> Value {
    let res = request(app, get("/api/auth/me", Some(session))).await;
    assert_eq!(res.status(), StatusCode::OK);
    serde_json::from_str(&body_string(res.into_body()).await).unwrap()
}

fn error(code: &str) -> String {
    format!("/login?error={code}")
}

async fn add_user(state: &AppState, email: &str, password: bool, active: bool) -> uuid::Uuid {
    entity::users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set(email.to_string()),
        name: Set(String::new()),
        password_hash: Set(password.then(|| "not-a-login-hash".to_string())),
        super_admin: Set(false),
        active: Set(active),
        ..Default::default()
    }
    .insert(state.db())
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn an_account_is_matched_by_verified_email_then_by_subject() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    bootstrap_admin(&app).await; // admin@home.io

    let (to, session) = sign_in(&app, &provider, "sub-1", Some("Admin@Home.io")).await;
    assert_eq!(to, "/");
    let user = me(&app, &session.unwrap()).await;
    assert_eq!(user["email"], "admin@home.io");
    assert_eq!(user["has_password"], true);

    // Linked now: the subject finds the account even with another email, or none.
    let (to, session) = sign_in(&app, &provider, "sub-1", None).await;
    assert_eq!(to, "/");
    assert_eq!(me(&app, &session.unwrap()).await["email"], "admin@home.io");
}

#[tokio::test]
async fn single_sign_on_does_not_ask_for_a_two_factor_code() {
    let provider = Provider::start().await;
    let (app, state) = setup(&provider).await;
    let admin = bootstrap_admin(&app).await;
    let id = me(&app, &admin).await["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    entity::user_totp::ActiveModel {
        user_id: Set(id),
        secret: Set(api::totp::new_secret()),
        enabled_at: Set(Some(api::time::now())),
        last_step: Set(None),
        created_at: Set(api::time::now()),
    }
    .insert(state.db())
    .await
    .unwrap();

    // The provider is where its second factor belongs.
    let (to, session) = sign_in(&app, &provider, "sub-1", Some("admin@home.io")).await;
    assert_eq!(to, "/");
    let user = me(&app, &session.unwrap()).await;
    assert_eq!(user["totp_enabled"], true);
}

#[tokio::test]
async fn an_email_the_provider_does_not_vouch_for_matches_nobody() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    bootstrap_admin(&app).await;

    let login = login(&app).await;
    let mut claims = provider.claims(&login, "sub-1", Some("admin@home.io"));
    claims["email_verified"] = json!(false);
    let code = provider.authorize(&login, claims);
    assert_eq!(finish(&app, &login, &code).await, (error("email"), None));

    // Not saying (Microsoft Entra never does) is not vouching either.
    let login = self::login(&app).await;
    let mut claims = provider.claims(&login, "sub-1", Some("admin@home.io"));
    claims.as_object_mut().unwrap().remove("email_verified");
    let code = provider.authorize(&login, claims);
    assert_eq!(finish(&app, &login, &code).await, (error("email"), None));

    assert_eq!(
        sign_in(&app, &provider, "sub-1", None).await,
        (error("email"), None)
    );
    // Vouched for, but not an address.
    let long = format!("{}@home.io", "a".repeat(300));
    for email in ["not-an-email", long.as_str()] {
        assert_eq!(
            sign_in(&app, &provider, "sub-1", Some(email)).await,
            (error("email"), None),
            "{email}"
        );
    }
}

#[tokio::test]
async fn someone_unknown_is_refused_unless_accounts_are_created() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    bootstrap_admin(&app).await;
    assert_eq!(
        sign_in(&app, &provider, "sub-9", Some("new@home.io")).await,
        (error("unknown"), None)
    );

    let (app, state) = setup_with(
        Oidc::new(EnvSettings {
            auto_create: Some(true),
            ..env_for(&provider)
        }),
        false,
    )
    .await;
    bootstrap_admin(&app).await;
    let (to, session) = sign_in(&app, &provider, "sub-9", Some("new@home.io")).await;
    assert_eq!(to, "/");
    let session = session.unwrap();
    let user = me(&app, &session).await;
    assert_eq!(user["email"], "new@home.io");
    assert_eq!(user["super_admin"], false);
    assert_eq!(user["global_role"], Value::Null);
    assert_eq!(user["has_password"], false);
    let row = entity::users::Entity::find()
        .filter(entity::users::Column::Email.eq("new@home.io"))
        .one(state.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.password_hash, None);

    // They can set a first password, with no current one.
    let res = request(
        &app,
        post_json(
            "/api/auth/password",
            r#"{"new_password":"a first password"}"#,
            Some(&session),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn nobody_signs_in_before_setup() {
    let provider = Provider::start().await;
    let (app, state) = setup_with(
        Oidc::new(EnvSettings {
            auto_create: Some(true),
            ..env_for(&provider)
        }),
        false,
    )
    .await;
    assert_eq!(
        sign_in(&app, &provider, "sub-1", Some("first@home.io")).await,
        (error("setup"), None)
    );
    assert!(
        entity::users::Entity::find()
            .all(state.db())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_account_linked_to_another_subject_is_not_taken_over_by_email() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    bootstrap_admin(&app).await;
    assert_eq!(
        sign_in(&app, &provider, "sub-1", Some("admin@home.io"))
            .await
            .0,
        "/"
    );
    // A recycled address: another account at the same provider with the same verified email.
    assert_eq!(
        sign_in(&app, &provider, "sub-2", Some("admin@home.io")).await,
        (error("linked"), None)
    );
}

#[tokio::test]
async fn a_disabled_account_is_refused_and_not_linked() {
    let provider = Provider::start().await;
    let (app, state) = setup(&provider).await;
    bootstrap_admin(&app).await;
    add_user(&state, "off@home.io", true, false).await;
    assert_eq!(
        sign_in(&app, &provider, "sub-3", Some("off@home.io")).await,
        (error("disabled"), None)
    );
    assert!(
        entity::user_identities::Entity::find()
            .all(state.db())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_invited_person_who_signs_in_is_no_longer_pending() {
    let provider = Provider::start().await;
    let (app, state) = setup(&provider).await;
    let admin = bootstrap_admin(&app).await;
    let res = request(
        &app,
        post_json(
            "/api/admin/users",
            r#"{"email":"invited@home.io","name":"","memberships":[]}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_str(&body_string(res.into_body()).await).unwrap();
    assert_eq!(created["user"]["pending"], true);

    assert_eq!(
        sign_in(&app, &provider, "sub-4", Some("invited@home.io"))
            .await
            .0,
        "/"
    );
    assert!(
        entity::user_invites::Entity::find()
            .all(state.db())
            .await
            .unwrap()
            .is_empty()
    );
    let res = request(&app, get("/api/admin/users", Some(&admin))).await;
    let users: Value = serde_json::from_str(&body_string(res.into_body()).await).unwrap();
    let invited = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["email"] == "invited@home.io")
        .unwrap();
    assert_eq!(invited["pending"], false);
}

/// Each way the ID token can be wrong is refused, and nobody is signed in.
#[tokio::test]
async fn a_bad_id_token_is_refused() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    bootstrap_admin(&app).await;
    let now = jiff::Timestamp::now().as_second();
    type Change = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Change)> = vec![
        (
            "wrong nonce",
            Box::new(|c| c["nonce"] = json!("another nonce")),
        ),
        (
            "no nonce",
            Box::new(|c| {
                c.as_object_mut().unwrap().remove("nonce");
            }),
        ),
        (
            "other audience",
            Box::new(|c| c["aud"] = json!(["someone-else"])),
        ),
        (
            "extra audience",
            Box::new(|c| c["aud"] = json!([CLIENT_ID, "someone-else"])),
        ),
        (
            "other issuer",
            Box::new(|c| c["iss"] = json!("http://127.0.0.1:1")),
        ),
        ("expired", Box::new(move |c| c["exp"] = json!(now - 600))),
    ];
    for (what, change) in cases {
        let login = login(&app).await;
        let mut claims = provider.claims(&login, "sub-1", Some("admin@home.io"));
        change(&mut claims);
        let code = provider.authorize(&login, claims);
        assert_eq!(
            finish(&app, &login, &code).await,
            (error("failed"), None),
            "{what}"
        );
    }

    // Signed with a key the provider never published, under a kid it does not have, or under its
    // own kid.
    for as_kid in ["foreign", "a"] {
        let login = login(&app).await;
        let claims = provider.claims(&login, "sub-1", Some("admin@home.io"));
        let code = provider.authorize_signed(&login, claims, "foreign", as_kid);
        assert_eq!(
            finish(&app, &login, &code).await,
            (error("failed"), None),
            "foreign key as {as_kid}"
        );
    }
}

#[tokio::test]
async fn the_login_state_must_be_this_browsers_and_fresh() {
    let provider = Provider::start().await;
    let (app, state) = setup(&provider).await;
    bootstrap_admin(&app).await;

    // Another state than the one this browser left with.
    let login = login(&app).await;
    let code = provider.authorize(
        &login,
        provider.claims(&login, "sub-1", Some("admin@home.io")),
    );
    let res = callback(
        &app,
        &format!("code={code}&state=forged"),
        Some(&login.cookie),
    )
    .await;
    assert_eq!(location(&res), error("state"));

    // Another browser: no login cookie.
    let login = self::login(&app).await;
    let code = provider.authorize(
        &login,
        provider.claims(&login, "sub-1", Some("admin@home.io")),
    );
    let res = callback(&app, &format!("code={code}&state={}", login.state), None).await;
    assert_eq!(location(&res), error("state"));
    assert!(
        set_cookies(&res)
            .iter()
            .any(|c| c.starts_with("woodmouse_oidc=;")),
        "cleared even when absent"
    );

    // A forged cookie: the state and nonce are in the provider URL for anyone to see, but an unsigned
    // cookie built from them is not accepted.
    let login = self::login(&app).await;
    let forged = json!({
        "state": login.state,
        "nonce": login.nonce,
        "pkce_verifier": "chosen-by-the-attacker",
        "callback": CALLBACK,
        "issuer": provider.issuer,
        "client_id": CLIENT_ID,
        "started": state.oidc.now(),
    });
    let code = provider.authorize(
        &login,
        provider.claims(&login, "sub-1", Some("admin@home.io")),
    );
    let cookie = format!(
        "woodmouse_oidc={}",
        Cookie::new("x", forged.to_string())
            .encoded()
            .to_string()
            .trim_start_matches("x=")
    );
    let res = callback(
        &app,
        &format!("code={code}&state={}", login.state),
        Some(&cookie),
    )
    .await;
    assert_eq!(location(&res), error("state"));
    assert!(
        set_cookies(&res)
            .iter()
            .any(|c| c.starts_with("woodmouse_oidc=;")),
        "a cookie that does not verify is cleared too"
    );

    // The provider refused (or the person cancelled): a fixed code, never the provider's words.
    let res = callback(
        &app,
        &format!(
            "error=access_denied&error_description=%3Cscript%3E&state={}",
            login.state
        ),
        Some(&login.cookie),
    )
    .await;
    assert_eq!(location(&res), error("denied"));
}

#[tokio::test]
async fn a_login_older_than_ten_minutes_is_refused() {
    let provider = Provider::start().await;
    let oidc = Oidc::new(env_for(&provider)).started_ago(std::time::Duration::from_secs(3600));
    assert!(oidc.now() >= 3600, "the machine has been up for an hour");
    let (app, state) = setup_with(oidc, false).await;
    bootstrap_admin(&app).await;

    let login = login(&app).await;
    // The same login, signed by the app, but started eleven minutes ago.
    let mut pending: Value = serde_json::from_str(
        SignedCookieJar::from_headers(
            &[(axum::http::header::COOKIE, login.cookie.parse().unwrap())]
                .into_iter()
                .collect(),
            state.oidc.key().clone(),
        )
        .get("woodmouse_oidc")
        .unwrap()
        .value(),
    )
    .unwrap();
    pending["started"] = json!(state.oidc.now() - 660);
    let jar = SignedCookieJar::new(state.oidc.key().clone())
        .add(Cookie::new("woodmouse_oidc", pending.to_string()));
    let res = (jar, ()).into_response();
    let old = set_cookies(&res)[0].split(';').next().unwrap().to_string();

    let code = provider.authorize(
        &login,
        provider.claims(&login, "sub-1", Some("admin@home.io")),
    );
    let res = callback(
        &app,
        &format!("code={code}&state={}", login.state),
        Some(&old),
    )
    .await;
    assert_eq!(location(&res), error("state"));
    // The same, fresh, works.
    let code = provider.authorize(
        &login,
        provider.claims(&login, "sub-1", Some("admin@home.io")),
    );
    assert_eq!(finish(&app, &login, &code).await.0, "/");
}

#[tokio::test]
async fn a_replayed_callback_fails_at_the_provider() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    bootstrap_admin(&app).await;
    let login = login(&app).await;
    let code = provider.authorize(
        &login,
        provider.claims(&login, "sub-1", Some("admin@home.io")),
    );
    assert_eq!(finish(&app, &login, &code).await.0, "/");
    // The same callback again, cookie and all: the code works once.
    assert_eq!(finish(&app, &login, &code).await, (error("failed"), None));
}

#[tokio::test]
async fn the_cookies_are_scoped_and_cleared_with_the_same_attributes() {
    let provider = Provider::start().await;

    // Plain http (development): the plain name, only on the callback path.
    let (app, _) = setup(&provider).await;
    let res = start(&app).await;
    let cookie = &set_cookies(&res)[0];
    assert!(cookie.starts_with("woodmouse_oidc="), "{cookie}");
    for part in [
        "HttpOnly",
        "SameSite=Lax",
        "Path=/api/auth/oidc/callback",
        "Max-Age=600",
    ] {
        assert!(cookie.contains(part), "{cookie} lacks {part}");
    }
    assert!(!cookie.contains("Secure"), "{cookie}");

    // https: `__Host-`, Secure, the whole site; and the removal says the same, or browsers ignore it.
    let (app, state) = setup_with(
        Oidc::new(EnvSettings {
            redirect_url: Some("https://woodmouse.test/api/auth/oidc/callback".to_string()),
            ..env_for(&provider)
        }),
        true,
    )
    .await;
    bootstrap_admin(&app).await;
    let res = start(&app).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let cookie = set_cookies(&res)[0].clone();
    assert!(cookie.starts_with("__Host-woodmouse_oidc="), "{cookie}");
    for part in ["Secure", "HttpOnly", "SameSite=Lax", "Path=/;"] {
        assert!(format!("{cookie};").contains(part), "{cookie} lacks {part}");
    }
    let url = openidconnect::url::Url::parse(&location(&res)).unwrap();
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let login = Login {
        state: query["state"].clone(),
        nonce: query["nonce"].clone(),
        challenge: query["code_challenge"].clone(),
        redirect_uri: query["redirect_uri"].clone(),
        cookie: cookie.split(';').next().unwrap().to_string(),
    };
    assert_eq!(
        login.redirect_uri,
        "https://woodmouse.test/api/auth/oidc/callback"
    );
    let code = provider.authorize(
        &login,
        provider.claims(&login, "sub-1", Some("admin@home.io")),
    );
    let res = callback(
        &app,
        &format!("code={code}&state={}", login.state),
        Some(&login.cookie),
    )
    .await;
    assert_eq!(location(&res), "/");
    let cookies = set_cookies(&res);
    let removal = cookies
        .iter()
        .find(|c| c.starts_with("__Host-woodmouse_oidc=;"))
        .expect("the login cookie is cleared");
    for part in ["Secure", "HttpOnly", "SameSite=Lax", "Path=/;", "Max-Age=0"] {
        assert!(
            format!("{removal};").contains(part),
            "{removal} lacks {part}"
        );
    }
    // The session cookie is set on the redirect itself.
    let session = cookies
        .iter()
        .find(|c| c.starts_with(&format!("{}=", api::auth::SECURE_SESSION_COOKIE)))
        .unwrap();
    assert!(session.contains("Secure"), "{session}");
    drop(state);
}

#[tokio::test]
async fn mixed_schemes_are_refused_as_misconfigured() {
    let provider = Provider::start().await;
    // An http callback with secure cookies on.
    let (app, _) = setup_with(Oidc::new(env_for(&provider)), true).await;
    let res = start(&app).await;
    assert_eq!(location(&res), error("misconfigured"));
    assert!(set_cookies(&res).is_empty());
    // An https callback with secure cookies off.
    let (app, _) = setup_with(
        Oidc::new(EnvSettings {
            redirect_url: Some("https://woodmouse.test/api/auth/oidc/callback".to_string()),
            ..env_for(&provider)
        }),
        false,
    )
    .await;
    assert_eq!(location(&start(&app).await), error("misconfigured"));
}

#[tokio::test]
async fn a_provider_that_is_down_is_not_asked_again_at_once() {
    let provider = Provider::start().await;
    provider.with(|s| s.down = true);
    let (app, _) = setup(&provider).await;
    let admin = bootstrap_admin(&app).await;
    assert_eq!(location(&start(&app).await), error("unavailable"));
    assert_eq!(location(&start(&app).await), error("unavailable"));
    assert_eq!(provider.with(|s| s.discovery_hits), 1, "backing off");

    // Back up: logins still wait out the back-off, but the admin's check always asks, and clears it.
    provider.with(|s| s.down = false);
    assert_eq!(location(&start(&app).await), error("unavailable"));
    assert_eq!(provider.with(|s| s.discovery_hits), 1);
    let res = request(&app, put_json("/api/admin/oidc", "{}", Some(&admin))).await;
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "{}",
        body_string(res.into_body()).await
    );
    assert_eq!(provider.with(|s| s.discovery_hits), 2);
    assert_eq!(start(&app).await.status(), StatusCode::SEE_OTHER);
    assert!(location(&start(&app).await).starts_with(&provider.issuer));
    // Cached now.
    assert_eq!(provider.with(|s| s.discovery_hits), 2);
}

#[tokio::test]
async fn rotated_keys_are_fetched_again_once() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    bootstrap_admin(&app).await;
    assert_eq!(
        sign_in(&app, &provider, "sub-1", Some("admin@home.io"))
            .await
            .0,
        "/"
    );
    assert_eq!(provider.with(|s| (s.discovery_hits, s.jwks_hits)), (1, 1));

    // The provider rotates to key b; the cached set has only a.
    provider.with(|s| s.published = vec!["b"]);
    let login = login(&app).await;
    let code = provider.authorize_signed(&login, provider.claims(&login, "sub-1", None), "b", "b");
    assert_eq!(finish(&app, &login, &code).await.0, "/");
    assert_eq!(provider.with(|s| (s.discovery_hits, s.jwks_hits)), (2, 2));
}

/// The provider's metadata must be https (or this machine) and small; the key set is not fetched from
/// an insecure URL at all.
#[tokio::test]
async fn insecure_or_oversized_metadata_is_refused() {
    let provider = Provider::start().await;
    let (app, state) = setup_with(Oidc::new(EnvSettings::default()), false).await;
    let admin = bootstrap_admin(&app).await;
    let save = |body: Value| {
        let (app, admin) = (app.clone(), admin.clone());
        async move {
            let res = request(
                &app,
                put_json("/api/admin/oidc", &body.to_string(), Some(&admin)),
            )
            .await;
            (res.status(), body_string(res.into_body()).await)
        }
    };
    let settings = json!({
        "enabled": true,
        "issuer": provider.issuer,
        "client_id": CLIENT_ID,
        "client_secret": CLIENT_SECRET,
    });

    provider.with(|s| s.jwks_uri = Some("http://keys.example.com/jwks".to_string()));
    let (status, body) = save(settings.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("not https"), "{body}");
    assert_eq!(provider.with(|s| s.jwks_hits), 0);

    provider.with(|s| {
        s.jwks_uri = None;
        s.authorization_uri = Some("http://login.example.com/authorize".to_string());
    });
    let (status, body) = save(settings.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("authorization endpoint"), "{body}");

    provider.with(|s| {
        s.authorization_uri = None;
        s.oversized = true;
    });
    let (status, body) = save(settings.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("larger than"), "{body}");

    provider.with(|s| {
        s.oversized = false;
        s.claimed_issuer = Some("http://127.0.0.1:1".to_string());
    });
    let (status, body) = save(settings.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("calls itself"), "{body}");

    let (status, body) = save(json!({
        "enabled": true,
        "issuer": "http://id.example.com",
        "client_id": CLIENT_ID,
    }))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("https"), "{body}");

    // Nothing was stored, and single sign-on is still off.
    assert!(
        entity::oidc_settings::Entity::find()
            .all(state.db())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(state.oidc.config().is_none());

    provider.with(|s| s.claimed_issuer = None);
    let (status, body) = save(settings).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(state.oidc.config().is_some());
}

#[tokio::test]
async fn settings_apply_at_once_and_never_return_the_secret() {
    let provider = Provider::start().await;
    let (app, state) = setup_with(Oidc::new(EnvSettings::default()), false).await;
    let admin = bootstrap_admin(&app).await;
    let status = || async {
        let res = request(&app, get("/api/auth/status", None)).await;
        serde_json::from_str::<Value>(&body_string(res.into_body()).await).unwrap()
    };
    assert_eq!(status().await["sso"], Value::Null);
    assert_eq!(location(&start(&app).await), error("unavailable"));

    let res = request(
        &app,
        put_json(
            "/api/admin/oidc",
            &json!({
                "enabled": true,
                "issuer": provider.issuer,
                "client_id": CLIENT_ID,
                "client_secret": CLIENT_SECRET,
                "display_name": "Keycloak",
            })
            .to_string(),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res.into_body()).await;
    assert!(!body.contains(CLIENT_SECRET), "{body}");
    let view: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(view["has_secret"], true);
    assert_eq!(view["active"], true);
    assert_eq!(
        view["callback_url"],
        Value::Null,
        "no Host header in this request"
    );
    let res = request(
        &app,
        Request::builder()
            .uri("/api/admin/oidc")
            .header("host", HOST)
            .header("cookie", &admin)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let view: Value = serde_json::from_str(&body_string(res.into_body()).await).unwrap();
    assert_eq!(view["callback_url"], CALLBACK);

    // In force without a restart.
    assert_eq!(status().await["sso"], "Keycloak");
    assert_eq!(
        sign_in(&app, &provider, "sub-1", Some("admin@home.io"))
            .await
            .0,
        "/"
    );

    // Saving again without a secret keeps it; removing it removes it.
    let keep = json!({"enabled": true, "issuer": provider.issuer, "client_id": CLIENT_ID});
    let res = request(
        &app,
        put_json("/api/admin/oidc", &keep.to_string(), Some(&admin)),
    )
    .await;
    let view: Value = serde_json::from_str(&body_string(res.into_body()).await).unwrap();
    assert_eq!(view["has_secret"], true);
    let mut remove = keep.clone();
    remove["remove_secret"] = json!(true);
    let res = request(
        &app,
        put_json("/api/admin/oidc", &remove.to_string(), Some(&admin)),
    )
    .await;
    let view: Value = serde_json::from_str(&body_string(res.into_body()).await).unwrap();
    assert_eq!(view["has_secret"], false);
    assert_eq!(state.oidc.config().unwrap().client_secret, None);

    // Turning it off applies at once too.
    let res = request(
        &app,
        put_json("/api/admin/oidc", r#"{"enabled":false}"#, Some(&admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(status().await["sso"], Value::Null);
    assert_eq!(location(&start(&app).await), error("unavailable"));
}

#[tokio::test]
async fn a_new_issuer_does_not_inherit_the_secret() {
    let provider = Provider::start().await;
    let other = Provider::start().await;
    let (app, state) = setup_with(Oidc::new(EnvSettings::default()), false).await;
    let admin = bootstrap_admin(&app).await;
    let save = |body: Value| {
        let (app, admin) = (app.clone(), admin.clone());
        async move {
            request(
                &app,
                put_json("/api/admin/oidc", &body.to_string(), Some(&admin)),
            )
            .await
            .status()
        }
    };
    assert_eq!(
        save(json!({"enabled": true, "issuer": provider.issuer, "client_id": CLIENT_ID, "client_secret": CLIENT_SECRET})).await,
        StatusCode::OK
    );
    assert_eq!(
        save(json!({"enabled": true, "issuer": other.issuer, "client_id": CLIENT_ID})).await,
        StatusCode::OK
    );
    assert_eq!(state.oidc.config().unwrap().client_secret, None);
    let row = entity::oidc_settings::Entity::find_by_id(1)
        .one(state.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.client_secret, None);
}

#[tokio::test]
async fn fields_from_the_environment_are_locked() {
    let provider = Provider::start().await;
    let (app, state) = setup(&provider).await; // issuer, client id and secret from the environment
    let admin = bootstrap_admin(&app).await;
    let res = request(&app, get("/api/admin/oidc", Some(&admin))).await;
    let view: Value = serde_json::from_str(&body_string(res.into_body()).await).unwrap();
    assert_eq!(view["locked"]["issuer"], true);
    assert_eq!(view["locked"]["client_secret"], true);
    assert_eq!(view["locked"]["display_name"], false);
    assert_eq!(view["issuer"], provider.issuer);
    assert_eq!(view["active"], true);

    for body in [
        json!({"issuer": "https://elsewhere.example.com"}),
        json!({"client_id": "another"}),
        json!({"client_secret": "another"}),
        json!({"remove_secret": true}),
    ] {
        let res = request(
            &app,
            put_json("/api/admin/oidc", &body.to_string(), Some(&admin)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    let res = request(
        &app,
        put_json(
            "/api/admin/oidc",
            r#"{"display_name":"Company"}"#,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(state.oidc.config().unwrap().display_name, "Company");
    assert_eq!(state.oidc.config().unwrap().issuer, provider.issuer);
}

/// With the secret from the environment, an admin may not point it at another issuer (one of theirs).
#[tokio::test]
async fn the_environments_secret_cannot_be_sent_to_another_issuer() {
    let provider = Provider::start().await;
    let (app, _) = setup_with(
        Oidc::new(EnvSettings {
            client_secret: Some(CLIENT_SECRET.to_string()),
            ..Default::default()
        }),
        false,
    )
    .await;
    let admin = bootstrap_admin(&app).await;
    let res = request(
        &app,
        put_json(
            "/api/admin/oidc",
            &json!({"enabled": true, "issuer": provider.issuer, "client_id": CLIENT_ID})
                .to_string(),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(provider.with(|s| s.discovery_hits), 0);
}

/// Saves at the same time end with the database and the settings in force agreeing.
#[tokio::test]
async fn concurrent_saves_leave_memory_and_database_in_step() {
    let provider = Provider::start().await;
    let (app, state) = setup_with(Oidc::new(EnvSettings::default()), false).await;
    let admin = bootstrap_admin(&app).await;
    let saves: Vec<_> = (0..6)
        .map(|i| {
            let (app, admin) = (app.clone(), admin.clone());
            let body = json!({
                "enabled": true,
                "issuer": provider.issuer,
                "client_id": CLIENT_ID,
                "display_name": format!("Provider {i}"),
            });
            tokio::spawn(async move {
                request(
                    &app,
                    put_json("/api/admin/oidc", &body.to_string(), Some(&admin)),
                )
                .await
                .status()
            })
        })
        .collect();
    for save in saves {
        assert_eq!(save.await.unwrap(), StatusCode::OK);
    }
    let row = entity::oidc_settings::Entity::find_by_id(1)
        .one(state.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        Some(state.oidc.config().unwrap().display_name.clone()),
        row.display_name
    );
}

#[tokio::test]
async fn yes_no_variables_are_strict() {
    for (value, expected) in [
        ("1", Some(true)),
        ("on", Some(true)),
        ("YES", Some(true)),
        ("false", Some(false)),
        ("off", Some(false)),
        ("", None),
    ] {
        assert_eq!(api::config::parse_flag("X", value), Ok(expected), "{value}");
    }
    for value in ["2", "enabled", "y", "nope"] {
        assert!(api::config::parse_flag("X", value).is_err(), "{value}");
    }
}

#[tokio::test]
async fn a_login_started_before_the_settings_changed_fails() {
    let provider = Provider::start().await;
    let (app, _) = setup_with(Oidc::new(EnvSettings::default()), false).await;
    let admin = bootstrap_admin(&app).await;
    let save = |client_id: &str| {
        let body = json!({"enabled": true, "issuer": provider.issuer, "client_id": client_id, "client_secret": CLIENT_SECRET});
        let (app, admin) = (app.clone(), admin.clone());
        async move {
            request(
                &app,
                put_json("/api/admin/oidc", &body.to_string(), Some(&admin)),
            )
            .await
            .status()
        }
    };
    assert_eq!(save(CLIENT_ID).await, StatusCode::OK);
    let login = login(&app).await;
    assert_eq!(save("another-client").await, StatusCode::OK);
    let code = provider.authorize(
        &login,
        provider.claims(&login, "sub-1", Some("admin@home.io")),
    );
    assert_eq!(finish(&app, &login, &code).await, (error("state"), None));
}

#[tokio::test]
async fn a_host_header_cannot_shape_the_callback_url() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    for host in ["evil.example/path?x=", "user@evil.example", "evil.example#"] {
        let res = request(
            &app,
            Request::builder()
                .uri("/api/auth/oidc/login")
                .header("host", host)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(location(&res), error("misconfigured"), "{host}");
    }
}

/// A save reads the stored row (for the secret it keeps) only after the save before it is in force: a
/// save that keeps the secret must not undo one that set it a moment earlier.
#[tokio::test]
async fn a_save_never_works_from_a_stale_row() {
    let provider = Provider::start().await;
    let (app, state) = setup_with(Oidc::new(EnvSettings::default()), false).await;
    let admin = bootstrap_admin(&app).await;
    let save = |body: Value| {
        let (app, admin) = (app.clone(), admin.clone());
        tokio::spawn(async move {
            request(
                &app,
                put_json("/api/admin/oidc", &body.to_string(), Some(&admin)),
            )
            .await
            .status()
        })
    };
    let base = json!({"enabled": true, "issuer": provider.issuer, "client_id": CLIENT_ID});
    assert_eq!(save(base.clone()).await.unwrap(), StatusCode::OK);

    // Both saves get as far as they can while the write turn is held here.
    let turn = state.writer().await.unwrap();
    let mut with_secret = base.clone();
    with_secret["client_secret"] = json!(CLIENT_SECRET);
    let first = save(with_secret);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let second = save(base);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    drop(turn);
    assert_eq!(first.await.unwrap(), StatusCode::OK);
    assert_eq!(second.await.unwrap(), StatusCode::OK);
    assert_eq!(
        state.oidc.config().unwrap().client_secret.as_deref(),
        Some(CLIENT_SECRET)
    );
}

/// A save with a field locked by the environment keeps what is stored under it: a row saved before
/// `OIDC_ISSUER` was set keeps its issuer and secret, though the form cannot send the issuer.
#[tokio::test]
async fn a_locked_issuer_does_not_clear_the_stored_secret() {
    let provider = Provider::start().await;
    let (app, state) = setup_with(
        Oidc::new(EnvSettings {
            issuer: Some(provider.issuer.clone()),
            ..Default::default()
        }),
        false,
    )
    .await;
    let admin = bootstrap_admin(&app).await;
    let row = entity::OidcSettings {
        id: 1,
        enabled: true,
        issuer: Some(provider.issuer.clone()),
        client_id: Some(CLIENT_ID.to_string()),
        client_secret: Some(CLIENT_SECRET.to_string()),
        display_name: None,
        redirect_url: None,
        auto_create: false,
    };
    entity::oidc_settings::ActiveModel::from(row.clone())
        .insert(state.db())
        .await
        .unwrap();
    state.oidc.apply(Some(&row));
    let res = request(
        &app,
        put_json(
            "/api/admin/oidc",
            &json!({"client_id": CLIENT_ID, "display_name": "Company"}).to_string(),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "{}",
        body_string(res.into_body()).await
    );
    assert_eq!(
        state.oidc.config().unwrap().client_secret.as_deref(),
        Some(CLIENT_SECRET)
    );
}

#[tokio::test]
async fn a_query_that_does_not_parse_still_redirects_and_clears() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    let res = callback(&app, "code=a&code=b&state=x", None).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), error("state"));
    assert!(
        set_cookies(&res)
            .iter()
            .any(|c| c.starts_with("woodmouse_oidc=;"))
    );
}

/// Only you see whether you have a password; coworkers listing members do not.
#[tokio::test]
async fn only_you_see_whether_you_have_a_password() {
    let provider = Provider::start().await;
    let (app, _) = setup(&provider).await;
    let admin = bootstrap_admin(&app).await;
    assert_eq!(me(&app, &admin).await["has_password"], true);
    let workspace = create_workspace(&app, &admin, "Home").await;
    let res = request(
        &app,
        get(
            &format!("/api/workspaces/{workspace}/members"),
            Some(&admin),
        ),
    )
    .await;
    let body = body_string(res.into_body()).await;
    assert!(body.contains("admin@home.io"), "{body}");
    assert!(!body.contains("has_password"), "{body}");
}

/// Refused sign-ins are in the audit log like failed password logins, with the reason and the account
/// when one was found; requests to the callback that no login of ours started are not.
#[tokio::test]
async fn refused_sign_ins_are_audited() {
    let provider = Provider::start().await;
    let (app, state) = setup(&provider).await;
    bootstrap_admin(&app).await;
    let off = add_user(&state, "off@home.io", true, false).await;
    let failures = || async {
        entity::audit_logs::Entity::find()
            .filter(entity::audit_logs::Column::Action.eq("login_failed"))
            .order_by_asc(entity::audit_logs::Column::Id)
            .all(state.db())
            .await
            .unwrap()
            .into_iter()
            .map(|e| {
                (
                    e.data["method"].as_str().unwrap_or("").to_string(),
                    e.data["reason"].as_str().unwrap_or("").to_string(),
                    e.target_id,
                )
            })
            .collect::<Vec<_>>()
    };

    // Not ours: no cookie, a forged one, a query that does not parse.
    callback(&app, "code=x&state=y", None).await;
    callback(&app, "error=access_denied", Some("woodmouse_oidc=forged")).await;
    callback(&app, "code=a&code=b", None).await;
    assert_eq!(failures().await, vec![]);

    assert_eq!(
        sign_in(&app, &provider, "sub-9", Some("nobody@home.io"))
            .await
            .0,
        error("unknown")
    );
    assert_eq!(
        sign_in(&app, &provider, "sub-3", Some("off@home.io"))
            .await
            .0,
        error("disabled")
    );
    let login = login(&app).await;
    let res = callback(
        &app,
        &format!("error=access_denied&state={}", login.state),
        Some(&login.cookie),
    )
    .await;
    assert_eq!(location(&res), error("denied"));

    let sso =
        |reason: &str, target: Option<uuid::Uuid>| ("sso".to_string(), reason.to_string(), target);
    assert_eq!(
        failures().await,
        vec![
            sso("unknown", None),
            sso("disabled", Some(off)),
            sso("denied", None)
        ]
    );
}
