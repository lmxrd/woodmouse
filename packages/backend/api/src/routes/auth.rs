use crate::db::prelude::*;
use entity::users;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::audit::{self, Entry};
use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::sessions::{self, AuthSession};
use crate::state::AppState;
use crate::totp;
use entity::{Role, User};

/// Routes reachable without a session.
pub fn public_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(auth_status))
        .routes(routes!(bootstrap))
        .routes(routes!(login))
        .routes(routes!(login_totp))
        .routes(routes!(login_totp_setup))
        .routes(routes!(login_totp_enable))
}

/// Routes requiring a session (the auth layer in `routes::mod` enforces it;
/// the annotations document it).
pub fn protected_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(logout))
        .routes(routes!(me, update_me))
        .routes(routes!(change_password))
        .routes(routes!(totp_setup))
        .routes(routes!(totp_enable))
        .routes(routes!(totp_disable))
        .routes(routes!(totp_recovery_codes))
        .routes(routes!(totp_replace))
        .routes(routes!(totp_replace_confirm))
}

pub const DATE_FORMATS: [&str; 4] = ["YYYY-MM-DD", "DD.MM.YYYY", "MM/DD/YYYY", "D MMM YYYY"];
pub const TIME_FORMATS: [&str; 2] = ["24h", "12h"];
pub const WEEK_STARTS: [&str; 2] = ["monday", "sunday"];

/// User as exposed over the API — never includes the password hash.
#[derive(Clone, Serialize, ToSchema)]
pub struct PublicUser {
    pub id: uuid::Uuid,
    pub email: String,
    /// Display name; may be empty.
    pub name: String,
    pub super_admin: bool,
    /// One of `YYYY-MM-DD`, `DD.MM.YYYY`, `MM/DD/YYYY`, `D MMM YYYY`.
    pub date_format: String,
    /// `24h` or `12h`.
    pub time_format: String,
    /// `monday` or `sunday`.
    pub week_start: String,
    /// `light`, `dark` or `system` (follow the device).
    pub theme_mode: String,
    /// IANA timezone (`Europe/Oslo`), once known.
    pub timezone: Option<String>,
    /// Access to every workspace at this role, given by the super admin.
    pub global_role: Option<Role>,
    /// False for a disabled account (it cannot sign in).
    pub active: bool,
}

/// The signed-in user, with what only they may know about their own account.
#[derive(Clone, Serialize, ToSchema)]
pub struct Me {
    #[serde(flatten)]
    pub user: PublicUser,
    /// Whether they have a password (someone signing in only through single sign-on may have none; then
    /// setting one needs no current password). Not in `PublicUser`, so coworkers cannot see who has one.
    pub has_password: bool,
    /// Whether signing in with the password also asks for a code from an authenticator app.
    pub totp_enabled: bool,
}

impl Me {
    pub async fn load(db: &impl crate::db::Db, user: &User) -> Result<Self, ApiError> {
        Ok(Self {
            user: PublicUser::from(user),
            has_password: user.password_hash.is_some(),
            totp_enabled: totp::enabled(db, user.id).await?.is_some(),
        })
    }
}

