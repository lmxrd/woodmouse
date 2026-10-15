//! Trash: everything the API "deletes" is soft-deleted and listed here, where write-access members
//! can restore it. There is deliberately no way to purge from Trash.

use std::collections::HashMap;

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
use crate::models::{Comment, Label, List, Project, Role, SavedView, Task, User};
use crate::routes::auth::PublicUser;
use crate::state::AppState;
use crate::trash;

/// (kind, id, name, context, deleted_at, deleted_by, child_count) before user lookup.
type RawItem = (
    TrashKind,
    uuid::Uuid,
    String,
    Option<String>,
    i64,
    Option<uuid::Uuid>,
    usize,
);

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_trash))
        .routes(routes!(restore_item))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TrashKind {
    Project,
    List,
    Task,
    Label,
    Comment,
    View,
}

/// One restorable thing in the trash. Items deleted as part of a bigger deletion (a task in a deleted
/// list) are not listed on their own: restoring the root brings them back.
#[derive(Serialize, ToSchema)]
pub struct TrashItem {
    pub kind: TrashKind,
    pub id: uuid::Uuid,
    pub name: String,
    /// Where it lived, e.g. the project and list of a task.
    pub context: Option<String>,
    /// Unix epoch seconds.
    pub deleted_at: i64,
    pub deleted_by: Option<PublicUser>,
    /// How many contained items go with it (tasks of a list, subtasks of a task, ...).
    pub child_count: usize,
}

/// The workspace's trash, newest first. Needs write access.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/trash",
    tag = "trash",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "Deleted items", body = [TrashItem]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or not enough access")
    )
)]
pub async fn list_trash(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<TrashItem>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Write).await?;

    let projects = Project::filter_by_workspace_id(id).exec(db).await?;
    let project_ids: Vec<uuid::Uuid> = projects.iter().map(|p| p.id).collect();
    let lists = if project_ids.is_empty() {
        vec![]
    } else {
        List::all()
            .filter(in_list(List::fields().project_id(), project_ids))
            .exec(db)
            .await?
    };
    let list_ids: Vec<uuid::Uuid> = lists.iter().map(|l| l.id).collect();
    let tasks = if list_ids.is_empty() {
        vec![]
    } else {
        Task::all()
            .filter(in_list(Task::fields().list_id(), list_ids))
            .exec(db)
            .await?
    };
    let labels = Label::filter_by_workspace_id(id).exec(db).await?;

    let project_of: HashMap<_, _> = projects.iter().map(|p| (p.id, p)).collect();
    let list_of: HashMap<_, _> = lists.iter().map(|l| (l.id, l)).collect();
    let task_of: HashMap<_, _> = tasks.iter().map(|t| (t.id, t)).collect();

    let mut raw: Vec<RawItem> = vec![];
    for p in projects.iter().filter(|p| p.deleted_at.is_some()) {
        let at = p.deleted_at.unwrap_or_default();
        let together = tasks
            .iter()
            .filter(|t| t.deleted_at == Some(at))
            .filter(|t| {
                list_of
                    .get(&t.list_id)
                    .is_some_and(|l| l.project_id == p.id)
            })
            .count();
        raw.push((
            TrashKind::Project,
            p.id,
            p.name.clone(),
            None,
            at,
            p.deleted_by,
            together,
        ));
    }
    for l in lists.iter().filter(|l| l.deleted_at.is_some()) {
        let at = l.deleted_at.unwrap_or_default();
        let project = project_of.get(&l.project_id);
        if project.is_some_and(|p| p.deleted_at == Some(at)) {
            continue; // deleted together with its project
        }
        let together = tasks
            .iter()
            .filter(|t| t.list_id == l.id && t.deleted_at == Some(at))
            .count();
        raw.push((
            TrashKind::List,
            l.id,
            l.name.clone(),
            project.map(|p| p.name.clone()),
            at,
            l.deleted_by,
            together,
        ));
    }
    for t in tasks.iter().filter(|t| t.deleted_at.is_some()) {
        let at = t.deleted_at.unwrap_or_default();
        let list = list_of.get(&t.list_id);
        let project = list.and_then(|l| project_of.get(&l.project_id));
        let with_container = list.is_some_and(|l| l.deleted_at == Some(at))
            || project.is_some_and(|p| p.deleted_at == Some(at));
        let with_parent = t
            .parent_id
            .and_then(|pid| task_of.get(&pid))
            .is_some_and(|parent| parent.deleted_at == Some(at));
        if with_container || with_parent {
            continue;
        }
        let together = trash::subtree(&tasks, t.id)
            .into_iter()
            .filter(|sid| {
                *sid != t.id && task_of.get(sid).is_some_and(|s| s.deleted_at == Some(at))
            })
            .count();
        let context = match (project, list) {
            (Some(p), Some(l)) => Some(format!("{} / {}", p.name, l.name)),
            _ => None,
        };
        raw.push((
            TrashKind::Task,
            t.id,
            t.title.clone(),
            context,
            at,
            t.deleted_by,
            together,
        ));
    }
    for l in labels.iter().filter(|l| l.deleted_at.is_some()) {
        raw.push((
            TrashKind::Label,
            l.id,
            l.name.clone(),
            None,
            l.deleted_at.unwrap_or_default(),
            l.deleted_by,
            0,
        ));
    }

    // Deleted saved views: shared ones, and the caller's own private ones.
    for v in SavedView::filter_by_workspace_id(id).exec(db).await? {
        if v.deleted_at.is_none() || !crate::routes::saved_views::visible_to(&v, &user) {
            continue;
        }
        raw.push((
            TrashKind::View,
            v.id,
            v.name.clone(),
            project_of.get(&v.project_id).map(|p| p.name.clone()),
            v.deleted_at.unwrap_or_default(),
            v.deleted_by,
            0,
        ));
    }

    // Deleted comments (a comment is only listed while its task is live).
    let task_ids: Vec<uuid::Uuid> = tasks
        .iter()
        .filter(|t| t.deleted_at.is_none())
        .map(|t| t.id)
        .collect();
    if !task_ids.is_empty() {
        let comments = Comment::all()
            .filter(in_list(Comment::fields().task_id(), task_ids))
            .exec(db)
            .await?;
        for c in comments.iter().filter(|c| c.deleted_at.is_some()) {
            let task = task_of.get(&c.task_id);
            let preview: String = c.body.chars().take(80).collect();
            raw.push((
                TrashKind::Comment,
                c.id,
                preview,
                task.map(|t| format!("on {}", t.title)),
                c.deleted_at.unwrap_or_default(),
                c.deleted_by,
                0,
            ));
        }
    }

    let user_ids: Vec<uuid::Uuid> = raw.iter().filter_map(|r| r.5).collect();
    let users: HashMap<uuid::Uuid, PublicUser> = if user_ids.is_empty() {
        HashMap::new()
    } else {
        User::all()
            .filter(in_list(User::fields().id(), user_ids))
            .exec(db)
            .await?
            .iter()
            .map(|u| (u.id, PublicUser::from(u)))
            .collect()
    };

    let mut items: Vec<TrashItem> = raw
        .into_iter()
        .map(
            |(kind, id, name, context, deleted_at, by, child_count)| TrashItem {
                kind,
                id,
                name,
                context,
                deleted_at,
                deleted_by: by.and_then(|u| users.get(&u).cloned()),
                child_count,
            },
        )
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.deleted_at));
    Ok(Json(items))
}

