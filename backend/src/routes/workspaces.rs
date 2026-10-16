use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Role, Workspace, WorkspaceMember};
use crate::state::AppState;
use crate::{time, trash};

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

const MAX_DESCRIPTION: usize = 2000;

pub(crate) fn clean_description(text: &str) -> Result<String, ApiError> {
    let text = text.trim();
    if text.chars().count() > MAX_DESCRIPTION {
        return Err(ApiError::bad_request(format!(
            "description can be at most {MAX_DESCRIPTION} characters"
        )));
    }
    Ok(text.to_string())
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
    let db = &mut state.db();
    let all: Vec<Workspace> = Workspace::all()
        .exec(db)
        .await?
        .into_iter()
        .filter(|w| w.deleted_at.is_none())
        .collect();
    if user.super_admin || user.global_role.is_some() {
        return Ok(Json(all));
    }
    let member_of: HashSet<uuid::Uuid> = WorkspaceMember::filter_by_user_id(user.id)
        .exec(db)
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
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    let description = clean_description(&input.description)?;

    let mut db = state.writer().await;
    let mut tx = db.transaction().await?;
    let workspace = toasty::create!(Workspace {
        name: name.to_string(),
        description,
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
    audit::record(
        &mut tx,
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
    let db = &mut state.db();
    access::require_access(db, &user, id, Role::Read).await?;
    Ok(Json(Workspace::get_by_id(db, &id).await?))
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
    let name = match &input.name {
        Some(n) if n.trim().is_empty() => return Err(ApiError::bad_request("name is required")),
        Some(n) => Some(n.trim().to_string()),
        None => None,
    };
    let description = input
        .description
        .as_deref()
        .map(clean_description)
        .transpose()?;
    let mut db = state.writer().await;
    access::require_access(&mut *db, &user, id, Role::Admin).await?;
    let mut workspace = Workspace::get_by_id(&mut *db, &id).await?;
    if name.is_some() || description.is_some() {
        let mut update = workspace.update();
        if let Some(name) = name {
            update = update.name(name);
        }
        if let Some(description) = description {
            update = update.description(description);
        }
        update.exec(&mut *db).await?;
    }
    Ok(Json(workspace))
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
    let mut db = state.writer().await;
    access::require_access(&mut *db, &user, id, Role::Admin).await?;
    let workspace = Workspace::get_by_id(&mut *db, &id).await?;
    let mut tx = db.transaction().await?;
    trash::delete_workspace(&mut tx, id, time::now()).await?;
    audit::record(
        &mut tx,
        Entry::admin(Some(user.id), "workspace_deleted")
            .target("workspace", id)
            .in_workspace(id)
            .data(serde_json::json!({ "name": workspace.name })),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
