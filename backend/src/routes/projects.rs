use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::models::{Project, Role};
use crate::state::AppState;
use crate::trash;

/// All project routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_projects))
        .routes(routes!(create_project))
        .routes(routes!(get_project))
        .routes(routes!(update_project))
        .routes(routes!(delete_project))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateProject {
    pub name: String,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateProject {
    pub name: Option<String>,
}

/// List the projects in a workspace.
#[utoipa::path(
    get,
    path = "/workspaces/{workspace_id}/projects",
    tag = "projects",
    security(("session_cookie" = [])),
    params(("workspace_id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "The workspace's projects", body = [Project]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn list_projects(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(workspace_id): Path<uuid::Uuid>,
) -> Result<Json<Vec<Project>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, workspace_id, Role::Read).await?;
    let projects = Project::filter_by_workspace_id(workspace_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|p| p.deleted_at.is_none())
        .collect();
    Ok(Json(projects))
}

/// Create a project in the workspace.
#[utoipa::path(
    post,
    path = "/workspaces/{workspace_id}/projects",
    tag = "projects",
    security(("session_cookie" = [])),
    params(("workspace_id" = uuid::Uuid, Path, description = "Workspace id")),
    request_body = CreateProject,
    responses(
        (status = 201, description = "Project created", body = Project),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn create_project(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(workspace_id): Path<uuid::Uuid>,
    Json(input): Json<CreateProject>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }

    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, workspace_id, Role::Write).await?;

    let project = toasty::create!(Project {
        workspace_id,
        name: name.to_string(),
    })
    .exec(db)
    .await?;
    Ok((StatusCode::CREATED, Json(project)))
}

/// Fetch one project.
#[utoipa::path(
    get,
    path = "/projects/{id}",
    tag = "projects",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "The project", body = Project),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or no access")
    )
)]
pub async fn get_project(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Project>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_project_access(db, &user, id, Role::Read).await?;
    let project = Project::get_by_id(db, &id).await?;
    Ok(Json(project))
}

/// Rename a project.
#[utoipa::path(
    patch,
    path = "/projects/{id}",
    tag = "projects",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Project id")),
    request_body = UpdateProject,
    responses(
        (status = 200, description = "Project updated", body = Project),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or no access")
    )
)]
pub async fn update_project(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdateProject>,
) -> Result<Json<Project>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_project_access(db, &user, id, Role::Write).await?;
    let mut project = Project::get_by_id(db, &id).await?;

    let mut update = project.update();
    if let Some(name) = input.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApiError::bad_request("name is required"));
        }
        update = update.name(name.to_string());
    }
    update.exec(db).await?;
    Ok(Json(project))
}

/// Delete a project with its lists and tasks (soft delete: it moves to Trash).
#[utoipa::path(
    delete,
    path = "/projects/{id}",
    tag = "projects",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Project id")),
    responses(
        (status = 204, description = "Project deleted"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or no access")
    )
)]
pub async fn delete_project(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_project_access(db, &user, id, Role::Write).await?;

    // Soft delete: the project, its lists and its tasks move to Trash together.
    let mut tx = db.transaction().await?;
    trash::delete_project(&mut tx, id, user.id, auth::now_secs()).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