/// Restore an item (and what was deleted with it). Conflicts if its container is still deleted.
#[utoipa::path(
    post,
    path = "/workspaces/{id}/trash/{kind}/{item_id}/restore",
    tag = "trash",
    security(("session_cookie" = [])),
    params(
        ("id" = uuid::Uuid, Path, description = "Workspace id"),
        ("kind" = TrashKind, Path, description = "What kind of item"),
        ("item_id" = uuid::Uuid, Path, description = "Item id")
    ),
    responses(
        (status = 204, description = "Restored"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "Not in this workspace's trash, or not enough access"),
        (status = 409, description = "Its project, list or parent task is still deleted")
    )
)]
pub async fn restore_item(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((id, kind, item_id)): Path<(uuid::Uuid, TrashKind, uuid::Uuid)>,
) -> Result<StatusCode, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Write).await?;

    let workspace_of = match kind {
        TrashKind::Project => access::project_workspace(db, item_id).await?,
        TrashKind::List => access::list_workspace(db, item_id).await?,
        TrashKind::Task => access::task_workspace(db, item_id).await?,
        TrashKind::Label => Label::get_by_id(db, &item_id).await?.workspace_id,
        TrashKind::View => SavedView::get_by_id(db, &item_id).await?.workspace_id,
        TrashKind::Comment => {
            let task = crate::routes::comments::comment_task(db, item_id).await?;
            access::task_workspace(db, task.id).await?
        }
    };
    if workspace_of != id {
        return Err(ApiError::not_found());
    }

    let mut restored_task: Option<Task> = None;
    let mut tx = db.transaction().await?;
    match kind {
        TrashKind::Project => {
            let project = Project::get_by_id(&mut tx, &item_id).await?;
            if project.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_project(&mut tx, &project).await?;
        }
        TrashKind::List => {
            let list = List::get_by_id(&mut tx, &item_id).await?;
            if list.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_list(&mut tx, &list).await?;
        }
        TrashKind::Task => {
            let task = Task::get_by_id(&mut tx, &item_id).await?;
            if task.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_task(&mut tx, &task).await?;
            restored_task = Some(task);
        }
        TrashKind::Label => {
            let label = Label::get_by_id(&mut tx, &item_id).await?;
            if label.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_label(&mut tx, item_id).await?;
        }
        TrashKind::View => {
            let view = SavedView::get_by_id(&mut tx, &item_id).await?;
            if view.deleted_at.is_none() || !crate::routes::saved_views::visible_to(&view, &user) {
                return Err(ApiError::not_found());
            }
            if Project::get_by_id(&mut tx, &view.project_id)
                .await?
                .deleted_at
                .is_some()
            {
                return Err(ApiError::conflict("restore its project first"));
            }
            SavedView::filter_by_id(item_id)
                .update()
                .deleted_at(None::<i64>)
                .deleted_by(None::<uuid::Uuid>)
                .exec(&mut tx)
                .await?;
        }
        TrashKind::Comment => {
            let comment = Comment::get_by_id(&mut tx, &item_id).await?;
            if comment.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            if Task::get_by_id(&mut tx, &comment.task_id)
                .await?
                .deleted_at
                .is_some()
            {
                return Err(ApiError::conflict("restore its task first"));
            }
            Comment::filter_by_id(item_id)
                .update()
                .deleted_at(None::<i64>)
                .deleted_by(None::<uuid::Uuid>)
                .exec(&mut tx)
                .await?;
        }
    }
    tx.commit().await?;

    if let Some(task) = restored_task {
        let project_id = List::get_by_id(db, &task.list_id).await?.project_id;
        activity::record(
            db,
            Event {
                workspace_id: id,
                project_id: Some(project_id),
                task_id: Some(task.id),
                actor_id: user.id,
                kind: "task_restored",
                data: serde_json::json!({ "title": task.title }),
            },
        )
        .await;
    }
    Ok(StatusCode::NO_CONTENT)
}
