use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::models::{Label, List, Priority, Project, Role, Task, TaskAssignee, TaskLabel, User};
use crate::routes::auth::PublicUser;
use crate::state::AppState;

/// All task routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_tasks))
        .routes(routes!(create_task))
        .routes(routes!(get_task))
        .routes(routes!(update_task))
        .routes(routes!(delete_task))
}

/// Delete a task and its join rows, atomically. Shared with the project and
/// list delete cascades.
pub async fn delete_list_tasks(
    tx: &mut toasty::Transaction<'_>,
    list_id: uuid::Uuid,
) -> Result<(), ApiError> {
    let tasks = Task::filter_by_list_id(list_id).exec(tx).await?;
    let task_ids: Vec<uuid::Uuid> = tasks.iter().map(|t| t.id).collect();
    delete_tasks_by_ids(tx, task_ids).await
}

async fn delete_tasks_by_ids(
    tx: &mut toasty::Transaction<'_>,
    task_ids: Vec<uuid::Uuid>,
) -> Result<(), ApiError> {
    if task_ids.is_empty() {
        return Ok(());
    }
    TaskAssignee::all()
        .filter(in_list(TaskAssignee::fields().task_id(), task_ids.clone()))
        .delete()
        .exec(tx)
        .await?;
    TaskLabel::all()
        .filter(in_list(TaskLabel::fields().task_id(), task_ids.clone()))
        .delete()
        .exec(tx)
        .await?;
    Task::all()
        .filter(in_list(Task::fields().id(), task_ids))
        .delete()
        .exec(tx)
        .await?;
    Ok(())
}

/// A task with its assignees and labels hydrated.
#[derive(Serialize, ToSchema)]
pub struct TaskResponse {
    pub id: uuid::Uuid,
    pub list_id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub title: String,
    pub description: String,
    pub is_completed: bool,
    /// Unix epoch seconds; set while `is_completed`.
    pub completed_at: Option<i64>,
    pub position: f64,
    pub priority: Priority,
    /// Unix epoch seconds, if set.
    pub start_date: Option<i64>,
    /// Unix epoch seconds, if set.
    pub due_date: Option<i64>,
    pub assignees: Vec<PublicUser>,
    pub labels: Vec<Label>,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub priority: Option<Priority>,
    pub start_date: Option<i64>,
    pub due_date: Option<i64>,
    #[serde(default)]
    pub assignee_ids: Vec<uuid::Uuid>,
    #[serde(default)]
    pub label_ids: Vec<uuid::Uuid>,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub is_completed: Option<bool>,
    pub priority: Option<Priority>,
    /// Absent leaves the due date unchanged; `null` clears it.
    #[serde(default, deserialize_with = "double_option")]
    pub due_date: Option<Option<i64>>,
    pub position: Option<f64>,
    /// Absent leaves the start date unchanged; `null` clears it.
    #[serde(default, deserialize_with = "double_option")]
    pub start_date: Option<Option<i64>>,
    /// Move the task to another list in the same workspace. Unless
    /// `position` is also given, the task is appended to the target list.
    pub list_id: Option<uuid::Uuid>,
    /// When provided, replaces the assignee set.
    pub assignee_ids: Option<Vec<uuid::Uuid>>,
    /// When provided, replaces the label set.
    pub label_ids: Option<Vec<uuid::Uuid>>,
}

/// `null` vs absent distinction for clearing optional fields on PATCH.
fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TaskFilters {
    /// Only tasks in this project.
    pub project_id: Option<uuid::Uuid>,
    /// Only tasks in this list.
    pub list_id: Option<uuid::Uuid>,
    /// Only completed (true) or open (false) tasks.
    pub is_completed: Option<bool>,
    /// Only tasks assigned to this user.
    pub assignee_id: Option<uuid::Uuid>,
    /// Only tasks carrying this label.
    pub label_id: Option<uuid::Uuid>,
    /// Sort field: `position` (default), `title`, or `due_date`.
    pub sort: Option<String>,
    /// Sort direction: `asc` (default) or `desc`.
    pub order: Option<String>,
}

