//! Workspace membership: listing, adding (with a one-time invite link for new
//! users), changing roles, removing, and reissuing invites. Mutations are
//! admin-only; like every workspace route, insufficient access is a 404.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::models::{Invite, List, Project, Role, Task, TaskAssignee, User, WorkspaceMember};
use crate::routes::auth::{PublicUser, validate_identity};
use crate::state::AppState;

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
    /// Used only when no account with this email exists yet.
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
async fn issue_invite(db: &mut toasty::Db, user_id: uuid::Uuid) -> Result<String, ApiError> {
    Invite::filter_by_user_id(user_id).delete().exec(db).await?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    toasty::create!(Invite {
        token: token.clone(),
        user_id,
        expires_at: auth::now_secs() + INVITE_TTL_SECS,
    })
    .exec(db)
    .await?;
    Ok(token)
}

async fn admin_count(db: &mut toasty::Db, workspace_id: uuid::Uuid) -> Result<usize, ApiError> {
    Ok(WorkspaceMember::all()
        .filter(WorkspaceMember::fields().workspace_id().eq(workspace_id))
        .exec(db)
        .await?
        .iter()
        .filter(|m| m.role == Role::Admin)
        .count())
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
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Read).await?;

    let memberships = WorkspaceMember::all()
        .filter(WorkspaceMember::fields().workspace_id().eq(id))
        .exec(db)
        .await?;
    if memberships.is_empty() {
        return Ok(Json(vec![]));
    }
    let user_ids: Vec<uuid::Uuid> = memberships.iter().map(|m| m.user_id).collect();
    let users = User::all()
        .filter(in_list(User::fields().id(), user_ids))
        .exec(db)
        .await?;

    let mut members: Vec<Member> = memberships
        .into_iter()
        .filter_map(|m| {
            users
                .iter()
                .find(|u| u.id == m.user_id)
                .map(|u| member_of(u, m.role))
        })
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
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Admin).await?;
    validate_identity(&input.email, &input.name)?;
    let email = input.email.trim().to_lowercase();

    let invitee = match User::get_by_email(db, &email).await {
        Ok(existing) => existing,
        Err(_) => {
            toasty::create!(User {
                email,
                name: input.name.trim().to_string(),
                password_hash: String::new(),
                super_admin: false,
            })
            .exec(db)
            .await?
        }
    };

    if WorkspaceMember::filter_by_user_id_and_workspace_id(invitee.id, id)
        .first()
        .exec(db)
        .await?
        .is_some()
    {
        return Err(ApiError::conflict("already a member of this workspace"));
    }
    toasty::create!(WorkspaceMember {
        user_id: invitee.id,
        workspace_id: id,
        role: input.role,
    })
    .exec(db)
    .await?;

    let invite_token = if invitee.password_hash.is_empty() {
        Some(issue_invite(db, invitee.id).await?)
    } else {
        None
    };
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
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Admin).await?;
    let mut membership = WorkspaceMember::filter_by_user_id_and_workspace_id(user_id, id)
        .first()
        .exec(db)
        .await?
        .ok_or_else(ApiError::not_found)?;

    if membership.role == Role::Admin
        && input.role != Role::Admin
        && admin_count(db, id).await? <= 1
    {
        return Err(ApiError::conflict("a workspace needs at least one admin"));
    }
    membership.update().role(input.role).exec(db).await?;

    let target = User::get_by_id(db, &user_id).await?;
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
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Admin).await?;
    let membership = WorkspaceMember::filter_by_user_id_and_workspace_id(user_id, id)
        .first()
        .exec(db)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if membership.role == Role::Admin && admin_count(db, id).await? <= 1 {
        return Err(ApiError::conflict("a workspace needs at least one admin"));
    }

    // Unassign them from this workspace's tasks.
    let project_ids: Vec<uuid::Uuid> = Project::filter_by_workspace_id(id)
        .exec(db)
        .await?
        .into_iter()
        .map(|p| p.id)
        .collect();
    if !project_ids.is_empty() {
        let list_ids: Vec<uuid::Uuid> = List::all()
            .filter(in_list(List::fields().project_id(), project_ids))
            .exec(db)
            .await?
            .into_iter()
            .map(|l| l.id)
            .collect();
        if !list_ids.is_empty() {
            let task_ids: Vec<uuid::Uuid> = Task::all()
                .filter(in_list(Task::fields().list_id(), list_ids))
                .exec(db)
                .await?
                .into_iter()
                .map(|t| t.id)
                .collect();
            if !task_ids.is_empty() {
                TaskAssignee::all()
                    .filter(in_list(TaskAssignee::fields().task_id(), task_ids))
                    .filter(TaskAssignee::fields().user_id().eq(user_id))
                    .delete()
                    .exec(db)
                    .await?;
            }
        }
    }
    membership.delete().exec(db).await?;
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
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Admin).await?;
    WorkspaceMember::filter_by_user_id_and_workspace_id(user_id, id)
        .first()
        .exec(db)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let target = User::get_by_id(db, &user_id).await?;
    if !target.password_hash.is_empty() {
        return Err(ApiError::conflict("this member has already set a password"));
    }
    let invite_token = issue_invite(db, user_id).await?;
    Ok((StatusCode::CREATED, Json(InviteToken { invite_token })))
}