impl From<&User> for PublicUser {
    fn from(user: &User) -> Self {
        Self {
            id: user.id,
            email: user.email.clone(),
            name: user.name.clone(),
            super_admin: user.super_admin,
            date_format: user
                .date_format
                .clone()
                .unwrap_or_else(|| DATE_FORMATS[0].to_string()),
            time_format: user
                .time_format
                .clone()
                .unwrap_or_else(|| TIME_FORMATS[0].to_string()),
            week_start: user
                .week_start
                .clone()
                .unwrap_or_else(|| WEEK_STARTS[0].to_string()),
            theme_mode: user
                .theme_mode
                .clone()
                .unwrap_or_else(|| crate::routes::themes::MODES[2].to_string()),
            timezone: user.timezone.clone(),
            global_role: user.global_role,
            active: user.active,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct AuthStatus {
    /// True while the users table is empty: the first user must register.
    pub bootstrap_required: bool,
    /// The single sign-on provider's name for the sign-in button; `null` while single sign-on is off.
    pub sso: Option<String>,
    /// False when single sign-on is the only way in (`OIDC_ONLY`): no password form, invite or change.
    pub password_login: bool,
    /// Every sign-in asks for two-factor (`TOTP_REQUIRED`), and someone without it sets it up first.
    pub totp_required: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct BootstrapRequest {
    pub email: String,
    /// Optional display name.
    #[serde(default)]
    pub name: String,
    pub password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// The display name is optional; only the email must be valid.
pub(crate) fn validate_identity(email: &str) -> Result<(), ApiError> {
    if email.trim().is_empty() || !email.contains('@') {
        return Err(ApiError::bad_request("email is invalid"));
    }
    Ok(())
}

pub(crate) fn validate_password(password: &str) -> Result<(), ApiError> {
    if password.len() < 8 {
        return Err(ApiError::bad_request(
            "password must be at least 8 characters",
        ));
    }
    Ok(())
}

fn validate_credentials(email: &str, password: &str) -> Result<(), ApiError> {
    validate_identity(email)?;
    validate_password(password)
}

/// Whether first-user bootstrap is open (users table empty).
#[utoipa::path(
    get,
    path = "/auth/status",
    tag = "auth",
    responses((status = 200, description = "Auth status", body = AuthStatus))
)]
pub async fn auth_status(State(state): State<AppState>) -> Result<Json<AuthStatus>, ApiError> {
    let db = state.db();
    let users = users::Entity::find().all(db).await?;
    Ok(Json(AuthStatus {
        bootstrap_required: users.is_empty(),
        sso: state.oidc.config().map(|c| c.display_name.clone()),
        password_login: state.oidc.password_login(),
        totp_required: state.totp_required,
    }))
}

/// Create the first user (super admin) and log in. Open only while the
/// users table is empty; afterwards admins create users (invite flow, later
/// phase).
#[utoipa::path(
    post,
    path = "/auth/bootstrap",
    tag = "auth",
    request_body = BootstrapRequest,
    responses(
        (status = 201, description = "Super admin created and logged in", body = PublicUser),
        (status = 409, description = "A user already exists"),
        (status = 400, description = "Invalid input")
    )
)]
pub async fn bootstrap(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    Json(input): Json<BootstrapRequest>,
) -> Result<(StatusCode, CookieJar, Json<PublicUser>), ApiError> {
    validate_credentials(&input.email, &input.password)?;

    let db = state.writer().await?;
    if !users::Entity::find().all(&*db).await?.is_empty() {
        return Err(ApiError::conflict(
            "bootstrap is closed: a user already exists",
        ));
    }

    let tx = db.begin().await?;
    let user = users::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        email: Set(input.email.trim().to_lowercase()),
        name: Set(input.name.trim().to_string()),
        password_hash: Set(Some(auth::hash_password(&input.password)?)),
        super_admin: Set(true),
        active: Set(true),
        ..Default::default()
    }
    .insert(&tx)
    .await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "user_created").target("user", user.id),
    )
    .await?;
    tx.commit().await?;
    drop(db);
    let jar = sessions::sign_in(&mut auth_session, &state, &user).await?;
    Ok((StatusCode::CREATED, jar, Json(PublicUser::from(&user))))
}

/// The password was right, and a second step is needed before the session (one of the two is true).
#[derive(Serialize, ToSchema)]
pub struct TotpChallenge {
    /// A code from the authenticator app (or a recovery code): send it to `POST /auth/login/totp` within
    /// five minutes.
    pub totp_required: bool,
    /// The site requires two-factor and they have none: set it up with `POST /auth/login/totp/setup` and
    /// `/enable` within fifteen minutes.
    pub totp_setup_required: bool,
}

impl TotpChallenge {
    fn new(mode: totp::ChallengeMode) -> Self {
        Self {
            totp_required: mode == totp::ChallengeMode::Code,
            totp_setup_required: mode == totp::ChallengeMode::Enroll,
        }
    }
}

