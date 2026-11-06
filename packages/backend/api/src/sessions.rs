//! Sign-in sessions: tower-sessions keeps them, axum-login says who is signed in.
//!
//! [`Store`] is our own `SessionStore`: the community SQL store (`tower-sessions-sqlx-store`) needs sqlx
//! 0.8, and a second bundled SQLite does not link next to ours (sqlx 0.9). Rows are keyed by the SHA-256 of
//! the session id, never the id itself, so the database (or a backup of it) holds no live session; every
//! write takes the app's writer turn (`AppState::writer`), like any other write.
//!
//! [`Backend`] loads the signed-in user on each request: `None` for a missing or disabled account, so
//! disabling someone stops their sessions. Its `session_auth_hash` is a hash of the password hash and the
//! two-factor secret, so changing either signs out every other session of theirs (the one that made the
//! change signs in again: [`refresh`]).

use std::convert::Infallible;
use std::fmt;

use async_trait::async_trait;
use axum_extra::extract::cookie::{Cookie, CookieJar};
use axum_login::{AuthUser, AuthnBackend};
use sha2::{Digest, Sha256};
use tower_sessions::cookie::SameSite;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, ExpiredDeletion, SessionStore};
use tower_sessions::{Expiry, SessionManagerLayer};

use crate::db::Db;
use crate::db::prelude::*;
use crate::error::ApiError;
use crate::state::AppState;
use crate::{time, totp};
use entity::{User, sessions, users};

/// How long a sign-in lasts.
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// Where a session keeps when it ends (Unix seconds), so signing in again after a password change keeps
/// it: tower-sessions does not remember a session's expiry between requests.
const EXPIRES_KEY: &str = "woodmouse.expires";
/// The key axum-login keeps its data under (its default).
const AUTH_DATA_KEY: &str = "axum-login.data";

/// The session cookie's name over plain http (development); see [`cookie_name`].
pub const SESSION_COOKIE: &str = "woodmouse_session";
/// Its name over https: browsers accept a `__Host-` cookie only with `Secure`, `Path=/` and no `Domain`,
/// so a sibling subdomain cannot plant a session of its own (login CSRF, cookie tossing).
pub const SECURE_SESSION_COOKIE: &str = "__Host-woodmouse_session";

/// The session cookie's name: prefixed when the site is https (`WOODMOUSE_SECURE_COOKIES`).
pub fn cookie_name(secure: bool) -> &'static str {
    if secure {
        SECURE_SESSION_COOKIE
    } else {
        SESSION_COOKIE
    }
}

/// The signed-in session, as handlers take it.
pub type AuthSession = axum_login::AuthSession<Backend>;

/// The layer that gives every request its session and, through it, its user. Its cookie: the name above,
/// `HttpOnly`, `SameSite=Lax`, `Path=/`, `Secure` over https; its lifetime is set at sign-in.
pub fn layer(state: &AppState) -> axum_login::AuthManagerLayer<Backend, Store> {
    let sessions = SessionManagerLayer::new(Store {
        state: state.clone(),
    })
    .with_name(cookie_name(state.secure_cookies))
    .with_http_only(true)
    .with_same_site(SameSite::Lax)
    .with_secure(state.secure_cookies)
    .with_path("/");
    axum_login::AuthManagerLayerBuilder::new(
        Backend {
            state: state.clone(),
        },
        sessions,
    )
    .build()
}

/// tower-sessions removes the cookie (sign-out, or a session that has ended) with a cookie built from the
/// request's, which has no attributes; a browser ignores a `__Host-` removal without `Secure`. Over https,
/// this adds it (wrapped around [`layer`] in `routes::api`).
pub async fn secure_removals(
    axum::extract::State(secure): axum::extract::State<bool>,
    mut response: axum::response::Response,
) -> axum::response::Response {
    if !secure {
        return response;
    }
    let prefix = format!("{SECURE_SESSION_COOKIE}=");
    let headers = response.headers_mut();
    let fixed: Vec<_> = headers
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .map(|value| match value.to_str() {
            Ok(text) if text.starts_with(&prefix) && !text.contains("; Secure") => {
                axum::http::HeaderValue::from_str(&format!(
                    "{text}; Secure; HttpOnly; SameSite=Lax"
                ))
                .unwrap_or_else(|_| value.clone())
            }
            _ => value.clone(),
        })
        .collect();
    headers.remove(axum::http::header::SET_COOKIE);
    for value in fixed {
        headers.append(axum::http::header::SET_COOKIE, value);
    }
    response
}