/// List the tasks of a workspace, optionally narrowed to a project or list,
/// with filters and sorting. Filters combine with AND.
#[utoipa::path(
    get,
    path = "/workspaces/{workspace_id}/tasks",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("workspace_id" = uuid::Uuid, Path, description = "Workspace id"), TaskFilters),
    responses(
        (status = 200, description = "The matching tasks", body = [TaskResponse]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access; also an unknown project or list filter")
    )
)]
pub async fn list_tasks(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(workspace_id): Path<uuid::Uuid>,
    Query(filters): Query<TaskFilters>,
) -> Result<Json<Vec<TaskResponse>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, workspace_id, Role::Read).await?;

    // The lists whose tasks are in scope. A project or list filter must
    // belong to this workspace, otherwise it looks nonexistent.
    let list_ids: Vec<uuid::Uuid> = if let Some(list_id) = filters.list_id {
        let list = List::get_by_id(db, &list_id).await?;
        let project = Project::get_by_id(db, &list.project_id).await?;
        if project.workspace_id != workspace_id
            || filters.project_id.is_some_and(|p| p != project.id)
        {
            return Err(ApiError::not_found());
        }
        vec![list_id]
    } else {
        let project_ids: Vec<uuid::Uuid> = if let Some(project_id) = filters.project_id {
            let project = Project::get_by_id(db, &project_id).await?;
            if project.workspace_id != workspace_id {
                return Err(ApiError::not_found());
            }
            vec![project_id]
        } else {
            Project::filter_by_workspace_id(workspace_id)
                .exec(db)
                .await?
                .into_iter()
                .map(|p| p.id)
                .collect()
        };
        if project_ids.is_empty() {
            return Ok(Json(vec![]));
        }
        List::all()
            .filter(in_list(List::fields().project_id(), project_ids))
            .exec(db)
            .await?
            .into_iter()
            .map(|l| l.id)
            .collect()
    };
    if list_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let mut query = Task::all().filter(in_list(Task::fields().list_id(), list_ids));
    if let Some(is_completed) = filters.is_completed {
        query = query.filter(Task::fields().is_completed().eq(is_completed));
    }
    if let Some(assignee_id) = filters.assignee_id {
        let task_ids: Vec<uuid::Uuid> = TaskAssignee::filter_by_user_id(assignee_id)
            .exec(db)
            .await?
            .into_iter()
            .map(|a| a.task_id)
            .collect();
        if task_ids.is_empty() {
            return Ok(Json(vec![]));
        }
        query = query.filter(in_list(Task::fields().id(), task_ids));
    }
    if let Some(label_id) = filters.label_id {
        let task_ids: Vec<uuid::Uuid> = TaskLabel::filter_by_label_id(label_id)
            .exec(db)
            .await?
            .into_iter()
            .map(|l| l.task_id)
            .collect();
        if task_ids.is_empty() {
            return Ok(Json(vec![]));
        }
        query = query.filter(in_list(Task::fields().id(), task_ids));
    }

    let desc = matches!(filters.order.as_deref(), Some("desc"));
    // Priority (an embedded enum) cannot order across backends; it is not
    // a sort option. Unknown sort fields fall back to the default.
    let order_by = match (filters.sort.as_deref(), desc) {
        (Some("title"), false) => Task::fields().title().asc(),
        (Some("title"), true) => Task::fields().title().desc(),
        (Some("due_date"), false) => Task::fields().due_date().asc(),
        (Some("due_date"), true) => Task::fields().due_date().desc(),
        (_, true) => Task::fields().position().desc(),
        (_, false) => Task::fields().position().asc(),
    };
    let tasks = query.order_by(order_by).exec(db).await?;

    Ok(Json(hydrate_tasks(db, tasks).await?))
}

