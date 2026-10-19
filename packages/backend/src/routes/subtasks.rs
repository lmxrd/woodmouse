//! Subtasks: ordinary tasks with a `parent_id`, nested to any depth. A subtask lives with its
//! parent (same project and list, or the inbox), so boards and lists (which hide subtasks by default) stay uncluttered.

use crate::db::prelude::*;
use crate::models::tasks;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Role, Task};
use crate::routes::tasks::{CreateTaskRequest, TaskResponse, create_task_in, hydrate_tasks};
use crate::state::AppState;
use crate::trash;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_subtasks, create_subtask))
}

/// All subtasks below a task, at any depth, in list order. Each carries its `parent_id`, so the
/// client can build the tree.
#[utoipa::path(
    get,
    path = "/tasks/{id}/subtasks",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    responses(
        (status = 200, description = "The task's subtasks, flat", body = [TaskResponse]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No task with that id, or no access")
    )
)]
pub async fn list_subtasks(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<TaskResponse>>, ApiError> {
    let db = state.db();
    let (task, _) = access::require_task(db, &user, id, Role::Read).await?;

    let live: Vec<Task> = tasks::Entity::find()
        .filter(tasks::Column::ProjectId.eq(task.project_id))
        .filter(tasks::Column::DeletedAt.is_null())
        .order_by_asc(tasks::Column::Position)
        .order_by_asc(tasks::Column::Id)
        .all(db)
        .await?;
    let below: Vec<uuid::Uuid> = trash::subtree(&live, id)
        .into_iter()
        .filter(|t| *t != id)
        .collect();
    let subtasks: Vec<Task> = live.into_iter().filter(|t| below.contains(&t.id)).collect();
    Ok(Json(hydrate_tasks(db, subtasks).await?))
}

/// Add a subtask under a task (which may itself be a subtask).
#[utoipa::path(
    post,
    path = "/tasks/{id}/subtasks",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Parent task id")),
    request_body = CreateTaskRequest,
    responses(
        (status = 201, description = "Subtask created", body = TaskResponse),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No task with that id, or no access")
    )
)]
pub async fn create_subtask(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), ApiError> {
    let db = state.writer().await?;
    let (parent, workspace_id) = access::require_task(&*db, &user, id, Role::Write).await?;
    let response = create_task_in(
        &*db,
        workspace_id,
        parent.project_id,
        parent.list_id,
        Some(id),
        user.id,
        input,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}
