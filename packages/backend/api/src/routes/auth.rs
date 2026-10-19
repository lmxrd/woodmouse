use crate::db::prelude::*;
use crate::models::users;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::audit::{self, Entry};
use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::models::{Role, User};
use crate::state::AppState;

/// Routes reachable without a session.
pub fn public_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(auth_status))
        .routes(routes!(bootstrap))
        .routes(routes!(login))
}

/// Routes requiring a session (the auth layer in `routes::mod` enforces it;
/// the annotations document it).
pub fn protected_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(logout))
        .routes(routes!(me, update_me))
        .routes(routes!(change_password))
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
        password_hash: Set(auth::hash_password(&input.password)?),
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
    let (_, jar) = auth::create_session(&tx, user.id, state.secure_cookies).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, jar, Json(PublicUser::from(&user))))
}

/// Log in with email + password; sets the session cookie.
#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Logged in", body = PublicUser),
        (status = 401, description = "Invalid credentials")
    )
)]
pub async fn login(
    State(state): State<AppState>,
    ClientAddr(address): ClientAddr,
    Json(input): Json<LoginRequest>,
) -> Result<(StatusCode, CookieJar, Json<PublicUser>), ApiError> {
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
    // Generic error: never reveal whether the email exists or the account is disabled.
    let ok = user
        .as_ref()
        .is_some_and(|u| u.active && auth::verify_password(&input.password, &u.password_hash));
    let Some(user) = user.filter(|_| ok) else {
        state.login_limits.failed(&email, address.as_deref());
        let _ = audit::record(&*db, Entry::admin(None, "login_failed")).await;
        return Err(ApiError::unauthorized());
    };
    state.login_limits.succeeded(&email);

    let tx = db.begin().await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "login").target("user", user.id),
    )
    .await?;
    let (_, jar) = auth::create_session(&tx, user.id, state.secure_cookies).await?;
    tx.commit().await?;
    Ok((StatusCode::OK, jar, Json(PublicUser::from(&user))))
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
    CurrentUser {
        user,
        session_token,
        ..
    }: CurrentUser,
) -> Result<(StatusCode, CookieJar), ApiError> {
    let db = state.writer().await?;
    let tx = db.begin().await?;
    auth::delete_session(&tx, &session_token).await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "logout").target("user", user.id),
    )
    .await?;
    tx.commit().await?;

    // Removing from the jar marks the cookie for expiry in the response.
    let removal = Cookie::build((auth::SESSION_COOKIE, "")).path("/").build();
    let jar = CookieJar::new().remove(removal);
    Ok((StatusCode::NO_CONTENT, jar))
}

/// The current user.
#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "The current user", body = PublicUser),
        (status = 401, description = "Not logged in")
    )
)]
pub async fn me(CurrentUser { user, .. }: CurrentUser) -> Json<PublicUser> {
    Json(PublicUser::from(&user))
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
        (status = 200, description = "The updated user", body = PublicUser),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 409, description = "That email is already taken")
    )
)]
pub async fn update_me(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<UpdateMe>,
) -> Result<Json<PublicUser>, ApiError> {
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
    Ok(Json(PublicUser::from(&current)))
}

#[derive(Deserialize, ToSchema)]
pub struct ChangePassword {
    pub current_password: String,
    pub new_password: String,
}

/// Change your password. Other sessions of yours are signed out.
#[utoipa::path(
    post,
    path = "/auth/password",
    tag = "auth",
    security(("session_cookie" = [])),
    request_body = ChangePassword,
    responses(
        (status = 204, description = "Password changed"),
        (status = 400, description = "The new password is too short"),
        (status = 401, description = "Not logged in, or the current password is wrong")
    )
)]
pub async fn change_password(
    State(state): State<AppState>,
    CurrentUser {
        user,
        session_token,
        ..
    }: CurrentUser,
    Json(input): Json<ChangePassword>,
) -> Result<StatusCode, ApiError> {
    if !auth::verify_password(&input.current_password, &user.password_hash) {
        return Err(ApiError::unauthorized());
    }
    validate_password(&input.new_password)?;

    let db = state.writer().await?;
    let tx = db.begin().await?;
    let current = users::Entity::find_by_id(user.id)
        .one_or_not_found(&tx)
        .await?;
    users::ActiveModel {
        id: Unchanged(current.id),
        password_hash: Set(auth::hash_password(&input.new_password)?),
        ..Default::default()
    }
    .update(&tx)
    .await?;
    auth::delete_sessions_of(&tx, user.id, Some(&session_token)).await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "password_changed").target("user", user.id),
    )
    .await?;
    tx.commit().await?;
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
