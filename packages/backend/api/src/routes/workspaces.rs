use std::collections::HashSet;

use crate::db::prelude::*;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use entity::{workspace_members, workspaces};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::api::{description, required};
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::state::AppState;
use crate::{time, trash};
use entity::{Role, Workspace};

/// All workspace routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_workspaces, create_workspace))
        .routes(routes!(get_workspace, update_workspace, delete_workspace))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateWorkspace {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// List the workspaces the current user can see: their memberships, or all of them for the super admin
/// and for people with a global role. Deleted workspaces are not listed.
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
    let db = state.db();
    let all: Vec<Workspace> = workspaces::Entity::find()
        .filter(workspaces::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .collect();
    if user.super_admin || user.global_role.is_some() {
        return Ok(Json(all));
    }
    let member_of: HashSet<uuid::Uuid> = workspace_members::Entity::find()
        .filter(workspace_members::Column::UserId.eq(user.id))
        .all(db)
        .await?
        .into_iter()
        .map(|m| m.workspace_id)
        .collect();
    Ok(Json(
        all.into_iter()
            .filter(|w| member_of.contains(&w.id))
            .collect(),
    ))
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
    let name = required(&input.name, "name")?;
    let description = description(&input.description)?;

    let db = state.writer().await?;
    let tx = db.begin().await?;
    let workspace = workspaces::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        name: Set(name.to_string()),
        description: Set(description),
        ..Default::default()
    }
    .insert(&tx)
    .await?;
    workspace_members::ActiveModel {
        user_id: Set(user.id),
        workspace_id: Set(workspace.id),
        role: Set(Role::Admin),
        ..Default::default()
    }
    .insert(&tx)
    .await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "workspace_created")
            .target("workspace", workspace.id)
            .in_workspace(workspace.id)
            .data(serde_json::json!({ "name": workspace.name })),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

/// Fetch one workspace. Requires access (any role).
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
    let db = state.db();
    access::require_access(db, &user, id, Role::Read).await?;
    Ok(Json(
        workspaces::Entity::find_by_id(id)
            .one_or_not_found(db)
            .await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateWorkspace {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Rename a workspace or change its description (admins only).
#[utoipa::path(
    patch,
    path = "/workspaces/{id}",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    request_body = UpdateWorkspace,
    responses(
        (status = 200, description = "The workspace", body = Workspace),
        (status = 400, description = "Invalid input"),
        (status = 404, description = "No such workspace, or not an admin")
    )
)]
pub async fn update_workspace(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdateWorkspace>,
) -> Result<Json<Workspace>, ApiError> {
    let name = input
        .name
        .as_deref()
        .map(|n| required(n, "name"))
        .transpose()?;
    let description = input.description.as_deref().map(description).transpose()?;
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Admin).await?;
    let workspace = workspaces::Entity::find_by_id(id)
        .one_or_not_found(&*db)
        .await?;
    let mut update = workspaces::ActiveModel::from(workspace);
    if let Some(name) = name {
        update.name = Set(name);
    }
    if let Some(description) = description {
        update.description = Set(description);
    }
    Ok(Json(update.update(&*db).await?))
}

/// Delete a workspace (admins only). It disappears for everyone; only the super admin can restore it,
/// and nothing in it is destroyed.
#[utoipa::path(
    delete,
    path = "/workspaces/{id}",
    tag = "workspaces",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, description = "No such workspace, or not an admin")
    )
)]
pub async fn delete_workspace(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Admin).await?;
    let workspace = workspaces::Entity::find_by_id(id)
        .one_or_not_found(&*db)
        .await?;
    let tx = db.begin().await?;
    trash::delete_workspace(&tx, id, time::now()).await?;
    audit::record(
        &tx,
        Entry::admin(Some(user.id), "workspace_deleted")
            .target("workspace", id)
            .in_workspace(id)
            .data(serde_json::json!({ "name": workspace.name })),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
