//! Workspace membership: listing, adding (with a one-time invite link for new
//! users), changing roles, removing, and reissuing invites. Mutations are
//! admin-only; like every workspace route, insufficient access is a 404.

use crate::db::Db;
use crate::db::prelude::*;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use entity::{projects, tasks, users, workspace_members};
use entity::{task_assignees, user_invites};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::routes::auth::{PublicUser, validate_identity};
use crate::state::AppState;
use crate::time;
use entity::{Role, User};

const INVITE_TTL_SECS: i64 = 7 * 24 * 60 * 60;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_members, add_member))
        .routes(routes!(update_member, remove_member))
        .routes(routes!(reissue_invite))
}

/// A workspace member: the user and their role in the workspace.
#[derive(Serialize, ToSchema)]
pub struct Member {
    pub user: PublicUser,
    pub role: Role,
    /// True until the user has accepted their invite and set a password.
    pub pending: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct AddMember {
    /// Optional display name; used only when no account with this email exists yet.
    #[serde(default)]
    pub name: String,
    pub email: String,
    pub role: Role,
}

#[derive(Serialize, ToSchema)]
pub struct AddMemberResponse {
    pub member: Member,
    /// Set when the member still has to set a password: the one-time token
    /// for `/invite/{token}`. Absent for users who already have an account.
    pub invite_token: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateMember {
    pub role: Role,
}

#[derive(Serialize, ToSchema)]
pub struct InviteToken {
    pub invite_token: String,
}

fn member_of(user: &User, role: Role) -> Member {
    Member {
        user: PublicUser::from(user),
        role,
        pending: user.password_hash.is_empty(),
    }
}

/// Replace any outstanding invite of the user with a fresh one; returns its token.
pub(crate) async fn issue_invite(db: &impl Db, user_id: uuid::Uuid) -> Result<String, ApiError> {
    user_invites::Entity::delete_many()
        .filter(user_invites::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    user_invites::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        token: Set(token.clone()),
        user_id: Set(user_id),
        expires_at: Set(time::add_seconds(time::now(), INVITE_TTL_SECS)),
    }
    .insert(db)
    .await?;
    Ok(token)
}

async fn admin_count(db: &impl Db, workspace_id: uuid::Uuid) -> Result<usize, ApiError> {
    Ok(workspace_members::Entity::find()
        .filter(workspace_members::Column::WorkspaceId.eq(workspace_id))
        .filter(workspace_members::Column::Role.eq(Role::Admin))
        .count(db)
        .await? as usize)
}

/// List the members of a workspace, ordered by name. Any member may list;
/// super admins act as members everywhere but only appear when they hold a
/// membership row.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/members",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "The workspace's members", body = [Member]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn list_members(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<Member>>, ApiError> {
    let db = state.db();
    access::require_access(db, &user, id, Role::Read).await?;

    let memberships = workspace_members::Entity::find()
        .filter(workspace_members::Column::WorkspaceId.eq(id))
        .all(db)
        .await?;
    if memberships.is_empty() {
        return Ok(Json(vec![]));
    }
    let users = memberships.load_one(users::Entity, db).await?;

    let mut members: Vec<Member> = memberships
        .into_iter()
        .zip(users)
        .filter_map(|(m, u)| u.map(|u| member_of(&u, m.role)))
        .collect();
    members.sort_by_key(|m| m.user.name.to_lowercase());
    Ok(Json(members))
}

/// Add a member by email. An existing account is added directly; otherwise
/// an account is created without a password and the response carries a
/// one-time invite token for the admin to share.
#[utoipa::path(
    post,
    path = "/workspaces/{id}/members",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    request_body = AddMember,
    responses(
        (status = 201, description = "Member added", body = AddMemberResponse),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or not an admin"),
        (status = 409, description = "Already a member")
    )
)]
pub async fn add_member(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<AddMember>,
) -> Result<(StatusCode, Json<AddMemberResponse>), ApiError> {
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Admin).await?;
    validate_identity(&input.email)?;
    let email = input.email.trim().to_lowercase();

    let tx = db.begin().await?;
    let invitee = match users::Entity::find()
        .filter(users::Column::Email.eq(&email))
        .one_or_not_found(&tx)
        .await
    {
        Ok(existing) => {
            if !existing.active {
                return Err(ApiError::bad_request("this account is disabled"));
            }
            existing
        }
        Err(_) => {
            users::ActiveModel {
                id: Set(uuid::Uuid::now_v7()),
                email: Set(email),
                name: Set(input.name.trim().to_string()),
                password_hash: Set(String::new()),
                super_admin: Set(false),
                active: Set(true),
                ..Default::default()
            }
            .insert(&tx)
            .await?
        }
    };

    if workspace_members::Entity::find()
        .filter(workspace_members::Column::UserId.eq(invitee.id))
        .filter(workspace_members::Column::WorkspaceId.eq(id))
        .one(&tx)
        .await?
        .is_some()
    {
        return Err(ApiError::conflict("already a member of this workspace"));
    }
    workspace_members::ActiveModel {
        user_id: Set(invitee.id),
        workspace_id: Set(id),
        role: Set(input.role),
        ..Default::default()
    }
    .insert(&tx)
    .await?;
    audit::record(
        &tx,
        Entry::feed(id, user.id, "member_added")
            .target("user", invitee.id)
            .data(serde_json::json!({ "role": input.role })),
    )
    .await?;

    let invite_token = if invitee.password_hash.is_empty() {
        Some(issue_invite(&tx, invitee.id).await?)
    } else {
        None
    };
    tx.commit().await?;
    if let Some(token) = &invite_token {
        crate::mail::send_invite(
            state.mailer.clone(),
            invitee.email.clone(),
            invitee.name.clone(),
            token.clone(),
        );
    }
    Ok((
        StatusCode::CREATED,
        Json(AddMemberResponse {
            member: member_of(&invitee, input.role),
            invite_token,
        }),
    ))
}

