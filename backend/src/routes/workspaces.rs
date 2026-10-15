use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Role, User, Workspace, WorkspaceMember};
use crate::routes::auth::PublicUser;
use crate::state::AppState;

/// All workspace routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_workspaces, create_workspace))
        .routes(routes!(get_workspace))
        .routes(routes!(list_members))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateWorkspace {
    pub name: String,
}

/// List the workspaces the current user can see: their memberships (super
/// admins see all).
#[utoipa::path(
    get,
    path = "/workspaces",
    tag = "workspaces",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "Visible workspaces", body = [Workspace]),
        (status = 401, description = "Not logged in")
    )
)]
pub async fn list_workspaces(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<Vec<Workspace>>, ApiError> {
    let db = &mut *state.db.lock().await;
    if user.super_admin {
        let workspaces = Workspace::all().exec(db).await?;
        return Ok(Json(workspaces));
    }

    let members = WorkspaceMember::filter_by_user_id(user.id).exec(db).await?;
    let member_of: std::collections::HashSet<uuid::Uuid> =
        members.iter().map(|m| m.workspace_id).collect();
    let workspaces = Workspace::all()
        .exec(db)
        .await?
        .into_iter()
        .filter(|w| member_of.contains(&w.id))
        .collect();
    Ok(Json(workspaces))
}

/// Create a workspace. The creator becomes its admin member.
#[utoipa::path(
    post,
    path = "/workspaces",
    tag = "workspaces",
    security(("session_cookie" = [])),
    request_body = CreateWorkspace,
    responses(
        (status = 201, description = "Workspace created", body = Workspace),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in")
    )
)]
pub async fn create_workspace(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<CreateWorkspace>,
) -> Result<(StatusCode, Json<Workspace>), ApiError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }

    let db = &mut *state.db.lock().await;
    let mut tx = db.transaction().await?;

    let workspace = toasty::create!(Workspace {
        name: name.to_string(),
    })
    .exec(&mut tx)
    .await?;

    toasty::create!(WorkspaceMember {
        user_id: user.id,
        workspace_id: workspace.id,
        role: Role::Admin,
    })
    .exec(&mut tx)
    .await?;

    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

/// Fetch one workspace. Requires membership (any role).
#[utoipa::path(
    get,
    path = "/workspaces/{id}",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "The workspace", body = Workspace),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn get_workspace(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Workspace>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Read).await?;
    let workspace = Workspace::get_by_id(db, &id).await?;
    Ok(Json(workspace))
}

/// A workspace member: the user and their role in the workspace.
#[derive(Serialize, ToSchema)]
pub struct Member {
    pub user: PublicUser,
    pub role: Role,
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
            users.iter().find(|u| u.id == m.user_id).map(|u| Member {
                user: PublicUser::from(u),
                role: m.role,
            })
        })
        .collect();
    members.sort_by_key(|m| m.user.name.to_lowercase());
    Ok(Json(members))
}
