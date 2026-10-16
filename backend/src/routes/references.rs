//! What pages refer to: "referenced by" lists, and the current titles of referenced pages and tasks.

use std::collections::hash_map::Entry;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{ContentLink, List, Page, Project, Role, Task};
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(page_backlinks))
        .routes(routes!(task_backlinks))
        .routes(routes!(resolve))
}

/// A page whose content refers to something.
#[derive(Serialize, ToSchema)]
pub struct Backlink {
    pub id: uuid::Uuid,
    pub title: String,
}

async fn backlinks_of(db: &mut toasty::Db, target: uuid::Uuid) -> Result<Vec<Backlink>, ApiError> {
    let sources: Vec<uuid::Uuid> = ContentLink::filter_by_target_id(target)
        .exec(db)
        .await?
        .into_iter()
        .map(|l| l.source_page_id)
        .collect();
    if sources.is_empty() {
        return Ok(vec![]);
    }
    let mut pages: Vec<Page> = Page::all()
        .filter(in_list(Page::fields().id(), sources))
        .filter(Page::fields().deleted_at().is_none())
        .exec(db)
        .await?;
    pages.sort_by_key(|p| p.title.to_lowercase());
    Ok(pages
        .into_iter()
        .map(|p| Backlink {
            id: p.id,
            title: p.title,
        })
        .collect())
}

/// The pages that link to this page.
#[utoipa::path(
    get,
    path = "/pages/{id}/backlinks",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    responses(
        (status = 200, description = "Pages that link here", body = Vec<Backlink>),
        (status = 404, description = "No such page, or no access")
    )
)]
pub async fn page_backlinks(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<Backlink>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_page_access(db, &user, id, Role::Read).await?;
    Ok(Json(backlinks_of(db, id).await?))
}

/// The pages that mention this task.
#[utoipa::path(
    get,
    path = "/tasks/{id}/backlinks",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    responses(
        (status = 200, description = "Pages that mention the task", body = Vec<Backlink>),
        (status = 404, description = "No such task, or no access")
    )
)]
pub async fn task_backlinks(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<Backlink>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_task_access(db, &user, id, Role::Read).await?;
    Ok(Json(backlinks_of(db, id).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct ResolveRequest {
    #[serde(default)]
    pub page_ids: Vec<uuid::Uuid>,
    #[serde(default)]
    pub task_ids: Vec<uuid::Uuid>,
}

/// A referenced page or task as it is now.
#[derive(Serialize, ToSchema)]
pub struct ResolvedRef {
    pub id: uuid::Uuid,
    /// The current title.
    pub title: String,
    /// In the trash (it works again if restored).
    pub deleted: bool,
    /// Tasks only.
    pub is_completed: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct Resolved {
    pub pages: Vec<ResolvedRef>,
    pub tasks: Vec<ResolvedRef>,
}

const MAX_RESOLVE: usize = 500;

/// Look up the current titles of pages and tasks referenced from content, including deleted ones.
/// Ids that are not in this workspace are left out of the answer.
#[utoipa::path(
    post,
    path = "/workspaces/{id}/resolve",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    request_body = ResolveRequest,
    responses(
        (status = 200, description = "What was found", body = Resolved),
        (status = 400, description = "Too many ids"),
        (status = 404, description = "No such workspace, or not a member")
    )
)]
pub async fn resolve(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(workspace_id): Path<uuid::Uuid>,
    Json(input): Json<ResolveRequest>,
) -> Result<Json<Resolved>, ApiError> {
    if input.page_ids.len() > MAX_RESOLVE || input.task_ids.len() > MAX_RESOLVE {
        return Err(ApiError::bad_request("too many ids"));
    }
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, workspace_id, Role::Read).await?;

    let mut pages = vec![];
    if !input.page_ids.is_empty() {
        for p in Page::all()
            .filter(in_list(Page::fields().id(), input.page_ids))
            .exec(db)
            .await?
            .into_iter()
            .filter(|p| p.workspace_id == workspace_id)
        {
            pages.push(ResolvedRef {
                id: p.id,
                title: p.title,
                deleted: p.deleted_at.is_some(),
                is_completed: None,
            });
        }
    }

    let mut tasks = vec![];
    if !input.task_ids.is_empty() {
        let found = Task::all()
            .filter(in_list(Task::fields().id(), input.task_ids))
            .exec(db)
            .await?;
        // A task is gone when it, its list or its project is in the trash.
        let mut lists = std::collections::HashMap::new();
        let mut projects = std::collections::HashMap::new();
        for t in found {
            if let Entry::Vacant(slot) = lists.entry(t.list_id) {
                let list = List::get_by_id(db, &t.list_id).await?;
                if let Entry::Vacant(project) = projects.entry(list.project_id) {
                    project.insert(Project::get_by_id(db, &list.project_id).await?);
                }
                slot.insert(list);
            }
            let list = &lists[&t.list_id];
            let project = &projects[&list.project_id];
            if project.workspace_id != workspace_id {
                continue;
            }
            tasks.push(ResolvedRef {
                id: t.id,
                deleted: t.deleted_at.is_some()
                    || list.deleted_at.is_some()
                    || project.deleted_at.is_some(),
                is_completed: Some(t.is_completed),
                title: t.title,
            });
        }
    }
    Ok(Json(Resolved { pages, tasks }))
}
