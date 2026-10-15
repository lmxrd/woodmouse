use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::models::User;
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
        .routes(routes!(me))
}

/// User as exposed over the API — never includes the password hash.
#[derive(Clone, Serialize, ToSchema)]
pub struct PublicUser {
    pub id: uuid::Uuid,
    pub email: String,
    pub name: String,
    pub super_admin: bool,
}

impl From<&User> for PublicUser {
    fn from(user: &User) -> Self {
        Self {
            id: user.id,
            email: user.email.clone(),
            name: user.name.clone(),
            super_admin: user.super_admin,
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
    pub name: String,
    pub password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

pub(crate) fn validate_identity(email: &str, name: &str) -> Result<(), ApiError> {
    if email.trim().is_empty() || !email.contains('@') {
        return Err(ApiError::bad_request("email is invalid"));
    }
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
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

fn validate_credentials(email: &str, name: &str, password: &str) -> Result<(), ApiError> {
    validate_identity(email, name)?;
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
    validate_credentials(&input.email, &input.name, &input.password)?;

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
