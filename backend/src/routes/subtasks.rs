//! Subtasks: ordinary tasks with a `parent_id`, nested to any depth. A subtask lives in its parent's
//! list, so boards and lists (which hide subtasks by default) stay uncluttered.

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
    let db = &mut *state.db.lock().await;
    access::require_task_access(db, &user, id, Role::Read).await?;
    let task = Task::get_by_id(db, &id).await?;

    let live: Vec<Task> = Task::filter_by_list_id(task.list_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|t| t.deleted_at.is_none())
        .collect();
    let below: Vec<uuid::Uuid> = trash::subtree(&live, id)
        .into_iter()
        .filter(|t| *t != id)
        .collect();
    let mut subtasks: Vec<Task> = live.into_iter().filter(|t| below.contains(&t.id)).collect();
    subtasks.sort_by(|a, b| a.position.total_cmp(&b.position));
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
    let db = &mut *state.db.lock().await;
    let workspace_id = access::require_task_access(db, &user, id, Role::Write).await?;
    let parent = Task::get_by_id(db, &id).await?;
    let response =
        create_task_in(db, workspace_id, parent.list_id, Some(id), user.id, input).await?;
    Ok((StatusCode::CREATED, Json(response)))
}