/// Log in with email + password; sets the session cookie. Someone with two-factor on gets a 202 and a
/// short-lived challenge cookie instead, and the session from `POST /auth/login/totp`.
#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Logged in", body = PublicUser),
        (status = 202, description = "The password is right; a two-factor code, or setting it up, is needed", body = TotpChallenge),
        (status = 401, description = "Invalid credentials"),
        (status = 403, description = "Password sign-in is turned off (`OIDC_ONLY`)")
    )
)]
pub async fn login(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    ClientAddr(address): ClientAddr,
    Json(input): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    state.oidc.require_password_login()?;
    let email = input.email.trim().to_lowercase();
    // Repeated failures wait longer (never a lockout).
    let delay = state.login_limits.delay(&email, address.as_deref());
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let db = state.writer().await?;
    let user = users::Entity::find()
        .filter(users::Column::Email.eq(&email))
        .one_or_not_found(&*db)
        .await
        .ok();
    // Generic error: never reveal whether the email exists, has a password, or the account is disabled.
    // An unknown email or a missing password is checked against a dummy hash, so it takes as long.
    let matches = auth::verify_user_password(
        &input.password,
        user.as_ref().and_then(|u| u.password_hash.as_deref()),
    );
    let ok = matches && user.as_ref().is_some_and(|u| u.active);
    let Some(user) = user.filter(|_| ok) else {
        state.login_limits.failed(&email, address.as_deref());
        let _ = audit::record(&*db, Entry::admin(None, "login_failed")).await;
        return Err(ApiError::unauthorized());
    };

    if let Some(mode) = totp::second_step(&*db, state.totp_required, user.id).await? {
        // Not `succeeded` yet: the failures for this email keep counting until the code is right too, or
        // the password alone would buy fresh guesses at the code.
        let jar = totp::challenge_jar(&state.challenge_key, state.secure_cookies, user.id, mode)?;
        return Ok((StatusCode::ACCEPTED, jar, Json(TotpChallenge::new(mode))).into_response());
    }
    state.login_limits.succeeded(&email);

    let tx = db.begin().await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "login").target("user", user.id),
    )
    .await?;
    tx.commit().await?;
    drop(db);
    let jar = sessions::sign_in(&mut auth_session, &state, &user).await?;
    Ok((StatusCode::OK, jar, Json(PublicUser::from(&user))).into_response())
}

#[derive(Deserialize, ToSchema)]
pub struct TotpCode {
    /// Six digits from the authenticator app, or one of the recovery codes.
    pub code: String,
}

/// The second step of a sign-in with two-factor on: the code, with the challenge cookie from
/// `POST /auth/login` or the single sign-on callback. Sets the session cookie.
#[utoipa::path(
    post,
    path = "/auth/login/totp",
    tag = "auth",
    request_body = TotpCode,
    responses(
        (status = 200, description = "Logged in", body = PublicUser),
        (status = 401, description = "Wrong code, or no current challenge (start again from the password)")
    )
)]
pub async fn login_totp(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    ClientAddr(address): ClientAddr,
    headers: axum::http::HeaderMap,
    Json(input): Json<TotpCode>,
) -> Result<(StatusCode, CookieJar, Json<PublicUser>), ApiError> {
    // No `OIDC_ONLY` check: a challenge exists only after a first factor, single sign-on's included.
    let challenge = totp::read_challenge(
        &headers,
        &state.challenge_key,
        state.secure_cookies,
        totp::ChallengeMode::Code,
    )
    .ok_or_else(ApiError::unauthorized)?;
    let user = users::Entity::find_by_id(challenge.user_id)
        .one_or_not_found(state.db())
        .await
        .map_err(|_| ApiError::unauthorized())?;
    let delay = state.login_limits.delay(&user.email, address.as_deref());
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }

    let db = state.writer().await?;
    let tx = db.begin().await?;
    let row = totp::enabled(&tx, user.id).await?;
    let proof = match (&row, user.active) {
        (Some(row), true) => totp::verify(&tx, row, &input.code).await?,
        _ => None,
    };
    let Some(proof) = proof else {
        drop(tx);
        state.login_limits.failed(&user.email, address.as_deref());
        let _ = audit::record(
            &*db,
            Entry::admin(None, "login_failed")
                .target("user", user.id)
                .data(serde_json::json!({ "method": "password", "reason": "totp" })),
        )
        .await;
        return Err(ApiError::unauthorized());
    };
    state.login_limits.succeeded(&user.email);
    if proof == totp::Proof::RecoveryCode {
        audit::record(
            &tx,
            Entry::admin(Some(user.id), "recovery_code_used").target("user", user.id),
        )
        .await?;
    }
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "login")
            .target("user", user.id)
            .data(serde_json::json!({ "totp": true })),
    )
    .await?;
    tx.commit().await?;
    drop(db);
    let session = sessions::sign_in(&mut auth_session, &state, &user).await?;
    Ok((
        StatusCode::OK,
        session.add(totp::challenge_cookie_removal(state.secure_cookies)),
        Json(PublicUser::from(&user)),
    ))
}

