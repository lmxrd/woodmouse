//! Trash: everything the API "deletes" is soft-deleted and listed here, where write-access members
//! can restore it. There is deliberately no way to purge from Trash. Who deleted an item comes from the
//! audit log.

use crate::db::prelude::*;
use crate::models::{User, users};
use crate::models::{
    audit_logs, comments, files, labels, lists, projects, saved_views, sprints, tasks, wiki_pages,
};
use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{AuditLog, Role};
use crate::routes::auth::PublicUser;
use crate::state::AppState;
use crate::time::DateTime;
use crate::{time, trash};

/// (kind, id, name, context, deleted_at, deleted_by, child_count) before user lookup.
type RawItem = (
    TrashKind,
    uuid::Uuid,
    String,
    Option<String>,
    DateTime,
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
    Page,
    Sprint,
    File,
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
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub deleted_at: DateTime,
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
    let db = state.db();
    access::require_access(db, &user, id, Role::Write).await?;

    // Who deleted what: the newest deletion record of each item.
    let mut deleted_by_item: HashMap<(String, uuid::Uuid), uuid::Uuid> = HashMap::new();
    let mut deletions: Vec<AuditLog> = audit_logs::Entity::find()
        .filter(audit_logs::Column::WorkspaceId.eq(id))
        .filter(audit_logs::Column::Action.eq("entity_deleted"))
        .all(db)
        .await?;
    deletions.sort_by_key(|e| (e.created_at, e.id));
    for entry in deletions {
        if let (Some(kind), Some(target), Some(actor)) =
            (entry.target_type, entry.target_id, entry.actor_id)
        {
            deleted_by_item.insert((kind, target), actor);
        }
    }
    let by = |kind: &str, item: uuid::Uuid| deleted_by_item.get(&(kind.to_string(), item)).copied();

    let projects = projects::Entity::find()
        .filter(projects::Column::WorkspaceId.eq(id))
        .all(db)
        .await?;
    let project_ids: Vec<uuid::Uuid> = projects.iter().map(|p| p.id).collect();
    let lists = if project_ids.is_empty() {
        vec![]
    } else {
        lists::Entity::find()
            .filter(lists::Column::ProjectId.is_in(project_ids.iter().copied()))
            .all(db)
            .await?
    };
    let tasks = if project_ids.is_empty() {
        vec![]
    } else {
        tasks::Entity::find()
            .filter(tasks::Column::ProjectId.is_in(project_ids.iter().copied()))
            .all(db)
            .await?
    };
    let labels = labels::Entity::find()
        .filter(labels::Column::WorkspaceId.eq(id))
        .all(db)
        .await?;

    let project_of: HashMap<_, _> = projects.iter().map(|p| (p.id, p)).collect();
    let list_of: HashMap<_, _> = lists.iter().map(|l| (l.id, l)).collect();
    let task_of: HashMap<_, _> = tasks.iter().map(|t| (t.id, t)).collect();

    let mut raw: Vec<RawItem> = vec![];
    for p in projects.iter().filter(|p| p.deleted_at.is_some()) {
        let at = p.deleted_at.unwrap_or_default();
        let together = tasks
            .iter()
            .filter(|t| t.deleted_at == Some(at) && t.project_id == p.id)
            .count();
        raw.push((
            TrashKind::Project,
            p.id,
            p.name.clone(),
            None,
            at,
            by("project", p.id),
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
            .filter(|t| t.list_id == Some(l.id) && t.deleted_at == Some(at))
            .count();
        raw.push((
            TrashKind::List,
            l.id,
            l.name.clone(),
            project.map(|p| p.name.clone()),
            at,
            by("list", l.id),
            together,
        ));
    }
    for t in tasks.iter().filter(|t| t.deleted_at.is_some()) {
        let at = t.deleted_at.unwrap_or_default();
        let list = t.list_id.and_then(|l| list_of.get(&l).copied());
        let project = project_of.get(&t.project_id);
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
            (Some(p), None) => Some(format!("{} / Inbox", p.name)),
            _ => None,
        };
        raw.push((
            TrashKind::Task,
            t.id,
            t.title.clone(),
            context,
            at,
            by("task", t.id),
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
            by("label", l.id),
            0,
        ));
    }

    // Deleted pages: only those not deleted together with their parent page.
    let pages = wiki_pages::Entity::find()
        .filter(wiki_pages::Column::WorkspaceId.eq(id))
        .all(db)
        .await?;
    for p in pages.iter().filter(|p| p.deleted_at.is_some()) {
        let at = p.deleted_at.unwrap_or_default();
        let with_parent = p
            .parent_id
            .and_then(|pid| pages.iter().find(|x| x.id == pid))
            .is_some_and(|parent| parent.deleted_at == Some(at));
        if with_parent {
            continue;
        }
        let together = trash::page_subtree(&pages, p.id)
            .into_iter()
            .filter(|sid| {
                *sid != p.id
                    && pages
                        .iter()
                        .any(|x| x.id == *sid && x.deleted_at == Some(at))
            })
            .count();
        raw.push((
            TrashKind::Page,
            p.id,
            p.title.clone(),
            None,
            at,
            by("project", p.id),
            together,
        ));
    }

    // Deleted files.
    for f in files::Entity::find()
        .filter(files::Column::WorkspaceId.eq(id))
        .all(db)
        .await?
        .iter()
        .filter(|f| f.deleted_at.is_some())
    {
        raw.push((
            TrashKind::File,
            f.id,
            f.name.clone(),
            None,
            f.deleted_at.unwrap_or_default(),
            by("file", f.id),
            0,
        ));
    }

    // Deleted sprints (not those whose project went too).
    let sprints = if projects.is_empty() {
        vec![]
    } else {
        sprints::Entity::find()
            .filter(sprints::Column::ProjectId.is_in(projects.iter().map(|p| p.id)))
            .all(db)
            .await?
    };
    for s in sprints.iter().filter(|s| s.deleted_at.is_some()) {
        raw.push((
            TrashKind::Sprint,
            s.id,
            s.name.clone(),
            project_of.get(&s.project_id).map(|p| p.name.clone()),
            s.deleted_at.unwrap_or_default(),
            by("sprint", s.id),
            0,
        ));
    }

    // Deleted saved views: shared ones, and the caller's own private ones.
    for v in saved_views::Entity::find()
        .filter(saved_views::Column::WorkspaceId.eq(id))
        .all(db)
        .await?
    {
        if v.deleted_at.is_none() || !crate::routes::saved_views::visible_to(&v, &user) {
            continue;
        }
        raw.push((
            TrashKind::View,
            v.id,
            v.name.clone(),
            project_of.get(&v.project_id).map(|p| p.name.clone()),
            v.deleted_at.unwrap_or_default(),
            by("view", v.id),
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
        let comments = comments::Entity::find()
            .filter(comments::Column::TaskId.is_in(task_ids.iter().copied()))
            .all(db)
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
                by("comment", c.id),
                0,
            ));
        }
    }

    let users: HashMap<uuid::Uuid, User> = users::Entity::find()
        .filter(users::Column::Id.is_in(raw.iter().filter_map(|r| r.5)))
        .all(db)
        .await?
        .into_iter()
        .map(|u| (u.id, u))
        .collect();

    let mut items: Vec<TrashItem> = raw
        .into_iter()
        .map(
            |(kind, id, name, context, deleted_at, by, child_count)| TrashItem {
                kind,
                id,
                name,
                context,
                deleted_at,
                deleted_by: by.and_then(|id| users.get(&id)).map(PublicUser::from),
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
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Write).await?;

    let workspace_of = match kind {
        TrashKind::Project => access::project_workspace(&*db, item_id).await?,
        TrashKind::List => access::list_workspace(&*db, item_id).await?,
        TrashKind::Task => access::task_workspace(&*db, item_id).await?,
        TrashKind::Label => {
            labels::Entity::find_by_id(item_id)
                .one_or_not_found(&*db)
                .await?
                .workspace_id
        }
        TrashKind::Page => {
            wiki_pages::Entity::find_by_id(item_id)
                .one_or_not_found(&*db)
                .await?
                .workspace_id
        }
        TrashKind::File => {
            files::Entity::find_by_id(item_id)
                .one_or_not_found(&*db)
                .await?
                .workspace_id
        }
        TrashKind::Sprint => {
            let sprint = sprints::Entity::find_by_id(item_id)
                .one_or_not_found(&*db)
                .await?;
            access::project_workspace(&*db, sprint.project_id).await?
        }
        TrashKind::View => {
            saved_views::Entity::find_by_id(item_id)
                .one_or_not_found(&*db)
                .await?
                .workspace_id
        }
        TrashKind::Comment => {
            let task = crate::routes::comments::comment_task(&*db, item_id).await?;
            access::task_workspace(&*db, task.id).await?
        }
    };
    if workspace_of != id {
        return Err(ApiError::not_found());
    }

    let tx = db.begin().await?;
    let (target_type, title): (&'static str, String) = match kind {
        TrashKind::Project => {
            let project = projects::Entity::find_by_id(item_id)
                .one_or_not_found(&tx)
                .await?;
            if project.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_project(&tx, &project).await?;
            ("project", project.name)
        }
        TrashKind::List => {
            let list = lists::Entity::find_by_id(item_id)
                .one_or_not_found(&tx)
                .await?;
            if list.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_list(&tx, &list).await?;
            ("list", list.name)
        }
        TrashKind::Task => {
            let task = tasks::Entity::find_by_id(item_id)
                .one_or_not_found(&tx)
                .await?;
            if task.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_task(&tx, &task).await?;
            audit::task_event(
                &tx,
                id,
                task.project_id,
                user.id,
                "task_restored",
                task.id,
                serde_json::json!({ "title": task.title }),
            )
            .await?;
            ("task", task.title)
        }
        TrashKind::Label => {
            let label = labels::Entity::find_by_id(item_id)
                .one_or_not_found(&tx)
                .await?;
            if label.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_label(&tx, item_id).await?;
            ("label", label.name)
        }
        TrashKind::Page => {
            let page = wiki_pages::Entity::find_by_id(item_id)
                .one_or_not_found(&tx)
                .await?;
            if page.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_page(&tx, &page).await?;
            ("page", page.title)
        }
        TrashKind::File => {
            let file = files::Entity::find_by_id(item_id)
                .one_or_not_found(&tx)
                .await?;
            if file.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_file(&tx, item_id).await?;
            ("file", file.name)
        }
        TrashKind::Sprint => {
            let sprint = sprints::Entity::find_by_id(item_id)
                .one_or_not_found(&tx)
                .await?;
            if sprint.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            trash::restore_sprint(&tx, &sprint).await?;
            ("sprint", sprint.name)
        }
        TrashKind::View => {
            let view = saved_views::Entity::find_by_id(item_id)
                .one_or_not_found(&tx)
                .await?;
            if view.deleted_at.is_none() || !crate::routes::saved_views::visible_to(&view, &user) {
                return Err(ApiError::not_found());
            }
            if projects::Entity::find_by_id(view.project_id)
                .one_or_not_found(&tx)
                .await?
                .deleted_at
                .is_some()
            {
                return Err(ApiError::conflict("restore its project first"));
            }
            saved_views::ActiveModel {
                id: Unchanged(item_id),
                deleted_at: Set(None::<DateTime>),
                ..Default::default()
            }
            .update(&tx)
            .await?;
            ("view", view.name)
        }
        TrashKind::Comment => {
            let comment = comments::Entity::find_by_id(item_id)
                .one_or_not_found(&tx)
                .await?;
            if comment.deleted_at.is_none() {
                return Err(ApiError::not_found());
            }
            if tasks::Entity::find_by_id(comment.task_id)
                .one_or_not_found(&tx)
                .await?
                .deleted_at
                .is_some()
            {
                return Err(ApiError::conflict("restore its task first"));
            }
            comments::ActiveModel {
                id: Unchanged(item_id),
                deleted_at: Set(None::<DateTime>),
                ..Default::default()
            }
            .update(&tx)
            .await?;
            ("comment", String::new())
        }
    };
    audit::restored(&tx, user.id, id, target_type, item_id, &title).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
