use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::models::{Session, User};
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
    let db = &mut *state.db.lock().await;
    let users = User::all().exec(db).await?;
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

    let db = &mut *state.db.lock().await;
    if !User::all().exec(db).await?.is_empty() {
        return Err(ApiError::conflict(
            "bootstrap is closed: a user already exists",
        ));
    }

    let user = toasty::create!(User {
        email: input.email.trim().to_lowercase(),
        name: input.name.trim().to_string(),
        password_hash: auth::hash_password(&input.password)?,
        super_admin: true,
    })
    .exec(db)
    .await?;

    let (_, jar) = auth::create_session(db, user.id, state.secure_cookies).await?;
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
    Json(input): Json<LoginRequest>,
) -> Result<(StatusCode, CookieJar, Json<PublicUser>), ApiError> {
    let db = &mut *state.db.lock().await;
    // Generic error: never reveal whether the email exists.
    let user = User::get_by_email(db, &input.email.trim().to_lowercase())
        .await
        .map_err(|_| ApiError::unauthorized())?;
    if !auth::verify_password(&input.password, &user.password_hash) {
        return Err(ApiError::unauthorized());
    }

    let (_, jar) = auth::create_session(db, user.id, state.secure_cookies).await?;
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
    CurrentUser { session_token, .. }: CurrentUser,
) -> Result<(StatusCode, CookieJar), ApiError> {
    let db = &mut *state.db.lock().await;
    auth::delete_session(db, &session_token).await?;

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

    let db = &mut *state.db.lock().await;
    let email = match &input.email {
        Some(email) => {
            validate_identity(email)?;
            let email = email.trim().to_lowercase();
            if email != user.email
                && let Ok(other) = User::get_by_email(db, &email).await
                && other.id != user.id
            {
                return Err(ApiError::conflict("that email is already taken"));
            }
            Some(email)
        }
        None => None,
    };

    let mut current = User::get_by_id(db, &user.id).await?;
    if input.name.is_some()
        || email.is_some()
        || input.date_format.is_some()
        || input.time_format.is_some()
        || input.week_start.is_some()
    {
        let mut update = current.update();
        if let Some(name) = &input.name {
            update = update.name(name.trim().to_string());
        }
        if let Some(email) = email {
            update = update.email(email);
        }
        if let Some(format) = input.date_format {
            update = update.date_format(Some(format));
        }
        if let Some(format) = input.time_format {
            update = update.time_format(Some(format));
        }
        if let Some(week_start) = input.week_start {
            update = update.week_start(Some(week_start));
        }
        update.exec(db).await?;
    }
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

    let db = &mut *state.db.lock().await;
    let mut current = User::get_by_id(db, &user.id).await?;
    current
        .update()
        .password_hash(auth::hash_password(&input.new_password)?)
        .exec(db)
        .await?;
    for session in Session::filter_by_user_id(user.id).exec(db).await? {
        if session.token != session_token {
            session.delete().exec(db).await?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}