/// Create a task at the end of the list.
#[utoipa::path(
    post,
    path = "/lists/{list_id}/tasks",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("list_id" = uuid::Uuid, Path, description = "List id")),
    request_body = CreateTaskRequest,
    responses(
        (status = 201, description = "Task created", body = TaskResponse),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No list with that id, or no access")
    )
)]
pub async fn create_task(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(list_id): Path<uuid::Uuid>,
    Json(input): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), ApiError> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(ApiError::bad_request("title is required"));
    }

    let db = &mut *state.db.lock().await;
    let workspace_id = access::require_list_access(db, &user, list_id, Role::Write).await?;
    validate_assignees(db, &input.assignee_ids, workspace_id).await?;
    let labels = validate_labels(db, &input.label_ids, workspace_id).await?;
    let assignees: Vec<PublicUser> = if input.assignee_ids.is_empty() {
        vec![]
    } else {
        User::all()
            .filter(in_list(User::fields().id(), input.assignee_ids.clone()))
            .exec(db)
            .await?
            .iter()
            .map(PublicUser::from)
            .collect()
    };

    let mut tx = db.transaction().await?;

    // Append: max position + 1.
    let position = Task::filter_by_list_id(list_id)
        .order_by(Task::fields().position().desc())
        .first()
        .exec(&mut tx)
        .await?
        .map_or(1.0, |last| last.position + 1.0);

    let task = toasty::create!(Task {
        list_id,
        title: title.to_string(),
        description: input.description,
        is_completed: false,
        completed_at: None,
        position,
        priority: input.priority.unwrap_or(Priority::Medium),
        start_date: input.start_date,
        due_date: input.due_date,
        parent_id: None,
    })
    .exec(&mut tx)
    .await?;

    for user_id in &input.assignee_ids {
        toasty::create!(TaskAssignee {
            task_id: task.id,
            user_id: *user_id,
        })
        .exec(&mut tx)
        .await?;
    }
    for label_id in &input.label_ids {
        toasty::create!(TaskLabel {
            task_id: task.id,
            label_id: *label_id,
        })
        .exec(&mut tx)
        .await?;
    }
    tx.commit().await?;

    let project_id = List::get_by_id(db, &list_id).await?.project_id;
    let response = TaskResponse {
        id: task.id,
        list_id: task.list_id,
        project_id,
        title: task.title,
        description: task.description,
        is_completed: task.is_completed,
        completed_at: task.completed_at,
        position: task.position,
        priority: task.priority,
        start_date: task.start_date,
        due_date: task.due_date,
        assignees,
        labels,
    };
    Ok((StatusCode::CREATED, Json(response)))
}

/// Fetch one task with assignees and labels.
#[utoipa::path(
    get,
    path = "/tasks/{id}",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    responses(
        (status = 200, description = "The task", body = TaskResponse),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No task with that id, or no access")
    )
)]
pub async fn get_task(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_task_access(db, &user, id, Role::Read).await?;
    let task = Task::get_by_id(db, &id).await?;
    let mut responses = hydrate_tasks(db, vec![task]).await?;
    Ok(Json(responses.swap_remove(0)))
}

