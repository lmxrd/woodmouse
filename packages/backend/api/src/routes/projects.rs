use crate::db::prelude::*;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use entity::projects;
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::api::{description, required};
use crate::audit;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::state::AppState;
use crate::{time, trash};
use entity::{Project, Role};

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
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub description: Option<String>,
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
    let db = state.db();
    access::require_access(db, &user, workspace_id, Role::Read).await?;
    let projects = projects::Entity::find()
        .filter(projects::Column::WorkspaceId.eq(workspace_id))
        .filter(projects::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
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
    let name = required(&input.name, "name")?;
    let description = description(&input.description)?;

    let db = state.writer().await?;
    access::require_access(&*db, &user, workspace_id, Role::Write).await?;

    // A project's inbox is not a list: it is the tasks that have none.
    let project = projects::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        workspace_id: Set(workspace_id),
        name: Set(name),
        description: Set(description),
        ..Default::default()
    }
    .insert(&*db)
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
    let db = state.db();
    let project = access::require_project(db, &user, id, Role::Read).await?;
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
    let db = state.writer().await?;
    let project = access::require_project(&*db, &user, id, Role::Write).await?;
    let description = input.description.as_deref().map(description).transpose()?;

    let mut update = projects::ActiveModel::from(project);
    if let Some(name) = input.name {
        update.name = Set(required(&name, "name")?);
    }
    if let Some(description) = description {
        update.description = Set(description);
    }
    Ok(Json(update.update(&*db).await?))
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
    let db = state.writer().await?;
    let project = access::require_project(&*db, &user, id, Role::Write).await?;

    // Soft delete: the project, its lists and its tasks move to Trash together.
    let tx = db.begin().await?;
    trash::delete_project(&tx, id, time::now()).await?;
    audit::deleted(
        &tx,
        user.id,
        project.workspace_id,
        "project",
        id,
        &project.name,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