/// Log out: delete the session and clear the cookie.
#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    security(("session_cookie" = [])),
    responses((status = 204, description = "Logged out"))
)]
pub async fn logout(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<(StatusCode, CookieJar), ApiError> {
    let db = state.writer().await?;
    let tx = db.begin().await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "logout").target("user", user.id),
    )
    .await?;
    tx.commit().await?;
    drop(db);
    // Deletes the session and clears its cookie.
    sessions::sign_out(&mut auth_session).await?;
    Ok((StatusCode::NO_CONTENT, CookieJar::new()))
}

/// The current user.
#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "The current user", body = Me),
        (status = 401, description = "Not logged in")
    )
)]
pub async fn me(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<Me>, ApiError> {
    Ok(Json(Me::load(state.db(), &user).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateMe {
    /// Display name; an empty string clears it.
    pub name: Option<String>,
    pub email: Option<String>,
    /// One of `YYYY-MM-DD`, `DD.MM.YYYY`, `MM/DD/YYYY`, `D MMM YYYY`.
    pub date_format: Option<String>,
    /// `24h` or `12h`.
    pub time_format: Option<String>,
    /// `monday` or `sunday`.
    pub week_start: Option<String>,
    /// `light`, `dark` or `system`.
    pub theme_mode: Option<String>,
    /// An IANA timezone name such as `Europe/Oslo`.
    pub timezone: Option<String>,
}

/// Edit your own profile and display preferences.
#[utoipa::path(
    patch,
    path = "/auth/me",
    tag = "auth",
    security(("session_cookie" = [])),
    request_body = UpdateMe,
    responses(
        (status = 200, description = "The updated user", body = Me),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 409, description = "That email is already taken")
    )
)]
pub async fn update_me(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<UpdateMe>,
) -> Result<Json<Me>, ApiError> {
    if let Some(format) = &input.date_format
        && !DATE_FORMATS.contains(&format.as_str())
    {
        return Err(ApiError::bad_request("unknown date format"));
    }
    if let Some(format) = &input.time_format
        && !TIME_FORMATS.contains(&format.as_str())
    {
        return Err(ApiError::bad_request("unknown time format"));
    }

    if let Some(week_start) = &input.week_start
        && !WEEK_STARTS.contains(&week_start.as_str())
    {
        return Err(ApiError::bad_request("unknown week start"));
    }

    if let Some(mode) = &input.theme_mode
        && !crate::routes::themes::MODES.contains(&mode.as_str())
    {
        return Err(ApiError::bad_request("unknown theme mode"));
    }

    if let Some(zone) = &input.timezone
        && jiff::tz::TimeZone::get(zone).is_err()
    {
        return Err(ApiError::bad_request("unknown timezone"));
    }

    let writer = state.writer().await?;
    let db = &*writer;
    let email = match &input.email {
        Some(email) => {
            validate_identity(email)?;
            let email = email.trim().to_lowercase();
            if email != user.email
                && let Ok(other) = users::Entity::find()
                    .filter(users::Column::Email.eq(&email))
                    .one_or_not_found(db)
                    .await
                && other.id != user.id
            {
                return Err(ApiError::conflict("that email is already taken"));
            }
            Some(email)
        }
        None => None,
    };

    let current = users::Entity::find_by_id(user.id)
        .one_or_not_found(db)
        .await?;
    let mut update = users::ActiveModel::from(current);
    if let Some(name) = &input.name {
        update.name = Set(name.trim().to_string());
    }
    if let Some(email) = email {
        update.email = Set(email);
    }
    if let Some(format) = input.date_format {
        update.date_format = Set(Some(format));
    }
    if let Some(format) = input.time_format {
        update.time_format = Set(Some(format));
    }
    if let Some(week_start) = input.week_start {
        update.week_start = Set(Some(week_start));
    }
    if let Some(mode) = input.theme_mode {
        update.theme_mode = Set(Some(mode));
    }
    if let Some(zone) = input.timezone {
        update.timezone = Set(Some(zone));
    }
    let current = update.update(db).await?;
    Ok(Json(Me::load(db, &current).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct ChangePassword {
    /// Required while you have a password; leave it out to set a first one (an account that signs in
    /// only through single sign-on).
    #[serde(default)]
    pub current_password: Option<String>,
    pub new_password: String,
}

/// Change your password, or set a first one. Other sessions of yours are signed out.
#[utoipa::path(
    post,
    path = "/auth/password",
    tag = "auth",
    security(("session_cookie" = [])),
    request_body = ChangePassword,
    responses(
        (status = 204, description = "Password changed"),
        (status = 400, description = "The new password is too short"),
        (status = 401, description = "Not logged in, or the current password is wrong"),
        (status = 403, description = "Password sign-in is turned off (`OIDC_ONLY`)"),
        (status = 409, description = "The password changed meanwhile; try again")
    )
)]
pub async fn change_password(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<ChangePassword>,
) -> Result<StatusCode, ApiError> {
    state.oidc.require_password_login()?;
    if let Some(current) = &user.password_hash
        && !auth::verify_password(input.current_password.as_deref().unwrap_or(""), current)
    {
        return Err(ApiError::unauthorized());
    }
    validate_password(&input.new_password)?;
    let new_hash = auth::hash_password(&input.new_password)?;

    let db = state.writer().await?;
    let tx = db.begin().await?;
    // Only if the hash is still the one just checked: two requests setting a first password (which needs
    // no current one) must not both succeed, and neither may replace a password set meanwhile.
    let checked = match &user.password_hash {
        Some(hash) => users::Column::PasswordHash.eq(hash.as_str()),
        None => users::Column::PasswordHash.is_null(),
    };
    let updated = users::Entity::update_many()
        .col_expr(users::Column::PasswordHash, Expr::value(new_hash))
        .filter(users::Column::Id.eq(user.id))
        .filter(checked)
        .exec(&tx)
        .await?
        .rows_affected;
    if updated != 1 {
        return Err(ApiError::conflict("the password changed meanwhile"));
    }
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "password_changed").target("user", user.id),
    )
    .await?;
    tx.commit().await?;
    drop(db);
    // The new password hash no longer matches the other sessions' (they end on their next request);
    // this one signs in again with it.
    sessions::refresh(&mut auth_session, &state, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The client's network address, if known: the connection's, else the first `X-Forwarded-For` entry.
/// Used only to slow down repeated failed logins; it is never stored.
pub struct ClientAddr(pub Option<String>);

impl axum::extract::FromRequestParts<AppState> for ClientAddr {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let from_connection = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|c| c.0.ip().to_string());
        let forwarded = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(|v| v.trim().to_string());
        Ok(ClientAddr(forwarded.or(from_connection)))
    }
}

