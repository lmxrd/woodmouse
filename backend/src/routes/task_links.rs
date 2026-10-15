//! Links between tasks: blocks / blocked by, relates to, duplicates / duplicated by.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::activity::{self, Event};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{LinkKind, List, Role, Task, TaskLink};
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_links, add_link))
        .routes(routes!(remove_link))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// This task is the owner: "this task blocks ...".
    Outgoing,
    /// The other task owns it: "this task is blocked by ...".
    Incoming,
}

/// The other end of a link.
#[derive(Serialize, ToSchema)]
pub struct LinkedTask {
    pub id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub title: String,
    pub is_completed: bool,
}

#[derive(Serialize, ToSchema)]
pub struct TaskLinkView {
    pub id: uuid::Uuid,
    pub kind: LinkKind,
    pub direction: Direction,
    pub task: LinkedTask,
}

#[derive(Deserialize, ToSchema)]
pub struct AddLink {
    pub related_task_id: uuid::Uuid,
    pub kind: LinkKind,
}

async fn views_for(
    db: &mut toasty::Db,
    task_id: uuid::Uuid,
    links: Vec<TaskLink>,
) -> Result<Vec<TaskLinkView>, ApiError> {
    let other_ids: Vec<uuid::Uuid> = links
        .iter()
        .map(|l| {
            if l.task_id == task_id {
                l.related_task_id
            } else {
                l.task_id
            }
        })
        .collect();
    if other_ids.is_empty() {
        return Ok(vec![]);
    }
    let tasks = Task::all()
        .filter(in_list(Task::fields().id(), other_ids))
        .exec(db)
        .await?;
    let list_ids: Vec<uuid::Uuid> = tasks.iter().map(|t| t.list_id).collect();
    let lists = List::all()
        .filter(in_list(List::fields().id(), list_ids))
        .exec(db)
        .await?;

    Ok(links
        .into_iter()
        .filter_map(|link| {
            let outgoing = link.task_id == task_id;
            let other_id = if outgoing {
                link.related_task_id
            } else {
                link.task_id
            };
            let other = tasks
                .iter()
                .find(|t| t.id == other_id && t.deleted_at.is_none())?;
            let project_id = lists.iter().find(|l| l.id == other.list_id)?.project_id;
            Some(TaskLinkView {
                id: link.id,
                kind: link.kind,
                direction: if outgoing {
                    Direction::Outgoing
                } else {
                    Direction::Incoming
                },
                task: LinkedTask {
                    id: other.id,
                    project_id,
                    title: other.title.clone(),
                    is_completed: other.is_completed,
                },
            })
        })
        .collect())
}

async fn links_of(db: &mut toasty::Db, task_id: uuid::Uuid) -> Result<Vec<TaskLink>, ApiError> {
    let mut links = TaskLink::filter_by_task_id(task_id).exec(db).await?;
    links.extend(
        TaskLink::filter_by_related_task_id(task_id)
            .exec(db)
            .await?,
    );
    Ok(links)
}

/// The links of a task, in both directions. Links to deleted tasks are hidden.
#[utoipa::path(
    get,
    path = "/tasks/{id}/links",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    responses(
        (status = 200, description = "The task's links", body = [TaskLinkView]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No task with that id, or no access")
    )
)]
pub async fn list_links(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<TaskLinkView>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_task_access(db, &user, id, Role::Read).await?;
    let links = links_of(db, id).await?;
    Ok(Json(views_for(db, id, links).await?))
}

/// Link a task to another task in the same workspace.
#[utoipa::path(
    post,
    path = "/tasks/{id}/links",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    request_body = AddLink,
    responses(
        (status = 201, description = "Link created", body = TaskLinkView),
        (status = 400, description = "Invalid link"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such task, or no access"),
        (status = 409, description = "These tasks are already linked this way")
    )
)]
pub async fn add_link(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<AddLink>,
) -> Result<(StatusCode, Json<TaskLinkView>), ApiError> {
    let db = &mut *state.db.lock().await;
    let workspace_id = access::require_task_access(db, &user, id, Role::Write).await?;
    if input.related_task_id == id {
        return Err(ApiError::bad_request("a task cannot link to itself"));
    }
    // The other task must be live and in the same workspace (otherwise it looks nonexistent).
    let other = access::require_task_access(db, &user, input.related_task_id, Role::Write).await?;
    if other != workspace_id {
        return Err(ApiError::not_found());
    }

    // Any existing link of the same kind between the two (either direction) is a conflict.
    let existing = links_of(db, id).await?;
    if existing.iter().any(|l| {
        l.kind == input.kind
            && (l.task_id == input.related_task_id || l.related_task_id == input.related_task_id)
    }) {
        return Err(ApiError::conflict(
            "these tasks are already linked this way",
        ));
    }

    let link = toasty::create!(TaskLink {
        task_id: id,
        related_task_id: input.related_task_id,
        kind: input.kind,
    })
    .exec(db)
    .await?;
    let mut views = views_for(db, id, vec![link]).await?;
    let view = views.swap_remove(0);

    let task = Task::get_by_id(db, &id).await?;
    let project_id = List::get_by_id(db, &task.list_id).await?.project_id;
    activity::record(
        db,
        Event {
            workspace_id,
            project_id: Some(project_id),
            task_id: Some(id),
            actor_id: user.id,
            kind: "link_added",
            data: serde_json::json!({ "title": task.title, "kind": view.kind, "other": view.task.title }),
        },
    )
    .await;
    Ok((StatusCode::CREATED, Json(view)))
}

/// Remove a link.
#[utoipa::path(
    delete,
    path = "/tasks/{id}/links/{link_id}",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(
        ("id" = uuid::Uuid, Path, description = "Task id"),
        ("link_id" = uuid::Uuid, Path, description = "Link id")
    ),
    responses(
        (status = 204, description = "Link removed"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such link on this task, or no access")
    )
)]
pub async fn remove_link(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((id, link_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<StatusCode, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_task_access(db, &user, id, Role::Write).await?;
    let link = TaskLink::get_by_id(db, &link_id).await?;
    if link.task_id != id && link.related_task_id != id {
        return Err(ApiError::not_found());
    }
    link.delete().exec(db).await?;
    Ok(StatusCode::NO_CONTENT)
}