/// The stored key of a session id.
fn id_hash(id: &Id) -> String {
    hex(&Sha256::digest(id.0.to_le_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn to_time(at: ::time::OffsetDateTime) -> time::DateTime {
    time::from_timestamp(
        jiff::Timestamp::from_second(at.unix_timestamp()).unwrap_or(jiff::Timestamp::MAX),
    )
}

fn from_time(at: time::DateTime) -> ::time::OffsetDateTime {
    ::time::OffsetDateTime::from_unix_timestamp(time::timestamp(at).as_second())
        .unwrap_or(::time::OffsetDateTime::UNIX_EPOCH)
}

fn backend_error(e: impl fmt::Display) -> session_store::Error {
    session_store::Error::Backend(e.to_string())
}

/// The session store (see the module docs).
#[derive(Clone)]
pub struct Store {
    state: AppState,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sessions::Store")
    }
}

impl Store {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    fn row(record: &Record) -> session_store::Result<sessions::ActiveModel> {
        let data = serde_json::to_string(&record.data)
            .map_err(|e| session_store::Error::Encode(e.to_string()))?;
        Ok(sessions::ActiveModel {
            id_hash: Set(id_hash(&record.id)),
            data: Set(data),
            expires_at: Set(to_time(record.expiry_date)),
        })
    }

    /// Delete the sessions that have ended; how many.
    pub async fn delete_expired_count(&self) -> Result<u64, ApiError> {
        let db = self.state.writer().await?;
        Ok(sessions::Entity::delete_many()
            .filter(sessions::Column::ExpiresAt.lte(time::now()))
            .exec(&*db)
            .await?
            .rows_affected)
    }
}

#[async_trait]
impl SessionStore for Store {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let db = self.state.writer().await.map_err(backend_error)?;
        // Writes take turns, so nothing can take the id between the check and the insert.
        while sessions::Entity::find_by_id(id_hash(&record.id))
            .one(&*db)
            .await
            .map_err(backend_error)?
            .is_some()
        {
            record.id = Id::default();
        }
        Self::row(record)?
            .insert(&*db)
            .await
            .map_err(backend_error)?;
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let db = self.state.writer().await.map_err(backend_error)?;
        sessions::Entity::insert(Self::row(record)?)
            .on_conflict(
                sea_orm::sea_query::OnConflict::column(sessions::Column::IdHash)
                    .update_columns([sessions::Column::Data, sessions::Column::ExpiresAt])
                    .to_owned(),
            )
            .exec(&*db)
            .await
            .map_err(backend_error)?;
        Ok(())
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let Some(row) = sessions::Entity::find_by_id(id_hash(id))
            .filter(sessions::Column::ExpiresAt.gt(time::now()))
            .one(self.state.db())
            .await
            .map_err(backend_error)?
        else {
            return Ok(None);
        };
        Ok(Some(Record {
            id: *id,
            data: serde_json::from_str(&row.data)
                .map_err(|e| session_store::Error::Decode(e.to_string()))?,
            expiry_date: from_time(row.expires_at),
        }))
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        let db = self.state.writer().await.map_err(backend_error)?;
        sessions::Entity::delete_by_id(id_hash(id))
            .exec(&*db)
            .await
            .map_err(backend_error)?;
        Ok(())
    }
}

#[async_trait]
impl ExpiredDeletion for Store {
    async fn delete_expired(&self) -> session_store::Result<()> {
        self.delete_expired_count()
            .await
            .map(|_| ())
            .map_err(backend_error)
    }
}

/// The signed-in user, with the hash that ties their sessions to their password and two-factor secret.
#[derive(Clone)]
pub struct SessionUser {
    pub user: User,
    auth_hash: Vec<u8>,
}

impl fmt::Debug for SessionUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionUser")
            .field("id", &self.user.id)
            .finish_non_exhaustive()
    }
}

impl AuthUser for SessionUser {
    type Id = uuid::Uuid;

    fn id(&self) -> uuid::Uuid {
        self.user.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        &self.auth_hash
    }
}

/// SHA-256 of the password hash and the two-factor secret (each may be empty).
fn auth_hash(password_hash: Option<&str>, totp_secret: Option<&str>) -> Vec<u8> {
    Sha256::new()
        .chain_update(password_hash.unwrap_or("").as_bytes())
        .chain_update([0])
        .chain_update(totp_secret.unwrap_or("").as_bytes())
        .finalize()
        .to_vec()
}

/// The user as a session holds them; `None` for a missing or disabled account.
pub async fn session_user(
    db: &impl Db,
    user_id: uuid::Uuid,
) -> Result<Option<SessionUser>, ApiError> {
    let Some(user) = users::Entity::find_by_id(user_id)
        .one(db)
        .await?
        .filter(|u| u.active)
    else {
        return Ok(None);
    };
    let secret = totp::enabled(db, user.id).await?.map(|t| t.secret);
    let auth_hash = auth_hash(user.password_hash.as_deref(), secret.as_deref());
    Ok(Some(SessionUser { user, auth_hash }))
}

/// axum-login's view of our users. Sign-in itself is ours (`routes::auth`, `routes::oidc`, invites): they
/// check the password, the code or the provider, then call [`sign_in`].
#[derive(Clone)]
pub struct Backend {
    state: AppState,
}