/// A new authenticator secret, to show as a QR code (`otpauth_uri`) and as text (`secret`).
#[derive(Serialize, ToSchema)]
pub struct TotpSetup {
    /// Base32, for typing into the app.
    pub secret: String,
    /// `otpauth://totp/...`, for the QR code.
    pub otpauth_uri: String,
}

/// Recovery codes, shown this once: each signs in once in place of a code from the app.
#[derive(Serialize, ToSchema)]
pub struct RecoveryCodes {
    pub codes: Vec<String>,
}

/// Start setting up two-factor sign-in: a new secret for the authenticator app. Nothing changes until it
/// is confirmed with a code (`POST /auth/totp/enable`); starting again replaces an unconfirmed secret.
#[utoipa::path(
    post,
    path = "/auth/totp/setup",
    tag = "auth",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "The new secret", body = TotpSetup),
        (status = 401, description = "Not logged in"),
        (status = 409, description = "Two-factor is already on (turn it off first)")
    )
)]
pub async fn totp_setup(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<TotpSetup>, ApiError> {
    Ok(Json(start_setup(&state, &user).await?))
}

/// A new, unconfirmed secret for `user` (replacing an unconfirmed one).
async fn start_setup(state: &AppState, user: &User) -> Result<TotpSetup, ApiError> {
    let db = state.writer().await?;
    let tx = db.begin().await?;
    if totp::enabled(&tx, user.id).await?.is_some() {
        return Err(ApiError::conflict("two-factor sign-in is already on"));
    }
    let secret = totp::new_secret();
    // Before anything is stored: an email an authenticator app cannot take is refused here.
    let otpauth_uri = totp::otpauth_uri(&secret, &user.email)?;
    entity::user_totp::Entity::delete_many()
        .filter(entity::user_totp::Column::UserId.eq(user.id))
        .exec(&tx)
        .await?;
    entity::user_totp::ActiveModel {
        user_id: Set(user.id),
        secret: Set(secret.clone()),
        enabled_at: Set(None),
        last_step: Set(None),
        created_at: Set(crate::time::now()),
        pending_secret: Set(None),
    }
    .insert(&tx)
    .await?;
    tx.commit().await?;
    Ok(TotpSetup {
        secret,
        otpauth_uri,
    })
}

/// Wait out the slow-down for repeated wrong codes, as at sign-in.
async fn slow_down(state: &AppState, user: &User, address: Option<&str>) {
    let delay = state.login_limits.delay(&user.email, address);
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

/// Turn two-factor on with the first code from the app. Answers the recovery codes, which are not shown
/// again.
#[utoipa::path(
    post,
    path = "/auth/totp/enable",
    tag = "auth",
    security(("session_cookie" = [])),
    request_body = TotpCode,
    responses(
        (status = 200, description = "Two-factor is on", body = RecoveryCodes),
        (status = 400, description = "Wrong code, or no setup started"),
        (status = 401, description = "Not logged in"),
        (status = 409, description = "Two-factor is already on")
    )
)]
pub async fn totp_enable(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    ClientAddr(address): ClientAddr,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<TotpCode>,
) -> Result<Json<RecoveryCodes>, ApiError> {
    slow_down(&state, &user, address.as_deref()).await;
    let db = state.writer().await?;
    let tx = db.begin().await?;
    let codes = finish_setup(&state, &tx, &user, address.as_deref(), &input.code).await?;
    tx.commit().await?;
    drop(db);
    // The two-factor secret is part of the session hash: the other sessions end, this one stays.
    sessions::refresh(&mut auth_session, &state, user.id).await?;
    Ok(Json(RecoveryCodes { codes }))
}

