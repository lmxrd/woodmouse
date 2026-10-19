//! Links between tasks: blocks / blocked by, relates to, duplicates / duplicated by.

use crate::db::Db;
use crate::db::prelude::*;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use entity::{projects, task_links, tasks};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::state::AppState;
use entity::{LinkKind, Role, TaskLink};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_links, add_link))
        .routes(routes!(remove_link))
        .routes(routes!(list_workspace_links))
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
    /// When the task was completed; absent while it is open.
    #[serde(with = "crate::time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub completed_at: Option<crate::time::DateTime>,
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
    db: &impl Db,
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
    let tasks = tasks::Entity::find()
        .filter(tasks::Column::Id.is_in(other_ids.iter().copied()))
        .all(db)
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
                    project_id: other.project_id,
                    title: other.title.clone(),
                    completed_at: other.completed_at,
                },
            })
        })
        .collect())
}

async fn links_of(db: &impl Db, task_id: uuid::Uuid) -> Result<Vec<TaskLink>, ApiError> {
    let mut links = task_links::Entity::find()
        .filter(task_links::Column::TaskId.eq(task_id))
        .all(db)
        .await?;
    links.extend(
        task_links::Entity::find()
            .filter(task_links::Column::RelatedTaskId.eq(task_id))
            .all(db)
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
    let db = state.db();
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
    let db = state.writer().await?;
    let workspace_id = access::require_task_access(&*db, &user, id, Role::Write).await?;
    if input.related_task_id == id {
        return Err(ApiError::bad_request("a task cannot link to itself"));
    }
    // The other task must be live and in the same workspace (otherwise it looks nonexistent).
    let other =
        access::require_task_access(&*db, &user, input.related_task_id, Role::Write).await?;
    if other != workspace_id {
        return Err(ApiError::not_found());
    }

    // Any existing link of the same kind between the two (either direction) is a conflict.
    let existing = links_of(&*db, id).await?;
    if existing.iter().any(|l| {
        l.kind == input.kind
            && (l.task_id == input.related_task_id || l.related_task_id == input.related_task_id)
    }) {
        return Err(ApiError::conflict(
            "these tasks are already linked this way",
        ));
    }

    let tx = db.begin().await?;
    let link = task_links::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        task_id: Set(id),
        related_task_id: Set(input.related_task_id),
        kind: Set(input.kind),
    }
    .insert(&tx)
    .await?;
    let mut views = views_for(&tx, id, vec![link]).await?;
    let view = views.swap_remove(0);

    let task = tasks::Entity::find_by_id(id).one_or_not_found(&tx).await?;
    audit::task_event(
        &tx,
        workspace_id,
        task.project_id,
        user.id,
        "link_added",
        id,
        serde_json::json!({ "title": task.title, "kind": view.kind, "other": view.task.title }),
    )
    .await?;
    tx.commit().await?;
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
    let db = state.writer().await?;
    access::require_task_access(&*db, &user, id, Role::Write).await?;
    let link = task_links::Entity::find_by_id(link_id)
        .one_or_not_found(&*db)
        .await?;
    if link.task_id != id && link.related_task_id != id {
        return Err(ApiError::not_found());
    }
    task_links::Entity::delete_many()
        .filter(task_links::Column::Id.eq(link.id))
        .exec(&*db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// One link between two tasks, as a plain edge (for drawing dependency arrows).
#[derive(Serialize, ToSchema)]
pub struct LinkEdge {
    pub id: uuid::Uuid,
    /// The owning task ("this task blocks ...").
    pub task_id: uuid::Uuid,
    pub related_task_id: uuid::Uuid,
    pub kind: LinkKind,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LinkFilters {
    /// Only links between tasks of this project.
    pub project_id: Option<uuid::Uuid>,
}

/// Every link between live tasks of a workspace (or one project), as edges.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/task-links",
    tag = "tasks",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id"), LinkFilters),
    responses(
        (status = 200, description = "The links", body = [LinkEdge]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn list_workspace_links(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Query(filters): Query<LinkFilters>,
) -> Result<Json<Vec<LinkEdge>>, ApiError> {
    let db = state.db();
    access::require_access(db, &user, id, Role::Read).await?;

    let mut projects = projects::Entity::find()
        .filter(projects::Column::WorkspaceId.eq(id))
        .filter(projects::Column::DeletedAt.is_null());
    if let Some(project_id) = filters.project_id {
        projects = projects.filter(projects::Column::Id.eq(project_id));
    }
    let project_ids: Vec<uuid::Uuid> = projects.all(db).await?.into_iter().map(|p| p.id).collect();
    if project_ids.is_empty() {
        return Ok(Json(vec![]));
    }
    let live: std::collections::HashSet<uuid::Uuid> = tasks::Entity::find()
        .filter(tasks::Column::DeletedAt.is_null())
        .filter(tasks::Column::ProjectId.is_in(project_ids.iter().copied()))
        .all(db)
        .await?
        .into_iter()
        .map(|t| t.id)
        .collect();
    if live.is_empty() {
        return Ok(Json(vec![]));
    }
    let links = task_links::Entity::find()
        .filter(task_links::Column::TaskId.is_in(live.iter().copied()))
        .all(db)
        .await?;
    Ok(Json(
        links
            .into_iter()
            .filter(|l| live.contains(&l.related_task_id))
            .map(|l| LinkEdge {
                id: l.id,
                task_id: l.task_id,
                related_task_id: l.related_task_id,
                kind: l.kind,
            })
            .collect(),
    ))
}