impl AuthnBackend for Backend {
    type User = SessionUser;
    type Credentials = Infallible;
    type Error = ApiError;

    async fn authenticate(&self, credentials: Infallible) -> Result<Option<SessionUser>, ApiError> {
        match credentials {}
    }

    async fn get_user(&self, user_id: &uuid::Uuid) -> Result<Option<SessionUser>, ApiError> {
        session_user(self.state.db(), *user_id).await
    }
}

fn login_error(e: axum_login::Error<Backend>) -> ApiError {
    match e {
        axum_login::Error::Backend(e) => e,
        axum_login::Error::Session(e) => anyhow::Error::new(e).context("session").into(),
    }
}

/// Start a session for `user` (after their password, code, provider or invite was checked), lasting
/// [`SESSION_TTL_SECS`]. Call it with no writer held: the store takes the writer turn. Over https, the
/// answer also clears a plain-named session cookie from before the site used the `__Host-` prefix.
pub async fn sign_in(
    auth: &mut AuthSession,
    state: &AppState,
    user: &User,
) -> Result<CookieJar, ApiError> {
    let session_user = session_user(state.db(), user.id)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    auth.login(&session_user).await.map_err(login_error)?;
    let expires = jiff::Timestamp::now().as_second() + SESSION_TTL_SECS;
    keep_until(auth, expires).await?;
    let mut jar = CookieJar::new();
    if state.secure_cookies {
        let mut old = Cookie::new(SESSION_COOKIE, "");
        old.set_path("/");
        old.set_secure(true);
        old.make_removal();
        jar = jar.add(old);
    }
    Ok(jar)
}

async fn keep_until(auth: &AuthSession, unix_seconds: i64) -> Result<(), ApiError> {
    let at = ::time::OffsetDateTime::from_unix_timestamp(unix_seconds)
        .map_err(|e| anyhow::Error::new(e).context("session expiry"))?;
    auth.session.set_expiry(Some(Expiry::AtDateTime(at)));
    auth.session
        .insert(EXPIRES_KEY, unix_seconds)
        .await
        .map_err(|e| anyhow::Error::new(e).context("session"))?;
    Ok(())
}

/// After someone changes their own password or two-factor setting: sign this session in again with the
/// new hash (their other sessions no longer match it and end), keeping when it expires.
pub async fn refresh(
    auth: &mut AuthSession,
    state: &AppState,
    user_id: uuid::Uuid,
) -> Result<(), ApiError> {
    if auth.user.is_none() {
        return Ok(());
    }
    let session_user = session_user(state.db(), user_id)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    let expires = auth
        .session
        .get::<i64>(EXPIRES_KEY)
        .await
        .map_err(|e| anyhow::Error::new(e).context("session"))?
        .unwrap_or_else(|| jiff::Timestamp::now().as_second() + SESSION_TTL_SECS);
    auth.login(&session_user).await.map_err(login_error)?;
    keep_until(auth, expires).await
}

/// End this session (sign out). Call it with no writer held.
pub async fn sign_out(auth: &mut AuthSession) -> Result<(), ApiError> {
    auth.logout().await.map_err(login_error)?;
    Ok(())
}

/// A session for `user_id`, written straight to the table (outside the writer turn): for tests and tools
/// that need someone signed in without going through a sign-in. The cookie's value.
pub async fn start_session(db: &impl Db, user_id: uuid::Uuid) -> Result<String, ApiError> {
    let session_user = session_user(db, user_id)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    let expires = jiff::Timestamp::now().as_second() + SESSION_TTL_SECS;
    let mut data = std::collections::HashMap::new();
    data.insert(
        AUTH_DATA_KEY.to_string(),
        serde_json::json!({ "user_id": user_id, "auth_hash": session_user.auth_hash }),
    );
    data.insert(EXPIRES_KEY.to_string(), serde_json::json!(expires));
    let record = Record {
        id: Id::default(),
        data,
        expiry_date: ::time::OffsetDateTime::from_unix_timestamp(expires)
            .map_err(|e| anyhow::Error::new(e).context("session expiry"))?,
    };
    Store::row(&record)
        .map_err(|e| anyhow::Error::new(e).context("session"))?
        .insert(db)
        .await?;
    Ok(record.id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_changes_with_the_password_and_the_secret() {
        let base = auth_hash(Some("a"), None);
        assert_ne!(base, auth_hash(Some("b"), None));
        assert_ne!(base, auth_hash(Some("a"), Some("S")));
        assert_eq!(base, auth_hash(Some("a"), Some("")));
        // The separator keeps "ab" + "" apart from "a" + "b".
        assert_ne!(auth_hash(Some("ab"), None), auth_hash(Some("a"), Some("b")));
    }

    #[test]
    fn rows_are_keyed_by_a_hash_of_the_id() {
        let id = Id::default();
        let key = id_hash(&id);
        assert_eq!(key.len(), 64);
        assert!(!key.contains(&id.to_string()));
    }
}