/// Update a task. Provided fields are changed; `assignee_ids`/`label_ids`
/// replace the whole set when given.
#[utoipa::path(
    patch,
    path = "/tasks/{id}",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    request_body = UpdateTaskRequest,
    responses(
        (status = 200, description = "Task updated", body = TaskResponse),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No task with that id, or no access")
    )
)]
pub async fn update_task(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdateTaskRequest>,
) -> Result<Json<TaskResponse>, ApiError> {
    let db = &mut *state.db.lock().await;
    let workspace_id = access::require_task_access(db, &user, id, Role::Write).await?;
    if let Some(title) = &input.title
        && title.trim().is_empty()
    {
        return Err(ApiError::bad_request("title cannot be empty"));
    }
    if let Some(assignee_ids) = &input.assignee_ids {
        validate_assignees(db, assignee_ids, workspace_id).await?;
    }
    if let Some(label_ids) = &input.label_ids {
        validate_labels(db, label_ids, workspace_id).await?;
    }

    if let Some(target) = input.list_id {
        // Moving needs write access to the target, which must share the
        // task's workspace (a 404 for lists the caller cannot see).
        let target_workspace = access::require_list_access(db, &user, target, Role::Write).await?;
        if target_workspace != workspace_id {
            return Err(ApiError::bad_request("list is in another workspace"));
        }
    }

    let mut tx = db.transaction().await?;
    let mut task = Task::get_by_id(&mut tx, &id).await?;

    let was_completed = task.is_completed;
    let mut update = task.update();
    let has_field_changes = input.title.is_some()
        || input.description.is_some()
        || input.is_completed.is_some()
        || input.priority.is_some()
        || input.due_date.is_some()
        || input.position.is_some()
        || input.start_date.is_some()
        || input.list_id.is_some();
    if let Some(title) = input.title {
        update = update.title(title.trim().to_string());
    }
    if let Some(description) = input.description {
        update = update.description(description);
    }
    if let Some(is_completed) = input.is_completed {
        update = update.is_completed(is_completed);
        // Keep the timestamp in step with the flag: set on completion, cleared on reopening.
        if is_completed != was_completed {
            update = update.completed_at(is_completed.then(auth::now_secs));
        }
    }
    if let Some(priority) = input.priority {
        update = update.priority(priority);
    }
    if let Some(due_date) = input.due_date {
        update = update.due_date(due_date);
    }
    if let Some(position) = input.position {
        update = update.position(position);
    }
    if let Some(start_date) = input.start_date {
        update = update.start_date(start_date);
    }
    if let Some(target) = input.list_id {
        update = update.list_id(target);
        if input.position.is_none() {
            let last = Task::filter_by_list_id(target)
                .order_by(Task::fields().position().desc())
                .first()
                .exec(&mut tx)
                .await?;
            update = update.position(last.map_or(1.0, |t| t.position + 1.0));
        }
    }
    // Toasty rejects an update with no assignments (e.g. assignee-only PATCH).
    if has_field_changes {
        update.exec(&mut tx).await?;
    }
    drop(task);

    if let Some(assignee_ids) = input.assignee_ids {
        TaskAssignee::filter_by_task_id(id)
            .delete()
            .exec(&mut tx)
            .await?;
        for user_id in assignee_ids {
            toasty::create!(TaskAssignee {
                task_id: id,
                user_id,
            })
            .exec(&mut tx)
            .await?;
        }
    }
    if let Some(label_ids) = input.label_ids {
        TaskLabel::filter_by_task_id(id)
            .delete()
            .exec(&mut tx)
            .await?;
        for label_id in label_ids {
            toasty::create!(TaskLabel {
                task_id: id,
                label_id,
            })
            .exec(&mut tx)
            .await?;
        }
    }
    tx.commit().await?;

    let task = Task::get_by_id(db, &id).await?;
    let mut responses = hydrate_tasks(db, vec![task]).await?;
    Ok(Json(responses.swap_remove(0)))
}