/// Turn two-factor on with the first code for the secret from [`start_setup`]: the recovery codes.
async fn finish_setup(
    state: &AppState,
    tx: &sea_orm::DatabaseTransaction,
    user: &User,
    address: Option<&str>,
    code: &str,
) -> Result<Vec<String>, ApiError> {
    let row = entity::user_totp::Entity::find_by_id(user.id)
        .one(tx)
        .await?
        .ok_or_else(|| ApiError::bad_request("start the setup first"))?;
    if row.enabled_at.is_some() {
        return Err(ApiError::conflict("two-factor sign-in is already on"));
    }
    if !totp::accept_app_code(tx, &row, code.trim()).await? {
        state.login_limits.failed(&user.email, address);
        return Err(ApiError::bad_request("that code is not right"));
    }
    entity::user_totp::ActiveModel {
        user_id: Unchanged(user.id),
        enabled_at: Set(Some(crate::time::now())),
        ..Default::default()
    }
    .update(tx)
    .await?;
    let codes = totp::new_recovery_codes(tx, user.id).await?;
    audit::record(
        tx,
        Entry::admin(Some(user.id), "totp_enabled").target("user", user.id),
    )
    .await?;
    Ok(codes)
}

/// Check a code of someone with two-factor on, for turning it off or making new recovery codes.
async fn require_code(
    state: &AppState,
    tx: &sea_orm::DatabaseTransaction,
    user: &User,
    address: Option<&str>,
    code: &str,
) -> Result<totp::Proof, ApiError> {
    let row = totp::enabled(tx, user.id)
        .await?
        .ok_or_else(|| ApiError::bad_request("two-factor sign-in is off"))?;
    match totp::verify(tx, &row, code).await? {
        Some(proof) => Ok(proof),
        None => {
            state.login_limits.failed(&user.email, address);
            Err(ApiError::bad_request("that code is not right"))
        }
    }
}

