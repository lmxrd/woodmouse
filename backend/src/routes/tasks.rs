use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::activity::{self, Event};
use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::models::{
    Comment, FileRef, Label, List, Priority, Project, Role, Task, TaskAssignee, TaskLabel, User,
};
use crate::revisions;
use crate::routes::auth::PublicUser;
use crate::state::AppState;
use crate::trash;

/// All task routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_tasks))
        .routes(routes!(create_task))
        .routes(routes!(get_task))
        .routes(routes!(update_task))
        .routes(routes!(delete_task))
}

/// A task with its assignees and labels hydrated.
#[derive(Serialize, ToSchema)]
pub struct TaskResponse {
    pub id: uuid::Uuid,
    pub list_id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    /// The parent task, when this is a subtask.
    pub parent_id: Option<uuid::Uuid>,
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
    /// All subtasks below this task, at any depth.
    pub subtask_total: usize,
    /// How many of them are completed.
    pub subtask_completed: usize,
    /// Live comments on the task.
    pub comment_count: usize,
    /// Files attached to the task.
    pub attachment_count: usize,
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
    /// Make this a subtask of another task (it then moves to that task's list); `null` makes it
    /// a top-level task again. A task cannot go below itself or one of its own subtasks.
    #[serde(default, deserialize_with = "double_option")]
    pub parent_id: Option<Option<uuid::Uuid>>,
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
    /// Only the direct subtasks of this task.
    pub parent_id: Option<uuid::Uuid>,
    /// Include subtasks (any depth) among the results; by default only top-level tasks are listed.
    pub include_subtasks: Option<bool>,
    /// Only completed (true) or open (false) tasks.
    pub is_completed: Option<bool>,
    /// Only tasks assigned to this user.
    pub assignee_id: Option<uuid::Uuid>,
    /// Only tasks carrying this label.
    pub label_id: Option<uuid::Uuid>,
    /// Only tasks scheduled on or after this day (Unix epoch seconds). A task is scheduled from its
    /// start date (or due date, if it has no start) to its due date (or start date).
    pub from: Option<i64>,
    /// Only tasks scheduled on or before this day (Unix epoch seconds).
    pub to: Option<i64>,
    /// Only tasks with neither a start nor a due date.
    pub unscheduled: Option<bool>,
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
            || list.deleted_at.is_some()
            || project.deleted_at.is_some()
            || filters.project_id.is_some_and(|p| p != project.id)
        {
            return Err(ApiError::not_found());
        }
        vec![list_id]
    } else {
        let project_ids: Vec<uuid::Uuid> = if let Some(project_id) = filters.project_id {
            let project = Project::get_by_id(db, &project_id).await?;
            if project.workspace_id != workspace_id || project.deleted_at.is_some() {
                return Err(ApiError::not_found());
            }
            vec![project_id]
        } else {
            Project::filter_by_workspace_id(workspace_id)
                .exec(db)
                .await?
                .into_iter()
                .filter(|p| p.deleted_at.is_none())
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
            .filter(|l| l.deleted_at.is_none())
            .map(|l| l.id)
            .collect()
    };
    if list_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let mut query = Task::all()
        .filter(in_list(Task::fields().list_id(), list_ids))
        .filter(Task::fields().deleted_at().is_none());
    if let Some(parent_id) = filters.parent_id {
        query = query.filter(Task::fields().parent_id().eq(parent_id));
    } else if filters.include_subtasks != Some(true) {
        query = query.filter(Task::fields().parent_id().is_none());
    }
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
    let mut tasks = query.order_by(order_by).exec(db).await?;
    if filters.unscheduled == Some(true) {
        tasks.retain(|t| t.start_date.is_none() && t.due_date.is_none());
    } else if filters.from.is_some() || filters.to.is_some() {
        tasks.retain(|t| {
            let (Some(first), Some(last)) =
                (t.start_date.or(t.due_date), t.due_date.or(t.start_date))
            else {
                return false;
            };
            filters.from.is_none_or(|from| last >= from) && filters.to.is_none_or(|to| first <= to)
        });
    }

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
    let db = &mut *state.db.lock().await;
    let workspace_id = access::require_list_access(db, &user, list_id, Role::Write).await?;
    let response = create_task_in(db, workspace_id, list_id, None, user.id, input).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// Create a task (or, with `parent_id`, a subtask) at the end of the list. Access to the list must
/// already be checked.
pub(crate) async fn create_task_in(
    db: &mut toasty::Db,
    workspace_id: uuid::Uuid,
    list_id: uuid::Uuid,
    parent_id: Option<uuid::Uuid>,
    actor: uuid::Uuid,
    input: CreateTaskRequest,
) -> Result<TaskResponse, ApiError> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(ApiError::bad_request("title is required"));
    }
    validate_assignees(db, &input.assignee_ids, workspace_id).await?;
    validate_labels(db, &input.label_ids, workspace_id).await?;

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
        parent_id,
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

    let mut responses = hydrate_tasks(db, vec![task]).await?;
    let created = responses.swap_remove(0);

    let parent_title = match parent_id {
        Some(pid) => Some(Task::get_by_id(db, &pid).await?.title),
        None => None,
    };
    activity::record(
        db,
        Event {
            workspace_id,
            project_id: Some(created.project_id),
            // A new subtask shows up in its parent's feed, where people look for it.
            task_id: Some(parent_id.unwrap_or(created.id)),
            actor_id: actor,
            kind: if parent_title.is_some() {
                "subtask_added"
            } else {
                "task_created"
            },
            data: serde_json::json!({ "title": created.title, "parent_title": parent_title }),
        },
    )
    .await;
    Ok(created)
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

    // Where the task ends up: under a new parent (in the parent's list) or in the requested list.
    let current = Task::get_by_id(db, &id).await?;
    let mut new_list = input.list_id.filter(|l| *l != current.list_id);
    if let Some(Some(parent_id)) = input.parent_id {
        let parent = Task::get_by_id(db, &parent_id).await?;
        if parent.deleted_at.is_some()
            || access::task_workspace(db, parent_id).await? != workspace_id
        {
            return Err(ApiError::not_found());
        }
        let in_current_list = Task::filter_by_list_id(current.list_id).exec(db).await?;
        if parent_id == id || trash::subtree(&in_current_list, id).contains(&parent_id) {
            return Err(ApiError::bad_request("a task cannot go below itself"));
        }
        new_list = Some(parent.list_id).filter(|l| *l != current.list_id);
    } else if current.parent_id.is_some() && new_list.is_some() && input.parent_id.is_none() {
        return Err(ApiError::bad_request(
            "a subtask stays in its parent's list; move the parent instead",
        ));
    }

    // Snapshot what the activity feed compares against after the update.
    let old_assignees: Option<std::collections::HashSet<uuid::Uuid>> =
        if input.assignee_ids.is_some() {
            Some(
                TaskAssignee::filter_by_task_id(id)
                    .exec(db)
                    .await?
                    .into_iter()
                    .map(|a| a.user_id)
                    .collect(),
            )
        } else {
            None
        };
    let new_assignees = input.assignee_ids.clone();

    // Keep the old title and description as a version when this edit starts a new editing session.
    let text_changed = input
        .title
        .as_ref()
        .is_some_and(|t| t.trim() != current.title)
        || input
            .description
            .as_ref()
            .is_some_and(|d| *d != current.description);
    if text_changed {
        revisions::snapshot_task(db, &current, user.id, false).await?;
    }

    let mut tx = db.transaction().await?;
    let mut task = Task::get_by_id(&mut tx, &id).await?;

    let was_completed = task.is_completed;
    let old_list = task.list_id;
    let mut update = task.update();
    let has_field_changes = input.title.is_some()
        || input.description.is_some()
        || input.is_completed.is_some()
        || input.priority.is_some()
        || input.due_date.is_some()
        || input.position.is_some()
        || input.start_date.is_some()
        || new_list.is_some()
        || input.parent_id.is_some();
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
    if text_changed {
        update = update
            .text_edited_at(Some(auth::now_secs()))
            .text_edited_by(Some(user.id));
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
    if let Some(parent_id) = input.parent_id {
        update = update.parent_id(parent_id);
    }
    if let Some(target) = new_list {
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

    // Subtasks follow their parent to the new list.
    if let Some(target) = new_list {
        let old_list_tasks = Task::filter_by_list_id(old_list).exec(&mut tx).await?;
        let below: Vec<uuid::Uuid> = trash::subtree(&old_list_tasks, id)
            .into_iter()
            .filter(|t| *t != id)
            .collect();
        if !below.is_empty() {
            Task::all()
                .filter(in_list(Task::fields().id(), below))
                .update()
                .list_id(target)
                .exec(&mut tx)
                .await?;
        }
    }

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
    record_changes(
        db,
        workspace_id,
        user.id,
        &current,
        &task,
        old_assignees,
        new_assignees,
    )
    .await?;
    let mut responses = hydrate_tasks(db, vec![task]).await?;
    Ok(Json(responses.swap_remove(0)))
}

/// Write an activity entry for each thing that changed between `before` and `after`.
async fn record_changes(
    db: &mut toasty::Db,
    workspace_id: uuid::Uuid,
    actor: uuid::Uuid,
    before: &Task,
    after: &Task,
    old_assignees: Option<HashSet<uuid::Uuid>>,
    new_assignees: Option<Vec<uuid::Uuid>>,
) -> Result<(), ApiError> {
    let project_id = List::get_by_id(db, &after.list_id).await?.project_id;
    let mut events: Vec<(&'static str, serde_json::Value)> = vec![];
    if before.title != after.title {
        events.push((
            "task_renamed",
            serde_json::json!({ "from": before.title, "to": after.title }),
        ));
    }
    if before.is_completed != after.is_completed {
        let kind = if after.is_completed {
            "task_completed"
        } else {
            "task_reopened"
        };
        events.push((kind, serde_json::json!({ "title": after.title })));
    }
    if before.list_id != after.list_id {
        let from = List::get_by_id(db, &before.list_id).await?.name;
        let to = List::get_by_id(db, &after.list_id).await?.name;
        events.push((
            "task_moved",
            serde_json::json!({ "title": after.title, "from": from, "to": to }),
        ));
    }
    if before.due_date != after.due_date {
        events.push((
            "due_changed",
            serde_json::json!({ "title": after.title, "from": before.due_date, "to": after.due_date }),
        ));
    }
    if before.priority != after.priority {
        events.push((
            "priority_changed",
            serde_json::json!({ "title": after.title, "from": before.priority, "to": after.priority }),
        ));
    }
    if let (Some(old), Some(new)) = (old_assignees, new_assignees) {
        let new: HashSet<uuid::Uuid> = new.into_iter().collect();
        for (kind, ids) in [
            (
                "assigned",
                new.difference(&old).copied().collect::<Vec<_>>(),
            ),
            (
                "unassigned",
                old.difference(&new).copied().collect::<Vec<_>>(),
            ),
        ] {
            if ids.is_empty() {
                continue;
            }
            let names: Vec<String> = User::all()
                .filter(in_list(User::fields().id(), ids))
                .exec(db)
                .await?
                .into_iter()
                .map(|u| if u.name.is_empty() { u.email } else { u.name })
                .collect();
            events.push((
                kind,
                serde_json::json!({ "title": after.title, "who": names }),
            ));
        }
    }
    for (kind, data) in events {
        activity::record(
            db,
            Event {
                workspace_id,
                project_id: Some(project_id),
                task_id: Some(after.id),
                actor_id: actor,
                kind,
                data,
            },
        )
        .await;
    }
    Ok(())
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
    let workspace_id = access::require_task_access(db, &user, id, Role::Write).await?;

    let mut tx = db.transaction().await?;
    let task = Task::get_by_id(&mut tx, &id).await?;
    trash::delete_task(&mut tx, &task, user.id, auth::now_secs()).await?;
    tx.commit().await?;

    let project_id = List::get_by_id(db, &task.list_id).await?.project_id;
    activity::record(
        db,
        Event {
            workspace_id,
            project_id: Some(project_id),
            task_id: Some(id),
            actor_id: user.id,
            kind: "task_deleted",
            data: serde_json::json!({ "title": task.title }),
        },
    )
    .await;
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
    if labels.len() != label_ids.len()
        || labels
            .iter()
            .any(|l| l.workspace_id != workspace_id || l.deleted_at.is_some())
    {
        return Err(ApiError::bad_request(
            "labels must belong to the same workspace",
        ));
    }
    Ok(labels)
}

/// Bulk-load assignees and labels for a set of tasks (no per-task queries).
pub(crate) async fn hydrate_tasks(
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
            list_ids.iter().copied().collect::<Vec<_>>(),
        ))
        .exec(db)
        .await?
        .into_iter()
        .map(|l| (l.id, l.project_id))
        .collect();

    // Every live task in those lists, to count each task's subtasks (any depth).
    let live_in_lists: Vec<Task> = Task::all()
        .filter(in_list(
            Task::fields().list_id(),
            list_ids.iter().copied().collect::<Vec<_>>(),
        ))
        .filter(Task::fields().deleted_at().is_none())
        .exec(db)
        .await?;
    let completed_by_id: HashMap<uuid::Uuid, bool> = live_in_lists
        .iter()
        .map(|t| (t.id, t.is_completed))
        .collect();

    let task_ids: Vec<uuid::Uuid> = tasks.iter().map(|t| t.id).collect();
    let comment_rows = Comment::all()
        .filter(in_list(Comment::fields().task_id(), task_ids.clone()))
        .exec(db)
        .await?;
    let file_rows = FileRef::all()
        .filter(in_list(FileRef::fields().task_id(), task_ids))
        .exec(db)
        .await?;

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
    let labels_by_id: HashMap<uuid::Uuid, Label> = labels
        .into_iter()
        .filter(|l| l.deleted_at.is_none())
        .map(|l| (l.id, l))
        .collect();

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
            let below: Vec<uuid::Uuid> = trash::subtree(&live_in_lists, task.id)
                .into_iter()
                .filter(|id| *id != task.id)
                .collect();
            let subtask_completed = below
                .iter()
                .filter(|id| completed_by_id.get(id).copied().unwrap_or(false))
                .count();
            TaskResponse {
                id: task.id,
                list_id: task.list_id,
                project_id: project_by_list
                    .get(&task.list_id)
                    .copied()
                    .unwrap_or_default(),
                parent_id: task.parent_id,
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
                subtask_total: below.len(),
                subtask_completed,
                comment_count: comment_rows
                    .iter()
                    .filter(|c| c.task_id == task.id && c.deleted_at.is_none())
                    .count(),
                attachment_count: file_rows.iter().filter(|r| r.task_id == task.id).count(),
            }
        })
        .collect())
}