/// Delete a task.
#[utoipa::path(
    delete,
    path = "/tasks/{id}",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    responses(
        (status = 204, description = "Task deleted"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No task with that id, or no access")
    )
)]
pub async fn delete_task(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_task_access(db, &user, id, Role::Write).await?;

    let mut tx = db.transaction().await?;
    delete_tasks_by_ids(&mut tx, vec![id]).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Assignees must be workspace members (they must be able to see the task).
async fn validate_assignees(
    db: &mut toasty::Db,
    assignee_ids: &[uuid::Uuid],
    workspace_id: uuid::Uuid,
) -> Result<(), ApiError> {
    for user_id in assignee_ids {
        if !access::is_workspace_member(db, *user_id, workspace_id).await? {
            return Err(ApiError::bad_request(
                "assignees must be members of the workspace",
            ));
        }
    }
    Ok(())
}

/// Labels must belong to the same workspace. Returns the label rows.
async fn validate_labels(
    db: &mut toasty::Db,
    label_ids: &[uuid::Uuid],
    workspace_id: uuid::Uuid,
) -> Result<Vec<Label>, ApiError> {
    if label_ids.is_empty() {
        return Ok(vec![]);
    }
    let labels = Label::all()
        .filter(in_list(Label::fields().id(), label_ids.to_vec()))
        .exec(db)
        .await?;
    if labels.len() != label_ids.len() || labels.iter().any(|l| l.workspace_id != workspace_id) {
        return Err(ApiError::bad_request(
            "labels must belong to the same workspace",
        ));
    }
    Ok(labels)
}

/// Bulk-load assignees and labels for a set of tasks (no per-task queries).
async fn hydrate_tasks(
    db: &mut toasty::Db,
    tasks: Vec<Task>,
) -> Result<Vec<TaskResponse>, ApiError> {
    if tasks.is_empty() {
        return Ok(vec![]);
    }

    let assignee_rows = TaskAssignee::all()
        .filter(in_list(
            TaskAssignee::fields().task_id(),
            tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
        ))
        .exec(db)
        .await?;
    let label_rows = TaskLabel::all()
        .filter(in_list(
            TaskLabel::fields().task_id(),
            tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
        ))
        .exec(db)
        .await?;

    let list_ids: HashSet<uuid::Uuid> = tasks.iter().map(|t| t.list_id).collect();
    let project_by_list: HashMap<uuid::Uuid, uuid::Uuid> = List::all()
        .filter(in_list(
            List::fields().id(),
            list_ids.into_iter().collect::<Vec<_>>(),
        ))
        .exec(db)
        .await?
        .into_iter()
        .map(|l| (l.id, l.project_id))
        .collect();

    let user_ids: HashSet<uuid::Uuid> = assignee_rows.iter().map(|a| a.user_id).collect();
    let users: Vec<User> = if user_ids.is_empty() {
        vec![]
    } else {
        User::all()
            .filter(in_list(
                User::fields().id(),
                user_ids.into_iter().collect::<Vec<_>>(),
            ))
            .exec(db)
            .await?
    };
    let label_ids: HashSet<uuid::Uuid> = label_rows.iter().map(|l| l.label_id).collect();
    let labels: Vec<Label> = if label_ids.is_empty() {
        vec![]
    } else {
        Label::all()
            .filter(in_list(
                Label::fields().id(),
                label_ids.into_iter().collect::<Vec<_>>(),
            ))
            .exec(db)
            .await?
    };

    let users_by_id: HashMap<uuid::Uuid, PublicUser> =
        users.iter().map(|u| (u.id, PublicUser::from(u))).collect();
    let labels_by_id: HashMap<uuid::Uuid, Label> = labels.into_iter().map(|l| (l.id, l)).collect();

    let mut assignees_by_task: HashMap<uuid::Uuid, Vec<PublicUser>> = HashMap::new();
    for row in assignee_rows {
        if let Some(user) = users_by_id.get(&row.user_id) {
            assignees_by_task
                .entry(row.task_id)
                .or_default()
                .push(user.clone());
        }
    }
    let mut labels_by_task: HashMap<uuid::Uuid, Vec<Label>> = HashMap::new();
    for row in label_rows {
        if let Some(label) = labels_by_id.get(&row.label_id) {
            labels_by_task
                .entry(row.task_id)
                .or_default()
                .push(label.clone());
        }
    }

    Ok(tasks
        .into_iter()
        .map(|task| {
            let assignees = assignees_by_task.remove(&task.id).unwrap_or_default();
            let task_labels = labels_by_task.remove(&task.id).unwrap_or_default();
            TaskResponse {
                id: task.id,
                list_id: task.list_id,
                project_id: project_by_list
                    .get(&task.list_id)
                    .copied()
                    .unwrap_or_default(),
                title: task.title,
                description: task.description,
                is_completed: task.is_completed,
                completed_at: task.completed_at,
                position: task.position,
                priority: task.priority,
                start_date: task.start_date,
                due_date: task.due_date,
                assignees,
                labels: task_labels,
            }
        })
        .collect())
}
