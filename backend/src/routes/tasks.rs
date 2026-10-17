use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use jiff::civil::DateTime;
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{
    Comment, FileRef, Label, List, Priority, Project, Role, Task, TaskAssignee, TaskLabel,
};
use crate::revisions::{self, TaskContent};
use crate::routes::auth::{People, PublicUser};
use crate::state::AppState;
use crate::{ordering, time, trash};

/// All task routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_tasks))
        .routes(routes!(create_task))
        .routes(routes!(create_inbox_task))
        .routes(routes!(get_task))
        .routes(routes!(update_task))
        .routes(routes!(delete_task))
}

/// A task with its assignees and labels hydrated. Dates are UTC (`2026-10-16T08:30:00Z`); `start_date`
/// and `due_date` are a calendar day at `00:00:00Z`.
#[derive(Serialize, ToSchema)]
pub struct TaskResponse {
    pub id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    /// The list the task is in; absent while it is in the project's inbox.
    pub list_id: Option<uuid::Uuid>,
    /// The parent task, when this is a subtask.
    pub parent_id: Option<uuid::Uuid>,
    pub title: String,
    pub description: String,
    /// When the task was completed; absent while it is open. Set by the server (send `is_completed` to
    /// close or reopen a task).
    #[serde(with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub completed_at: Option<DateTime>,
    /// Ordering among its siblings (integers 20 apart).
    pub position: i64,
    pub priority: Priority,
    #[serde(with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub start_date: Option<DateTime>,
    #[serde(with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub due_date: Option<DateTime>,
    /// The sprint it is planned in, if any.
    pub sprint_id: Option<uuid::Uuid>,
    /// Expected work, in minutes.
    pub estimate_minutes: Option<i64>,
    /// Work done so far, in minutes.
    pub spent_minutes: Option<i64>,
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
    #[serde(default, with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub start_date: Option<DateTime>,
    #[serde(default, with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub due_date: Option<DateTime>,
    /// The sprint to plan it in (one of the project's open sprints).
    pub sprint_id: Option<uuid::Uuid>,
    /// Expected work, in minutes.
    pub estimate_minutes: Option<i64>,
    /// Work done so far, in minutes.
    pub spent_minutes: Option<i64>,
    #[serde(default)]
    pub assignee_ids: Vec<uuid::Uuid>,
    #[serde(default)]
    pub label_ids: Vec<uuid::Uuid>,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    /// Close (`true`) or reopen (`false`) the task; the server sets `completed_at`.
    pub is_completed: Option<bool>,
    pub priority: Option<Priority>,
    /// Absent leaves the due date unchanged; `null` clears it. Only the day counts.
    #[serde(default, deserialize_with = "crate::time::utc_patch::deserialize")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub due_date: Option<Option<DateTime>>,
    /// Absent leaves the start date unchanged; `null` clears it. Only the day counts.
    #[serde(default, deserialize_with = "crate::time::utc_patch::deserialize")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub start_date: Option<Option<DateTime>>,
    /// Plan it in one of the project's open sprints; `null` takes it out of its sprint (the backlog).
    #[serde(default, deserialize_with = "crate::api::double_option")]
    pub sprint_id: Option<Option<uuid::Uuid>>,
    /// Expected work in minutes. Absent leaves it unchanged; `null` clears it.
    #[serde(default, deserialize_with = "crate::api::double_option")]
    pub estimate_minutes: Option<Option<i64>>,
    /// Work done so far in minutes. Absent leaves it unchanged; `null` clears it.
    #[serde(default, deserialize_with = "crate::api::double_option")]
    pub spent_minutes: Option<Option<i64>>,
    /// Place the task right after this sibling (`null`: first). Absent keeps its place, unless it moves
    /// to another list or parent, where it goes last.
    #[serde(default, deserialize_with = "crate::api::double_option")]
    pub after_id: Option<Option<uuid::Uuid>>,
    /// Move the task to another list of its project; `null` moves it to the project's inbox.
    #[serde(default, deserialize_with = "crate::api::double_option")]
    pub list_id: Option<Option<uuid::Uuid>>,
    /// Make this a subtask of another task (it then moves to that task's list); `null` makes it
    /// a top-level task again. A task cannot go below itself or one of its own subtasks.
    #[serde(default, deserialize_with = "crate::api::double_option")]
    pub parent_id: Option<Option<uuid::Uuid>>,
    /// When provided, replaces the assignee set.
    pub assignee_ids: Option<Vec<uuid::Uuid>>,
    /// When provided, replaces the label set.
    pub label_ids: Option<Vec<uuid::Uuid>>,
}