/// Turn two-factor off, with a code from the app or a recovery code.
#[utoipa::path(
    post,
    path = "/auth/totp/disable",
    tag = "auth",
    security(("session_cookie" = [])),
    request_body = TotpCode,
    responses(
        (status = 204, description = "Two-factor is off"),
        (status = 400, description = "Wrong code, or two-factor is off already"),
        (status = 401, description = "Not logged in"),
        (status = 403, description = "The site requires two-factor (`TOTP_REQUIRED`)")
    )
)]
pub async fn totp_disable(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    ClientAddr(address): ClientAddr,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<TotpCode>,
) -> Result<StatusCode, ApiError> {
    if state.totp_required {
        return Err(ApiError::forbidden(
            "two-factor sign-in is required on this site",
        ));
    }
    slow_down(&state, &user, address.as_deref()).await;
    let db = state.writer().await?;
    let tx = db.begin().await?;
    require_code(&state, &tx, &user, address.as_deref(), &input.code).await?;
    totp::remove(&tx, user.id).await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "totp_disabled").target("user", user.id),
    )
    .await?;
    tx.commit().await?;
    drop(db);
    // The two-factor secret is part of the session hash: the other sessions end, this one stays.
    sessions::refresh(&mut auth_session, &state, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Replace the recovery codes with new ones (the old ones stop working), with a code from the app or a
/// recovery code.
#[utoipa::path(
    post,
    path = "/auth/totp/recovery-codes",
    tag = "auth",
    security(("session_cookie" = [])),
    request_body = TotpCode,
    responses(
        (status = 200, description = "The new recovery codes", body = RecoveryCodes),
        (status = 400, description = "Wrong code, or two-factor is off"),
        (status = 401, description = "Not logged in")
    )
)]
pub async fn totp_recovery_codes(
    State(state): State<AppState>,
    ClientAddr(address): ClientAddr,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<TotpCode>,
) -> Result<Json<RecoveryCodes>, ApiError> {
    slow_down(&state, &user, address.as_deref()).await;
    let db = state.writer().await?;
    let tx = db.begin().await?;
    require_code(&state, &tx, &user, address.as_deref(), &input.code).await?;
    let codes = totp::new_recovery_codes(&tx, user.id).await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "recovery_codes_renewed").target("user", user.id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(RecoveryCodes { codes }))
}

/// The challenge of a sign-in that must set up two-factor first (`TOTP_REQUIRED`), and its user.
async fn enroll_challenge(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<User, ApiError> {
    let challenge = totp::read_challenge(
        headers,
        &state.challenge_key,
        state.secure_cookies,
        totp::ChallengeMode::Enroll,
    )
    .ok_or_else(ApiError::unauthorized)?;
    users::Entity::find_by_id(challenge.user_id)
        .one(state.db())
        .await?
        .filter(|u| u.active)
        .ok_or_else(ApiError::unauthorized)
}

/// Signing in where two-factor is required (`TOTP_REQUIRED`) and they have none: a new secret, with the
/// challenge cookie from `POST /auth/login` or the single sign-on callback.
#[utoipa::path(
    post,
    path = "/auth/login/totp/setup",
    tag = "auth",
    responses(
        (status = 200, description = "The new secret", body = TotpSetup),
        (status = 401, description = "No current setup challenge (start again from the sign-in)"),
        (status = 409, description = "Two-factor is already on")
    )
)]
pub async fn login_totp_setup(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<TotpSetup>, ApiError> {
    let user = enroll_challenge(&state, &headers).await?;
    Ok(Json(start_setup(&state, &user).await?))
}

/// Signed in, with two-factor just set up, and the recovery codes to keep.
#[derive(Serialize, ToSchema)]
pub struct EnrolledLogin {
    pub user: PublicUser,
    /// Shown this once: each signs in once in place of a code from the app.
    pub codes: Vec<String>,
}

/// Turn two-factor on with the first code from the app, and sign in. Sets the session cookie.
#[utoipa::path(
    post,
    path = "/auth/login/totp/enable",
    tag = "auth",
    request_body = TotpCode,
    responses(
        (status = 200, description = "Two-factor is on and you are signed in", body = EnrolledLogin),
        (status = 400, description = "Wrong code, or no setup started"),
        (status = 401, description = "No current setup challenge (start again from the sign-in)"),
        (status = 409, description = "Two-factor is already on")
    )
)]
pub async fn login_totp_enable(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    ClientAddr(address): ClientAddr,
    headers: axum::http::HeaderMap,
    Json(input): Json<TotpCode>,
) -> Result<(StatusCode, CookieJar, Json<EnrolledLogin>), ApiError> {
    let user = enroll_challenge(&state, &headers).await?;
    slow_down(&state, &user, address.as_deref()).await;
    let db = state.writer().await?;
    let tx = db.begin().await?;
    let codes = finish_setup(&state, &tx, &user, address.as_deref(), &input.code).await?;
    state.login_limits.succeeded(&user.email);
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "login")
            .target("user", user.id)
            .data(serde_json::json!({ "totp": "enrolled" })),
    )
    .await?;
    tx.commit().await?;
    drop(db);
    let session = sessions::sign_in(&mut auth_session, &state, &user).await?;
    Ok((
        StatusCode::OK,
        session.add(totp::challenge_cookie_removal(state.secure_cookies)),
        Json(EnrolledLogin {
            user: PublicUser::from(&user),
            codes,
        }),
    ))
}

