//! Session auth: Argon2id password hashing and cookie-backed sessions.
//!
//! The session token is an opaque UUID stored in an httpOnly, SameSite=Lax
//! cookie. Sessions live in the `sessions` table with an expiry; the
//! [`CurrentUser`] extractor validates them on every request.

use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};

use crate::error::ApiError;
use crate::models::{Session, User};
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "woodmouse_session";
const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_secs() as i64
}

/// Hash a password with Argon2id and the crate's default (OWASP-recommended)
/// parameters. The PHC string encodes salt and parameters.
pub fn hash_password(password: &str) -> Result<String, ApiError> {
    // Random salt from a v4 UUID (122 bits) — no extra RNG dependency.
    let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes())
        .map_err(|_| ApiError::internal("password hashing failed"))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| ApiError::internal("password hashing failed"))
}

/// Verify a password against a stored PHC hash.
pub fn verify_password(password: &str, phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Create a session for `user_id` and the Set-Cookie jar for it.
pub async fn create_session(
    db: &mut toasty::Db,
    user_id: uuid::Uuid,
    secure_cookies: bool,
) -> Result<(Session, CookieJar), ApiError> {
    let token = uuid::Uuid::new_v4().to_string();
    let session = toasty::create!(Session {
        token,
        user_id,
        expires_at: now_secs() + SESSION_TTL_SECS,
    })
    .exec(db)
    .await?;

    let mut cookie = Cookie::new(SESSION_COOKIE, session.token.clone());
    cookie.set_path("/");
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_max_age(time::Duration::seconds(SESSION_TTL_SECS));
    if secure_cookies {
        cookie.set_secure(true);
    }

    let jar = CookieJar::new().add(cookie);
    Ok((session, jar))
}

/// Delete a session by its token (logout). Unknown tokens are fine.
pub async fn delete_session(db: &mut toasty::Db, token: &str) -> Result<(), ApiError> {
    if let Ok(session) = Session::get_by_token(db, token).await {
        session.delete().exec(db).await?;
    }
    Ok(())
}

/// The authenticated user, extracted from the session cookie. Rejects with
/// 401 when the request carries no valid (unexpired) session.
pub struct CurrentUser {
    pub user: User,
    /// Session token, so handlers (logout) can invalidate it.
    pub session_token: String,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let Some(token) = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()) else {
            return Err(ApiError::unauthorized());
        };

        let db = &mut *state.db.lock().await;
        let session = Session::get_by_token(db, &token)
            .await
            .map_err(|_| ApiError::unauthorized())?;

        if now_secs() > session.expires_at {
            let _ = session.delete().exec(db).await;
            return Err(ApiError::unauthorized());
        }

        let user = User::get_by_id(db, &session.user_id)
            .await
            .map_err(|_| ApiError::unauthorized())?;

        Ok(CurrentUser {
            user,
            session_token: token,
        })
    }
}
