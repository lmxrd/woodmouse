//! API keys: a person's tokens for scripts and integrations (`Authorization: Bearer wm_...`). Listing,
//! creating and revoking them takes a session: a key can never manage keys (`auth::api_key_scope_allows`).

use crate::db::prelude::*;
use base64::Engine;
use entity::{ApiKey, Role, api_keys};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::audit::{self, Entry};
use crate::auth::{self, CurrentUser};
use crate::error::{ApiError, ErrorBody};
use crate::extract::ApiJson;
use crate::state::AppState;
use crate::time::{self, DateTime};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_api_keys, create_api_key))
        .routes(routes!(revoke_api_key))
}

/// Characters of the token kept to tell keys apart (`wm_` and the first eight after it).
const PREFIX_LEN: usize = 11;
/// The longest an expiry may be set.
const MAX_EXPIRY_DAYS: i64 = 3650;

/// An API key as its owner sees it (never the token, which is shown only when it is made).
#[derive(Serialize, ToSchema)]
pub struct ApiKeyView {
    pub id: uuid::Uuid,
    pub name: String,
    /// The start of the token, to tell keys apart.
    pub prefix: String,
    /// `read` (GET only), `write` (also changes; no site admin) or `admin` (as you).
    pub scope: Role,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime,
    /// `null` until it is first used; updated at most once a minute.
    #[serde(with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub last_used_at: Option<DateTime>,
    /// `null` for a key that does not expire.
    #[serde(with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub expires_at: Option<DateTime>,
}

impl From<ApiKey> for ApiKeyView {
    fn from(key: ApiKey) -> Self {
        Self {
            id: key.id,
            name: key.name,
            prefix: key.prefix,
            scope: key.scope,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
            expires_at: key.expires_at,
        }
    }
}

/// Your API keys that are not revoked, newest first (expired ones too, until revoked).
#[utoipa::path(
    get,
    path = "/auth/api-keys",
    tag = "auth",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "Your API keys", body = [ApiKeyView]),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Made with an API key: keys are managed with a session only", body = ErrorBody)
    )
)]
pub async fn list_api_keys(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<Vec<ApiKeyView>>, ApiError> {
    let keys = api_keys::Entity::find()
        .filter(api_keys::Column::UserId.eq(user.id))
        .filter(api_keys::Column::RevokedAt.is_null())
        .order_by_desc(api_keys::Column::Id)
        .all(state.db())
        .await?;
    Ok(Json(keys.into_iter().map(ApiKeyView::from).collect()))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateApiKey {
    /// What it is for.
    pub name: String,
    /// `read`, `write` or `admin` (admin only for the super admin or someone with the global admin role).
    pub scope: Role,
    /// Days until it stops working; leave it out for a key that does not expire.
    #[serde(default)]
    pub expires_in_days: Option<i64>,
}

/// A new key, with its token: shown this once.
#[derive(Serialize, ToSchema)]
pub struct CreatedApiKey {
    pub key: ApiKeyView,
    /// `wm_...`: send it as `Authorization: Bearer <token>`.
    pub token: String,
}

/// Make an API key. It acts as you, capped at its scope.
#[utoipa::path(
    post,
    path = "/auth/api-keys",
    tag = "auth",
    security(("session_cookie" = [])),
    request_body = CreateApiKey,
    responses(
        (status = 201, description = "The new key and its token", body = CreatedApiKey),
        (status = 400, description = "No name, or a bad expiry", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "An admin key without being an admin, or made with an API key", body = ErrorBody),
        (status = 415, description = "The body must be JSON (Content-Type: application/json)", body = ErrorBody)
    )
)]
pub async fn create_api_key(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    ApiJson(input): ApiJson<CreateApiKey>,
) -> Result<(StatusCode, Json<CreatedApiKey>), ApiError> {
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(ApiError::bad_request("A name of 1 to 100 characters"));
    }
    if input.scope == Role::Admin && !(user.super_admin || user.global_role == Some(Role::Admin)) {
        return Err(ApiError::forbidden("Only an admin may make an admin key"));
    }
    let now = time::now();
    let expires_at = match input.expires_in_days {
        None => None,
        Some(days) if (1..=MAX_EXPIRY_DAYS).contains(&days) => {
            Some(time::add_seconds(now, days * 24 * 60 * 60))
        }
        Some(_) => {
            return Err(ApiError::bad_request(format!(
                "Expires_in_days must be 1 to {MAX_EXPIRY_DAYS}"
            )));
        }
    };
    let token = format!(
        "{}{}",
        auth::API_KEY_PREFIX,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(crate::random::bytes::<32>())
    );

    let db = state.writer().await?;
    let tx = db.begin().await?;
    let key = api_keys::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        user_id: Set(user.id),
        name: Set(name),
        prefix: Set(token[..PREFIX_LEN].to_string()),
        token_hash: Set(auth::api_key_hash(&token)),
        scope: Set(input.scope),
        created_at: Set(now),
        last_used_at: Set(None),
        expires_at: Set(expires_at),
        revoked_at: Set(None),
    }
    .insert(&tx)
    .await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "api_key_created")
            .target("api_key", key.id)
            .data(serde_json::json!({ "name": key.name, "scope": key.scope })),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedApiKey {
            key: key.into(),
            token,
        }),
    ))
}

/// Revoke one of your API keys: it stops working at once.
#[utoipa::path(
    delete,
    path = "/auth/api-keys/{id}",
    tag = "auth",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "API key id")),
    responses(
        (status = 204, description = "Revoked"),
        (status = 400, description = "The request is invalid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Made with an API key: keys are managed with a session only", body = ErrorBody),
        (status = 404, description = "Not your key, or revoked already", body = ErrorBody)
    )
)]
pub async fn revoke_api_key(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let db = state.writer().await?;
    let tx = db.begin().await?;
    let key = api_keys::Entity::find_by_id(id)
        .filter(api_keys::Column::UserId.eq(user.id))
        .filter(api_keys::Column::RevokedAt.is_null())
        .one_or_not_found(&tx)
        .await?;
    api_keys::ActiveModel {
        id: Unchanged(key.id),
        revoked_at: Set(Some(time::now())),
        ..Default::default()
    }
    .update(&tx)
    .await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "api_key_revoked")
            .target("api_key", key.id)
            .data(serde_json::json!({ "name": key.name })),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