/// Start replacing the authenticator app while two-factor stays on (also where it is required): with a
/// current code from the old app or a recovery code (spent), a new secret to scan. The old app keeps
/// working until a code from the new one confirms it (`POST /auth/totp/replace/confirm`); starting again
/// replaces the unconfirmed secret.
#[utoipa::path(
    post,
    path = "/auth/totp/replace",
    tag = "auth",
    security(("session_cookie" = [])),
    request_body = TotpCode,
    responses(
        (status = 200, description = "The new secret", body = TotpSetup),
        (status = 400, description = "Wrong code, or two-factor is off"),
        (status = 401, description = "Not logged in"),
        (status = 403, description = "Made with an API key")
    )
)]
pub async fn totp_replace(
    State(state): State<AppState>,
    ClientAddr(address): ClientAddr,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<TotpCode>,
) -> Result<Json<TotpSetup>, ApiError> {
    slow_down(&state, &user, address.as_deref()).await;
    let secret = totp::new_secret();
    let otpauth_uri = totp::otpauth_uri(&secret, &user.email)?;
    let db = state.writer().await?;
    let tx = db.begin().await?;
    require_code(&state, &tx, &user, address.as_deref(), &input.code).await?;
    entity::user_totp::ActiveModel {
        user_id: Unchanged(user.id),
        pending_secret: Set(Some(secret.clone())),
        ..Default::default()
    }
    .update(&tx)
    .await?;
    tx.commit().await?;
    Ok(Json(TotpSetup {
        secret,
        otpauth_uri,
    }))
}

/// Finish replacing the authenticator app with a code from the new one: the new secret takes over (the
/// old app stops working); the recovery codes stay. A wrong code changes nothing.
#[utoipa::path(
    post,
    path = "/auth/totp/replace/confirm",
    tag = "auth",
    security(("session_cookie" = [])),
    request_body = TotpCode,
    responses(
        (status = 204, description = "The new app is the one in use"),
        (status = 400, description = "Wrong code, or no replacement started"),
        (status = 401, description = "Not logged in"),
        (status = 403, description = "Made with an API key"),
        (status = 409, description = "Another replacement started meanwhile")
    )
)]
pub async fn totp_replace_confirm(
    State(state): State<AppState>,
    mut auth_session: AuthSession,
    ClientAddr(address): ClientAddr,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<TotpCode>,
) -> Result<StatusCode, ApiError> {
    slow_down(&state, &user, address.as_deref()).await;
    let db = state.writer().await?;
    let tx = db.begin().await?;
    let (row, pending) = totp::enabled(&tx, user.id)
        .await?
        .and_then(|row| row.pending_secret.clone().map(|p| (row, p)))
        .ok_or_else(|| ApiError::bad_request("start the replacement first"))?;
    // A new secret: the old one's used steps say nothing about it.
    let Some(step) = totp::matching_step(&pending, input.code.trim(), totp::now_unix(), None)
    else {
        drop(tx);
        state.login_limits.failed(&user.email, address.as_deref());
        return Err(ApiError::bad_request("that code is not right"));
    };
    // Only if it is still the secret just checked (another replacement may have started meanwhile).
    let replaced = entity::user_totp::Entity::update_many()
        .col_expr(
            entity::user_totp::Column::Secret,
            Expr::value(pending.clone()),
        )
        .col_expr(
            entity::user_totp::Column::PendingSecret,
            Expr::value(Option::<String>::None),
        )
        .col_expr(
            entity::user_totp::Column::LastStep,
            Expr::value(step as i64),
        )
        .filter(entity::user_totp::Column::UserId.eq(row.user_id))
        .filter(entity::user_totp::Column::PendingSecret.eq(pending))
        .exec(&tx)
        .await?
        .rows_affected;
    if replaced != 1 {
        return Err(ApiError::conflict(
            "the replacement changed meanwhile; start again",
        ));
    }
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "totp_replaced").target("user", user.id),
    )
    .await?;
    tx.commit().await?;
    drop(db);
    // The two-factor secret is part of the session hash: the other sessions end, this one stays.
    sessions::refresh(&mut auth_session, &state, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
