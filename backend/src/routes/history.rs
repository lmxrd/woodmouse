//! Version history of pages and tasks: list, preview and restore.

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::blocks::BlockData;
use crate::error::ApiError;
use crate::models::{Page, PageRevision, Role, Task, TaskRevision, User};
use crate::revisions;
use crate::routes::auth::PublicUser;
use crate::routes::pages::{PageDetail, detail, write_blocks};
use crate::routes::tasks::{TaskResponse, hydrate_tasks};
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_page_revisions))
        .routes(routes!(get_page_revision))
        .routes(routes!(restore_page_revision))
        .routes(routes!(list_task_revisions))
        .routes(routes!(get_task_revision))
        .routes(routes!(restore_task_revision))
}

/// One earlier version, newest first in lists.
#[derive(Serialize, ToSchema)]
pub struct RevisionSummary {
    pub id: uuid::Uuid,
    /// Unix epoch seconds when this content was saved; absent for a task's original text.
    pub saved_at: Option<i64>,
    pub saved_by: Option<PublicUser>,
    /// The page's or task's title in this version.
    pub title: String,
}

#[derive(Serialize, ToSchema)]
pub struct PageRevisionDetail {
    #[serde(flatten)]
    pub summary: RevisionSummary,
    pub blocks: Vec<BlockData>,
}

#[derive(Serialize, ToSchema)]
pub struct TaskRevisionDetail {
    #[serde(flatten)]
    pub summary: RevisionSummary,
    pub description: String,
}

async fn author(db: &mut toasty::Db, id: Option<uuid::Uuid>) -> Option<PublicUser> {
    let id = id?;
    User::get_by_id(db, &id)
        .await
        .ok()
        .map(|u| PublicUser::from(&u))
}

async fn page_summary(db: &mut toasty::Db, r: &PageRevision) -> RevisionSummary {
    RevisionSummary {
        id: r.id,
        saved_at: Some(r.saved_at),
        saved_by: author(db, Some(r.saved_by)).await,
        title: r.title.clone(),
    }
}

async fn task_summary(db: &mut toasty::Db, r: &TaskRevision) -> RevisionSummary {
    RevisionSummary {
        id: r.id,
        saved_at: r.saved_at,
        saved_by: author(db, r.saved_by).await,
        title: r.title.clone(),
    }
}

fn page_blocks(r: &PageRevision) -> Result<Vec<BlockData>, ApiError> {
    serde_json::from_str(&r.blocks).map_err(|_| ApiError::internal("stored version is unreadable"))
}

/// The earlier versions of a page, newest first.
#[utoipa::path(
    get,
    path = "/pages/{id}/revisions",
    tag = "history",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    responses(
        (status = 200, description = "Earlier versions, newest first", body = Vec<RevisionSummary>),
        (status = 404, description = "No such page, or no access")
    )
)]
pub async fn list_page_revisions(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<RevisionSummary>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_page_access(db, &user, id, Role::Read).await?;
    let mut revisions = PageRevision::filter_by_page_id(id).exec(db).await?;
    // Ids are time-ordered, so they break ties within the same second.
    revisions.sort_by_key(|r| std::cmp::Reverse((r.created_at, r.id)));
    let mut out = vec![];
    for r in &revisions {
        out.push(page_summary(db, r).await);
    }
    Ok(Json(out))
}

async fn load_page_revision(
    db: &mut toasty::Db,
    user: &User,
    id: uuid::Uuid,
    min: Role,
) -> Result<(PageRevision, Page), ApiError> {
    let revision = PageRevision::get_by_id(db, &id).await?;
    let page = access::require_page_access(db, user, revision.page_id, min).await?;
    Ok((revision, page))
}