/// Change a member's role. The last admin cannot be demoted.
#[utoipa::path(
    patch,
    path = "/workspaces/{id}/members/{user_id}",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(
        ("id" = uuid::Uuid, Path, description = "Workspace id"),
        ("user_id" = uuid::Uuid, Path, description = "User id")
    ),
    request_body = UpdateMember,
    responses(
        (status = 200, description = "Role changed", body = Member),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such workspace or member, or not an admin"),
        (status = 409, description = "Would leave the workspace without an admin")
    )
)]
pub async fn update_member(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((id, user_id)): Path<(uuid::Uuid, uuid::Uuid)>,
    Json(input): Json<UpdateMember>,
) -> Result<Json<Member>, ApiError> {
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Admin).await?;
    let membership = workspace_members::Entity::find()
        .filter(workspace_members::Column::UserId.eq(user_id))
        .filter(workspace_members::Column::WorkspaceId.eq(id))
        .one(&*db)
        .await?
        .ok_or_else(ApiError::not_found)?;

    if membership.role == Role::Admin
        && input.role != Role::Admin
        && admin_count(&*db, id).await? <= 1
    {
        return Err(ApiError::conflict("a workspace needs at least one admin"));
    }
    let old_role = membership.role;
    let tx = db.begin().await?;
    workspace_members::Entity::update_many()
        .set(workspace_members::ActiveModel {
            role: Set(input.role),
            ..Default::default()
        })
        .filter(workspace_members::Column::UserId.eq(membership.user_id))
        .filter(workspace_members::Column::WorkspaceId.eq(membership.workspace_id))
        .exec(&tx)
        .await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "role_changed")
            .target("user", user_id)
            .in_workspace(id)
            .data(serde_json::json!({ "from": old_role, "to": input.role })),
    )
    .await?;
    tx.commit().await?;

    let target = users::Entity::find_by_id(user_id)
        .one_or_not_found(&*db)
        .await?;
    Ok(Json(member_of(&target, input.role)))
}

/// Remove a member and unassign them from the workspace's tasks. The last
/// admin cannot be removed.
#[utoipa::path(
    delete,
    path = "/workspaces/{id}/members/{user_id}",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(
        ("id" = uuid::Uuid, Path, description = "Workspace id"),
        ("user_id" = uuid::Uuid, Path, description = "User id")
    ),
    responses(
        (status = 204, description = "Member removed"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such workspace or member, or not an admin"),
        (status = 409, description = "Would leave the workspace without an admin")
    )
)]
pub async fn remove_member(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((id, user_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<StatusCode, ApiError> {
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Admin).await?;
    let membership = workspace_members::Entity::find()
        .filter(workspace_members::Column::UserId.eq(user_id))
        .filter(workspace_members::Column::WorkspaceId.eq(id))
        .one(&*db)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if membership.role == Role::Admin && admin_count(&*db, id).await? <= 1 {
        return Err(ApiError::conflict("a workspace needs at least one admin"));
    }

    // Unassign them from this workspace's tasks (live or deleted).
    let task_ids: Vec<uuid::Uuid> = {
        let project_ids: Vec<uuid::Uuid> = projects::Entity::find()
            .filter(projects::Column::WorkspaceId.eq(id))
            .all(&*db)
            .await?
            .into_iter()
            .map(|p| p.id)
            .collect();
        if project_ids.is_empty() {
            vec![]
        } else {
            tasks::Entity::find()
                .filter(tasks::Column::ProjectId.is_in(project_ids.iter().copied()))
                .all(&*db)
                .await?
                .into_iter()
                .map(|t| t.id)
                .collect()
        }
    };
    let tx = db.begin().await?;
    if !task_ids.is_empty() {
        task_assignees::Entity::delete_many()
            .filter(task_assignees::Column::UserId.eq(user_id))
            .filter(task_assignees::Column::TaskId.is_in(task_ids))
            .exec(&tx)
            .await?;
    }
    workspace_members::Entity::delete_many()
        .filter(workspace_members::Column::UserId.eq(membership.user_id))
        .filter(workspace_members::Column::WorkspaceId.eq(membership.workspace_id))
        .exec(&tx)
        .await?;
    audit::record(
        &tx,
        Entry::feed(id, user.id, "member_removed").target("user", user_id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Issue a fresh one-time invite link for a member who has not set a
/// password yet. Any earlier link stops working.
#[utoipa::path(
    post,
    path = "/workspaces/{id}/members/{user_id}/invite",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(
        ("id" = uuid::Uuid, Path, description = "Workspace id"),
        ("user_id" = uuid::Uuid, Path, description = "User id")
    ),
    responses(
        (status = 201, description = "New invite", body = InviteToken),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such workspace or member, or not an admin"),
        (status = 409, description = "The member already has a password")
    )
)]
pub async fn reissue_invite(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((id, user_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<(StatusCode, Json<InviteToken>), ApiError> {
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Admin).await?;
    workspace_members::Entity::find()
        .filter(workspace_members::Column::UserId.eq(user_id))
        .filter(workspace_members::Column::WorkspaceId.eq(id))
        .one(&*db)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let target = users::Entity::find_by_id(user_id)
        .one_or_not_found(&*db)
        .await?;
    if !target.password_hash.is_empty() {
        return Err(ApiError::conflict("this member has already set a password"));
    }
    let invite_token = issue_invite(&*db, user_id).await?;
    Ok((StatusCode::CREATED, Json(InviteToken { invite_token })))
}
