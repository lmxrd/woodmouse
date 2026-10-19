//! Public invite endpoints: look up an invite link and accept it by setting a
//! password. The token in the URL is the credential; it works once and
//! expires after seven days.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::audit::{self, Entry};
use crate::auth;
use crate::error::ApiError;
use crate::models::{User, UserInvite};
use crate::routes::auth::{PublicUser, validate_password};
use crate::state::AppState;
use crate::time;

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
async fn load_invite(
    db: &mut dyn toasty::Executor,
    token: &str,
) -> Result<(UserInvite, User), ApiError> {
    let invite = UserInvite::get_by_token(db, token)
        .await
        .map_err(|_| ApiError::not_found())?;
    if invite.expires_at < time::now() {
        return Err(ApiError::not_found());
    }
    let user = User::get_by_id(db, &invite.user_id).await?;
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
    let db = &mut state.db();
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
    let mut db = state.writer().await;
    let (_, mut user) = load_invite(&mut *db, &token).await?;
    let mut tx = db.transaction().await?;
    user.update()
        .password_hash(auth::hash_password(&input.password)?)
        .exec(&mut tx)
        .await?;
    UserInvite::filter_by_user_id(user.id)
        .delete()
        .exec(&mut tx)
        .await?;
    audit::record(
        &mut tx,
        Entry::admin(Some(user.id), "invite_accepted").target("user", user.id),
    )
    .await?;
    let (_, jar) = auth::create_session(&mut tx, user.id, state.secure_cookies).await?;
    tx.commit().await?;
    Ok((StatusCode::OK, jar, Json(PublicUser::from(&user))))
}