/// A task can only be planned in an open (live, not completed) sprint of its own project.
pub(crate) async fn validate_sprint(
    db: &mut dyn toasty::Executor,
    project_id: uuid::Uuid,
    sprint_id: uuid::Uuid,
) -> Result<(), ApiError> {
    let sprint = crate::models::Sprint::get_by_id(db, &sprint_id)
        .await
        .map_err(|_| ApiError::bad_request("no such sprint"))?;
    if sprint.project_id != project_id || sprint.deleted_at.is_some() {
        return Err(ApiError::bad_request("no such sprint"));
    }
    if sprint.completed_at.is_some() {
        return Err(ApiError::conflict("that sprint is completed"));
    }
    Ok(())
}

/// Work time is a whole number of minutes, not negative and not absurdly large (about 16 000 hours).
fn validate_minutes(minutes: Option<i64>) -> Result<(), ApiError> {
    if minutes.is_some_and(|m| !(0..=1_000_000).contains(&m)) {
        return Err(ApiError::bad_request(
            "work time must be between 0 and 1 000 000 minutes",
        ));
    }
    Ok(())
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TaskFilters {
    /// Only tasks in this project.
    pub project_id: Option<uuid::Uuid>,
    /// Only tasks in this list.
    pub list_id: Option<uuid::Uuid>,
    /// Only tasks in a project's inbox (with no list).
    pub inbox: Option<bool>,
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
    /// Only tasks planned in this sprint.
    pub sprint_id: Option<uuid::Uuid>,
    /// Only tasks in no sprint (the backlog).
    pub no_sprint: Option<bool>,
    /// Only tasks scheduled on or after this day (an RFC 3339 date-time; only the day counts). A task is
    /// scheduled from its start date (or due date, if it has no start) to its due date (or start date).
    #[serde(default, with = "time::utc_option")]
    #[param(value_type = Option<String>, format = DateTime)]
    pub from: Option<DateTime>,
    /// Only tasks scheduled on or before this day.
    #[serde(default, with = "time::utc_option")]
    #[param(value_type = Option<String>, format = DateTime)]
    pub to: Option<DateTime>,
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
    let db = &mut state.db();
    access::require_access(db, &user, workspace_id, Role::Read).await?;

    // The projects in scope. A project or list filter must belong to this workspace, otherwise it looks
    // nonexistent.
    let mut list_filter: Option<uuid::Uuid> = None;
    let project_ids: Vec<uuid::Uuid> = if let Some(list_id) = filters.list_id {
        let list = List::get_by_id(db, &list_id).await?;
        let project = Project::get_by_id(db, &list.project_id).await?;
        if project.workspace_id != workspace_id
            || list.deleted_at.is_some()
            || project.deleted_at.is_some()
            || filters.project_id.is_some_and(|p| p != project.id)
        {
            return Err(ApiError::not_found());
        }
        list_filter = Some(list_id);
        vec![project.id]
    } else if let Some(project_id) = filters.project_id {
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

    let mut query = Task::all()
        .filter(in_list(Task::fields().project_id(), project_ids))
        .filter(Task::fields().deleted_at().is_none());
    if let Some(list_id) = list_filter {
        query = query.filter(Task::fields().list_id().eq(list_id));
    } else if filters.inbox == Some(true) {
        query = query.filter(Task::fields().list_id().is_none());
    }
    if let Some(sprint_id) = filters.sprint_id {
        query = query.filter(Task::fields().sprint_id().eq(sprint_id));
    } else if filters.no_sprint == Some(true) {
        query = query.filter(Task::fields().sprint_id().is_none());
    }
    if let Some(parent_id) = filters.parent_id {
        query = query.filter(Task::fields().parent_id().eq(parent_id));
    } else if filters.include_subtasks != Some(true) {
        query = query.filter(Task::fields().parent_id().is_none());
    }
    match filters.is_completed {
        Some(true) => query = query.filter(Task::fields().completed_at().is_some()),
        Some(false) => query = query.filter(Task::fields().completed_at().is_none()),
        None => {}
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
        let (from, to) = (filters.from.map(time::to_day), filters.to.map(time::to_day));
        tasks.retain(|t| {
            let (Some(first), Some(last)) =
                (t.start_date.or(t.due_date), t.due_date.or(t.start_date))
            else {
                return false;
            };
            from.is_none_or(|from| last >= from) && to.is_none_or(|to| first <= to)
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
    let mut db = state.writer().await;
    let (list, workspace_id) = access::require_list(&mut *db, &user, list_id, Role::Write).await?;
    let response = create_task_in(
        &mut db,
        workspace_id,
        list.project_id,
        Some(list_id),
        None,
        user.id,
        input,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// Create a task in a project's inbox (a task with no list yet).
#[utoipa::path(
    post,
    path = "/projects/{project_id}/tasks",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("project_id" = uuid::Uuid, Path, description = "Project id")),
    request_body = CreateTaskRequest,
    responses(
        (status = 201, description = "Task created", body = TaskResponse),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or no access")
    )
)]
pub async fn create_inbox_task(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<uuid::Uuid>,
    Json(input): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), ApiError> {
    let mut db = state.writer().await;
    let workspace_id =
        access::require_project_access(&mut *db, &user, project_id, Role::Write).await?;
    let response = create_task_in(
        &mut db,
        workspace_id,
        project_id,
        None,
        None,
        user.id,
        input,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// The tasks that are siblings of a new or moved task: same project, list (or inbox) and parent, live,
/// in order, as (id, position).
async fn siblings(
    db: &mut dyn toasty::Executor,
    project_id: uuid::Uuid,
    list_id: Option<uuid::Uuid>,
    parent_id: Option<uuid::Uuid>,
    except: Option<uuid::Uuid>,
) -> Result<Vec<(uuid::Uuid, i64)>, ApiError> {
    let mut found: Vec<(uuid::Uuid, i64)> = Task::filter_by_project_id(project_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|t| {
            t.deleted_at.is_none()
                && t.list_id == list_id
                && t.parent_id == parent_id
                && Some(t.id) != except
        })
        .map(|t| (t.id, t.position))
        .collect();
    found.sort_by_key(|(id, position)| (*position, *id));
    Ok(found)
}

/// Create a task (or, with `parent_id`, a subtask) at the end of its siblings, with its first revision
/// and feed entry, in one transaction. Access to the list or project must already be checked. A subtask
/// lives with its parent: `list_id` is then the parent's.
pub(crate) async fn create_task_tx(
    tx: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    list_id: Option<uuid::Uuid>,
    parent_id: Option<uuid::Uuid>,
    actor: uuid::Uuid,
    input: CreateTaskRequest,
) -> Result<Task, ApiError> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(ApiError::bad_request("title is required"));
    }
    validate_assignees(tx, &input.assignee_ids, workspace_id).await?;
    validate_labels(tx, &input.label_ids, workspace_id).await?;
    validate_minutes(input.estimate_minutes)?;
    validate_minutes(input.spent_minutes)?;
    if let Some(sprint_id) = input.sprint_id {
        validate_sprint(tx, project_id, sprint_id).await?;
    }
    let (start_date, due_date) = (
        input.start_date.map(time::to_day),
        input.due_date.map(time::to_day),
    );

    let position = ordering::append(
        siblings(tx, project_id, list_id, parent_id, None)
            .await?
            .iter()
            .map(|(_, p)| *p),
    );
    let task = toasty::create!(Task {
        project_id,
        list_id,
        title: title.clone(),
        description: input.description.clone(),
        position,
        priority: input.priority.unwrap_or(Priority::Medium),
        start_date,
        due_date,
        sprint_id: input.sprint_id,
        estimate_minutes: input.estimate_minutes,
        spent_minutes: input.spent_minutes,
        parent_id,
    })
    .exec(tx)
    .await?;

    for user_id in &input.assignee_ids {
        toasty::create!(TaskAssignee {
            task_id: task.id,
            user_id: *user_id,
        })
        .exec(tx)
        .await?;
    }
    crate::notify::notify(
        tx,
        input.assignee_ids.clone(),
        crate::notify::Notice {
            workspace_id,
            kind: "task_assigned",
            target_type: crate::notify::TASK,
            target_id: task.id,
            actor: Some(actor),
            data: serde_json::json!({ "title": title, "project_id": project_id }),
        },
    )
    .await?;
    for label_id in &input.label_ids {
        toasty::create!(TaskLabel {
            task_id: task.id,
            label_id: *label_id,
        })
        .exec(tx)
        .await?;
    }
    revisions::record_task(
        tx,
        task.id,
        actor,
        &TaskContent {
            title: title.clone(),
            description: input.description,
        },
    )
    .await?;
    let parent_title = match parent_id {
        Some(pid) => Some(Task::get_by_id(tx, &pid).await?.title),
        None => None,
    };
    // A new subtask shows up in its parent's feed, where people look for it.
    audit::task_event(
        tx,
        workspace_id,
        project_id,
        actor,
        if parent_title.is_some() {
            "subtask_added"
        } else {
            "task_created"
        },
        parent_id.unwrap_or(task.id),
        serde_json::json!({ "title": title, "parent_title": parent_title, "task_id": task.id }),
    )
    .await?;
    Ok(task)
}

/// Create a task in its own transaction and return it as the API shows it.
pub(crate) async fn create_task_in(
    db: &mut toasty::Db,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    list_id: Option<uuid::Uuid>,
    parent_id: Option<uuid::Uuid>,
    actor: uuid::Uuid,
    input: CreateTaskRequest,
) -> Result<TaskResponse, ApiError> {
    let mut tx = db.transaction().await?;
    let task = create_task_tx(
        &mut tx,
        workspace_id,
        project_id,
        list_id,
        parent_id,
        actor,
        input,
    )
    .await?;
    tx.commit().await?;
    let mut responses = hydrate_tasks(db, vec![task]).await?;
    Ok(responses.swap_remove(0))
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
    let db = &mut state.db();
    let (task, _) = access::require_task(db, &user, id, Role::Read).await?;
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
    let mut db = state.writer().await;
    let (current, workspace_id) = access::require_task(&mut *db, &user, id, Role::Write).await?;
    if let Some(title) = &input.title
        && title.trim().is_empty()
    {
        return Err(ApiError::bad_request("title cannot be empty"));
    }
    if let Some(assignee_ids) = &input.assignee_ids {
        validate_assignees(&mut *db, assignee_ids, workspace_id).await?;
    }
    if let Some(label_ids) = &input.label_ids {
        validate_labels(&mut *db, label_ids, workspace_id).await?;
    }

    // Where the task ends up: under a new parent (in the parent's list), in the requested list, or the
    // inbox. A subtask always lives with its parent.
    let mut new_list: Option<Option<uuid::Uuid>> = None;
    if let Some(target) = input.list_id {
        if let Some(list_id) = target {
            let (list, _) = access::require_list(&mut *db, &user, list_id, Role::Write).await?;
            if list.project_id != current.project_id {
                return Err(ApiError::bad_request("list is in another project"));
            }
        }
        if target != current.list_id {
            new_list = Some(target);
        }
    }
    let project_tasks = Task::filter_by_project_id(current.project_id)
        .exec(&mut *db)
        .await?;
    let mut new_parent: Option<Option<uuid::Uuid>> = None;
    if let Some(parent) = input.parent_id {
        if let Some(parent_id) = parent {
            let parent_task = project_tasks
                .iter()
                .find(|t| t.id == parent_id && t.deleted_at.is_none())
                .ok_or_else(ApiError::not_found)?;
            if parent_id == id || trash::subtree(&project_tasks, id).contains(&parent_id) {
                return Err(ApiError::bad_request("a task cannot go below itself"));
            }
            if parent_task.list_id != current.list_id {
                new_list = Some(parent_task.list_id);
            }
        }
        if parent != current.parent_id {
            new_parent = Some(parent);
        }
    } else if current.parent_id.is_some() && new_list.is_some() {
        return Err(ApiError::bad_request(
            "a subtask stays in its parent's list; move the parent instead",
        ));
    }

    // Snapshot what the activity feed compares against after the update.
    let old_assignees: Option<HashSet<uuid::Uuid>> = if input.assignee_ids.is_some() {
        Some(
            TaskAssignee::filter_by_task_id(id)
                .exec(&mut *db)
                .await?
                .into_iter()
                .map(|a| a.user_id)
                .collect(),
        )
    } else {
        None
    };
    let new_assignees = input.assignee_ids.clone();
    let old_list_name = match current.list_id {
        Some(l) => Some(List::get_by_id(&mut *db, &l).await?.name),
        None => None,
    };

    let text_changed = input
        .title
        .as_ref()
        .is_some_and(|t| t.trim() != current.title)
        || input
            .description
            .as_ref()
            .is_some_and(|d| *d != current.description);

    let mut tx = db.transaction().await?;
    let mut task = Task::get_by_id(&mut tx, &id).await?;
    let moving = new_list.is_some() || new_parent.is_some();
    let (target_list, target_parent) = (
        new_list.unwrap_or(current.list_id),
        new_parent.unwrap_or(current.parent_id),
    );

    // Where it goes among its (new) siblings; neighbours are renumbered only when there is no room.
    let mut position: Option<i64> = None;
    if input.after_id.is_some() || moving {
        let others = siblings(
            &mut tx,
            current.project_id,
            target_list,
            target_parent,
            Some(id),
        )
        .await?;
        let placement = match input.after_id {
            Some(after) => ordering::place(&others, after),
            None => ordering::Placement {
                position: ordering::append(others.iter().map(|(_, p)| *p)),
                renumber: vec![],
            },
        };
        for (sibling, new_position) in &placement.renumber {
            Task::filter_by_id(*sibling)
                .update()
                .position(*new_position)
                .exec(&mut tx)
                .await?;
        }
        position = Some(placement.position);
    }

    let was_completed = task.completed_at.is_some();
    let mut update = task.update();
    let has_field_changes = input.title.is_some()
        || input.description.is_some()
        || input.is_completed.is_some()
        || input.priority.is_some()
        || input.due_date.is_some()
        || input.start_date.is_some()
        || input.estimate_minutes.is_some()
        || input.spent_minutes.is_some()
        || input.sprint_id.is_some()
        || position.is_some()
        || moving;
    if let Some(title) = &input.title {
        update = update.title(title.trim().to_string());
    }
    if let Some(description) = &input.description {
        update = update.description(description.clone());
    }
    if let Some(is_completed) = input.is_completed
        && is_completed != was_completed
    {
        update = update.completed_at(is_completed.then(time::now));
    }
    if let Some(priority) = input.priority {
        update = update.priority(priority);
    }
    if let Some(due_date) = input.due_date {
        update = update.due_date(due_date.map(time::to_day));
    }
    if let Some(start_date) = input.start_date {
        update = update.start_date(start_date.map(time::to_day));
    }
    if let Some(sprint_id) = input.sprint_id {
        if let Some(sprint_id) = sprint_id {
            validate_sprint(&mut tx, current.project_id, sprint_id).await?;
        }
        update = update.sprint_id(sprint_id);
    }
    if let Some(minutes) = input.estimate_minutes {
        validate_minutes(minutes)?;
        update = update.estimate_minutes(minutes);
    }
    if let Some(minutes) = input.spent_minutes {
        validate_minutes(minutes)?;
        update = update.spent_minutes(minutes);
    }
    if let Some(position) = position {
        update = update.position(position);
    }
    if moving {
        update = update.list_id(target_list).parent_id(target_parent);
    }
    // Toasty rejects an update with no assignments (e.g. assignee-only PATCH).
    if has_field_changes {
        update.exec(&mut tx).await?;
    }
    drop(task);

    // Subtasks follow their parent to the new list.
    if let Some(target) = new_list {
        let below: Vec<uuid::Uuid> = trash::subtree(&project_tasks, id)
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

    let task = Task::get_by_id(&mut tx, &id).await?;
    if text_changed {
        let revision = revisions::record_task(
            &mut tx,
            id,
            user.id,
            &TaskContent {
                title: task.title.clone(),
                description: task.description.clone(),
            },
        )
        .await?;
        audit::record(
            &mut tx,
            audit::Entry::admin(Some(user.id), "task_edited")
                .target("task", id)
                .in_workspace(workspace_id)
                .data(serde_json::json!({ "revision_id": revision.id })),
        )
        .await?;
    }
    record_changes(
        &mut tx,
        workspace_id,
        user.id,
        &current,
        &task,
        old_list_name,
        old_assignees,
        new_assignees,
    )
    .await?;
    tx.commit().await?;

    let mut responses = hydrate_tasks(&mut *db, vec![task]).await?;
    Ok(Json(responses.swap_remove(0)))
}

/// Write a feed entry for each thing that changed between `before` and `after`.
#[allow(clippy::too_many_arguments)]
async fn record_changes(
    db: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
    actor: uuid::Uuid,
    before: &Task,
    after: &Task,
    old_list_name: Option<String>,
    old_assignees: Option<HashSet<uuid::Uuid>>,
    new_assignees: Option<Vec<uuid::Uuid>>,
) -> Result<(), ApiError> {
    let mut events: Vec<(&'static str, serde_json::Value)> = vec![];
    if before.title != after.title {
        events.push((
            "task_renamed",
            serde_json::json!({ "from": before.title, "to": after.title }),
        ));
    }
    if before.completed_at.is_some() != after.completed_at.is_some() {
        let kind = if after.completed_at.is_some() {
            "task_completed"
        } else {
            "task_reopened"
        };
        events.push((kind, serde_json::json!({ "title": after.title })));
    }
    if before.list_id != after.list_id {
        let to = match after.list_id {
            Some(l) => Some(List::get_by_id(db, &l).await?.name),
            None => None,
        };
        events.push((
            "task_moved",
            serde_json::json!({
                "title": after.title,
                "from": old_list_name.unwrap_or_else(|| "Inbox".to_string()),
                "to": to.unwrap_or_else(|| "Inbox".to_string()),
            }),
        ));
    }
    if before.due_date != after.due_date {
        events.push((
            "due_changed",
            serde_json::json!({
                "title": after.title,
                "from": before.due_date.map(|d| time::timestamp(d).to_string()),
                "to": after.due_date.map(|d| time::timestamp(d).to_string()),
            }),
        ));
    }
    if before.priority != after.priority {
        events.push((
            "priority_changed",
            serde_json::json!({ "title": after.title, "from": before.priority, "to": after.priority }),
        ));
    }
    // What the people who follow the task are told: the first thing that changed.
    let what = events.iter().find_map(|(kind, _)| match *kind {
        "task_completed" => Some("completed"),
        "task_reopened" => Some("reopened"),
        "task_renamed" => Some("renamed"),
        "task_moved" => Some("moved"),
        "due_changed" => Some("due date changed"),
        "priority_changed" => Some("priority changed"),
        _ => None,
    });
    if let Some(what) = what {
        crate::notify::task_changed(
            db,
            workspace_id,
            after.project_id,
            after.id,
            actor,
            &after.title,
            what,
        )
        .await?;
    }
    if let (Some(old), Some(new)) = (old_assignees, new_assignees) {
        let new: HashSet<uuid::Uuid> = new.into_iter().collect();
        crate::notify::notify(
            db,
            new.difference(&old).copied().collect::<Vec<_>>(),
            crate::notify::Notice {
                workspace_id,
                kind: "task_assigned",
                target_type: crate::notify::TASK,
                target_id: after.id,
                actor: Some(actor),
                data: serde_json::json!({ "title": after.title, "project_id": after.project_id }),
            },
        )
        .await?;
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
            let people = People::load(db, ids.iter().copied()).await?;
            let names: Vec<String> = ids.into_iter().filter_map(|id| people.label(id)).collect();
            events.push((
                kind,
                serde_json::json!({ "title": after.title, "who": names }),
            ));
        }
    }
    for (kind, data) in events {
        audit::task_event(
            db,
            workspace_id,
            after.project_id,
            actor,
            kind,
            after.id,
            data,
        )
        .await?;
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
    let mut db = state.writer().await;
    let (task, workspace_id) = access::require_task(&mut *db, &user, id, Role::Write).await?;

    let mut tx = db.transaction().await?;
    trash::delete_task(&mut tx, &task, time::now()).await?;
    audit::deleted(&mut tx, user.id, workspace_id, "task", id, &task.title).await?;
    audit::task_event(
        &mut tx,
        workspace_id,
        task.project_id,
        user.id,
        "task_deleted",
        id,
        serde_json::json!({ "title": task.title }),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Assignees must be able to see the workspace (they must be able to see the task).
pub(crate) async fn validate_assignees(
    db: &mut dyn toasty::Executor,
    assignee_ids: &[uuid::Uuid],
    workspace_id: uuid::Uuid,
) -> Result<(), ApiError> {
    for user_id in assignee_ids {
        if !access::can_see_workspace(db, *user_id, workspace_id).await? {
            return Err(ApiError::bad_request(
                "assignees must be members of the workspace",
            ));
        }
    }
    Ok(())
}

/// Labels must belong to the same workspace. Returns the label rows.
async fn validate_labels(
    db: &mut dyn toasty::Executor,
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
    db: &mut dyn toasty::Executor,
    tasks: Vec<Task>,
) -> Result<Vec<TaskResponse>, ApiError> {
    if tasks.is_empty() {
        return Ok(vec![]);
    }
    let task_ids: Vec<uuid::Uuid> = tasks.iter().map(|t| t.id).collect();

    let assignee_rows = TaskAssignee::all()
        .filter(in_list(TaskAssignee::fields().task_id(), task_ids.clone()))
        .exec(db)
        .await?;
    let label_rows = TaskLabel::all()
        .filter(in_list(TaskLabel::fields().task_id(), task_ids.clone()))
        .exec(db)
        .await?;

    // Every live task in those projects, to count each task's subtasks (any depth).
    let project_ids: HashSet<uuid::Uuid> = tasks.iter().map(|t| t.project_id).collect();
    let live_in_projects: Vec<Task> = Task::all()
        .filter(in_list(
            Task::fields().project_id(),
            project_ids.iter().copied().collect::<Vec<_>>(),
        ))
        .filter(Task::fields().deleted_at().is_none())
        .exec(db)
        .await?;
    let completed_by_id: HashMap<uuid::Uuid, bool> = live_in_projects
        .iter()
        .map(|t| (t.id, t.completed_at.is_some()))
        .collect();

    let comment_rows = Comment::all()
        .filter(in_list(Comment::fields().task_id(), task_ids.clone()))
        .exec(db)
        .await?;
    let file_rows = FileRef::all()
        .filter(in_list(FileRef::fields().target_id(), task_ids))
        .exec(db)
        .await?;

    let people = People::load(db, assignee_rows.iter().map(|a| a.user_id)).await?;
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

    let labels_by_id: HashMap<uuid::Uuid, Label> = labels
        .into_iter()
        .filter(|l| l.deleted_at.is_none())
        .map(|l| (l.id, l))
        .collect();

    let mut assignees_by_task: HashMap<uuid::Uuid, Vec<PublicUser>> = HashMap::new();
    for row in assignee_rows {
        if let Some(user) = people.public(row.user_id) {
            assignees_by_task.entry(row.task_id).or_default().push(user);
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
            let below: Vec<uuid::Uuid> = trash::subtree(&live_in_projects, task.id)
                .into_iter()
                .filter(|id| *id != task.id)
                .collect();
            let subtask_completed = below
                .iter()
                .filter(|id| completed_by_id.get(id).copied().unwrap_or(false))
                .count();
            TaskResponse {
                id: task.id,
                project_id: task.project_id,
                list_id: task.list_id,
                parent_id: task.parent_id,
                title: task.title,
                description: task.description,
                completed_at: task.completed_at,
                position: task.position,
                priority: task.priority,
                start_date: task.start_date,
                due_date: task.due_date,
                sprint_id: task.sprint_id,
                estimate_minutes: task.estimate_minutes,
                spent_minutes: task.spent_minutes,
                assignees,
                labels: task_labels,
                subtask_total: below.len(),
                subtask_completed,
                comment_count: comment_rows
                    .iter()
                    .filter(|c| c.task_id == task.id && c.deleted_at.is_none())
                    .count(),
                attachment_count: file_rows
                    .iter()
                    .filter(|r| r.target_type == "task" && r.target_id == task.id)
                    .count(),
            }
        })
        .collect())
}
