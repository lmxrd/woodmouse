//! Public invite endpoints: look up an invite link and accept it by setting a
//! password. The token in the URL is the credential; it works once and
//! expires after seven days.

use crate::db::Db;
use crate::db::prelude::*;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum_extra::extract::cookie::CookieJar;
use entity::{user_invites, users};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::audit::{self, Entry};
use crate::auth;
use crate::error::ApiError;
use crate::routes::auth::{PublicUser, validate_password};
use crate::state::AppState;
use crate::time;
use entity::{User, UserInvite};

pub fn public_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_invite))
        .routes(routes!(accept_invite))
}

#[derive(Serialize, ToSchema)]
pub struct InviteInfo {
    pub name: String,
    pub email: String,
}

#[derive(Deserialize, ToSchema)]
pub struct AcceptInvite {
    pub password: String,
}

/// A valid (existing, unexpired) invite and the user it belongs to.
async fn load_invite(db: &impl Db, token: &str) -> Result<(UserInvite, User), ApiError> {
    let invite = user_invites::Entity::find()
        .filter(user_invites::Column::Token.eq(token))
        .one_or_not_found(db)
        .await
        .map_err(|_| ApiError::not_found())?;
    if invite.expires_at < time::now() {
        return Err(ApiError::not_found());
    }
    let user = users::Entity::find_by_id(invite.user_id)
        .one_or_not_found(db)
        .await?;
    if !user.active {
        return Err(ApiError::not_found());
    }
    Ok((invite, user))
}

/// Who an invite link is for. Unknown, used and expired links are all 404.
#[utoipa::path(
    get,
    path = "/invites/{token}",
    tag = "auth",
    params(("token" = String, Path, description = "Invite token")),
    responses(
        (status = 200, description = "The invited user", body = InviteInfo),
        (status = 404, description = "Unknown, used or expired invite")
    )
)]
pub async fn get_invite(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<InviteInfo>, ApiError> {
    let db = state.db();
    let (_, user) = load_invite(db, &token).await?;
    Ok(Json(InviteInfo {
        name: user.name,
        email: user.email,
    }))
}

/// Accept an invite: set the password, consume the invite, and log in.
#[utoipa::path(
    post,
    path = "/invites/{token}/accept",
    tag = "auth",
    params(("token" = String, Path, description = "Invite token")),
    request_body = AcceptInvite,
    responses(
        (status = 200, description = "Password set and logged in", body = PublicUser),
        (status = 400, description = "Invalid password"),
        (status = 404, description = "Unknown, used or expired invite")
    )
)]
pub async fn accept_invite(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(input): Json<AcceptInvite>,
) -> Result<(StatusCode, CookieJar, Json<PublicUser>), ApiError> {
    validate_password(&input.password)?;
    let db = state.writer().await?;
    let (_, user) = load_invite(&*db, &token).await?;
    let tx = db.begin().await?;
    users::ActiveModel {
        id: Unchanged(user.id),
        password_hash: Set(auth::hash_password(&input.password)?),
        ..Default::default()
    }
    .update(&tx)
    .await?;
    user_invites::Entity::delete_many()
        .filter(user_invites::Column::UserId.eq(user.id))
        .exec(&tx)
        .await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "invite_accepted").target("user", user.id),
    )
    .await?;
    let (_, jar) = auth::create_session(&tx, user.id, state.secure_cookies).await?;
    tx.commit().await?;
    Ok((StatusCode::OK, jar, Json(PublicUser::from(&user))))
}