/// One earlier version of a page, with its content.
#[utoipa::path(
    get,
    path = "/page-revisions/{id}",
    tag = "history",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Revision id")),
    responses(
        (status = 200, description = "The version", body = PageRevisionDetail),
        (status = 404, description = "No such version, or no access")
    )
)]
pub async fn get_page_revision(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<PageRevisionDetail>, ApiError> {
    let db = &mut *state.db.lock().await;
    let (revision, _) = load_page_revision(db, &user, id, Role::Read).await?;
    Ok(Json(PageRevisionDetail {
        summary: page_summary(db, &revision).await,
        blocks: page_blocks(&revision)?,
    }))
}

/// Put an earlier version's content back. The content being replaced is kept as a version first.
#[utoipa::path(
    post,
    path = "/page-revisions/{id}/restore",
    tag = "history",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Revision id")),
    responses(
        (status = 200, description = "The page with the restored content", body = PageDetail),
        (status = 404, description = "No such version, or not enough access")
    )
)]
pub async fn restore_page_revision(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<PageDetail>, ApiError> {
    let (restored, page_id) = {
        let db = &mut *state.db.lock().await;
        let (revision, mut page) = load_page_revision(db, &user, id, Role::Write).await?;
        write_blocks(db, &mut page, user.id, &page_blocks(&revision)?, true).await?;
        let page_id = page.id;
        (detail(db, page).await?, page_id)
    };
    // Everyone editing live gets the restored content.
    state.rooms.reset(&state, page_id).await;
    Ok(Json(restored))
}

/// The earlier title and description of a task, newest first.
#[utoipa::path(
    get,
    path = "/tasks/{id}/revisions",
    tag = "history",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    responses(
        (status = 200, description = "Earlier versions, newest first", body = Vec<RevisionSummary>),
        (status = 404, description = "No such task, or no access")
    )
)]
pub async fn list_task_revisions(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<RevisionSummary>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_task_access(db, &user, id, Role::Read).await?;
    let mut revisions = TaskRevision::filter_by_task_id(id).exec(db).await?;
    // Ids are time-ordered, so they break ties within the same second.
    revisions.sort_by_key(|r| std::cmp::Reverse((r.created_at, r.id)));
    let mut out = vec![];
    for r in &revisions {
        out.push(task_summary(db, r).await);
    }
    Ok(Json(out))
}

async fn load_task_revision(
    db: &mut toasty::Db,
    user: &User,
    id: uuid::Uuid,
    min: Role,
) -> Result<TaskRevision, ApiError> {
    let revision = TaskRevision::get_by_id(db, &id).await?;
    access::require_task_access(db, user, revision.task_id, min).await?;
    Ok(revision)
}

/// One earlier title and description of a task.
#[utoipa::path(
    get,
    path = "/task-revisions/{id}",
    tag = "history",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Revision id")),
    responses(
        (status = 200, description = "The version", body = TaskRevisionDetail),
        (status = 404, description = "No such version, or no access")
    )
)]
pub async fn get_task_revision(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<TaskRevisionDetail>, ApiError> {
    let db = &mut *state.db.lock().await;
    let revision = load_task_revision(db, &user, id, Role::Read).await?;
    Ok(Json(TaskRevisionDetail {
        summary: task_summary(db, &revision).await,
        description: revision.description.clone(),
    }))
}

/// Put an earlier title and description back. The current ones are kept as a version first.
#[utoipa::path(
    post,
    path = "/task-revisions/{id}/restore",
    tag = "history",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Revision id")),
    responses(
        (status = 200, description = "The task with the restored text", body = TaskResponse),
        (status = 404, description = "No such version, or not enough access")
    )
)]
pub async fn restore_task_revision(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    let db = &mut *state.db.lock().await;
    let revision = load_task_revision(db, &user, id, Role::Write).await?;
    let mut task = Task::get_by_id(db, &revision.task_id).await?;
    if task.title != revision.title || task.description != revision.description {
        revisions::snapshot_task(db, &task, user.id, true).await?;
        task.update()
            .title(revision.title.clone())
            .description(revision.description.clone())
            .text_edited_at(Some(crate::auth::now_secs()))
            .text_edited_by(Some(user.id))
            .exec(db)
            .await?;
    }
    let task = Task::get_by_id(db, &revision.task_id).await?;
    let mut responses = hydrate_tasks(db, vec![task]).await?;
    Ok(Json(responses.swap_remove(0)))
}
