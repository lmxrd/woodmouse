//! Version history of pages and tasks: list, preview and restore.

use crate::db::Db;
use crate::db::prelude::*;
use crate::time::DateTime;
use axum::Json;
use axum::extract::{Path, State};
use entity::revisions as revision_rows;
use entity::{comments, tasks, users};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit;
use crate::auth::CurrentUser;
use crate::blocks::BlockData;
use crate::error::ApiError;
use crate::revisions::{self, CommentContent, PageContent, TaskContent};
use crate::routes::auth::PublicUser;
use crate::routes::pages::{PageDetail, detail};
use crate::routes::tasks::{TaskResponse, hydrate_tasks};
use crate::state::AppState;
use crate::{pages, time};
use entity::{Revision, Role, User, WikiPage};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_page_revisions))
        .routes(routes!(get_page_revision))
        .routes(routes!(restore_page_revision))
        .routes(routes!(list_task_revisions))
        .routes(routes!(get_task_revision))
        .routes(routes!(restore_task_revision))
        .routes(routes!(list_comment_revisions))
}

/// One version, newest first in lists. A revision holds the content as it was after the change.
#[derive(Serialize, ToSchema)]
pub struct RevisionSummary {
    pub id: uuid::Uuid,
    /// When this content was last saved.
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub saved_at: DateTime,
    pub saved_by: Option<PublicUser>,
    /// The page's or task's title in this version (empty when redacted).
    pub title: String,
    /// An admin removed this version's content.
    pub redacted: bool,
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

async fn author(db: &impl Db, id: uuid::Uuid) -> Option<PublicUser> {
    users::Entity::find_by_id(id)
        .one_or_not_found(db)
        .await
        .ok()
        .map(|u| PublicUser::from(&u))
}

async fn summary(db: &impl Db, r: &Revision, title: String) -> RevisionSummary {
    RevisionSummary {
        id: r.id,
        saved_at: r.updated_at,
        saved_by: author(db, r.actor_id).await,
        title,
        redacted: r.redacted_at.is_some(),
    }
}

async fn page_summary(db: &impl Db, r: &Revision) -> RevisionSummary {
    let title = revisions::content::<PageContent>(r)
        .map(|c| c.title)
        .unwrap_or_default();
    summary(db, r, title).await
}

async fn task_summary(db: &impl Db, r: &Revision) -> RevisionSummary {
    let title = revisions::content::<TaskContent>(r)
        .map(|c| c.title)
        .unwrap_or_default();
    summary(db, r, title).await
}

async fn load_revision(db: &impl Db, id: uuid::Uuid, kind: &str) -> Result<Revision, ApiError> {
    let revision = revision_rows::Entity::find_by_id(id)
        .one_or_not_found(db)
        .await?;
    if revision.target_type != kind {
        return Err(ApiError::not_found());
    }
    Ok(revision)
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
    let db = state.db();
    access::require_page_access(db, &user, id, Role::Read).await?;
    let mut out = vec![];
    for r in &revisions::of(db, id).await? {
        out.push(page_summary(db, r).await);
    }
    Ok(Json(out))
}

async fn load_page_revision(
    db: &impl Db,
    user: &User,
    id: uuid::Uuid,
    min: Role,
) -> Result<(Revision, WikiPage), ApiError> {
    let revision = load_revision(db, id, revisions::PAGE).await?;
    let page = access::require_page_access(db, user, revision.target_id, min).await?;
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
    let db = state.db();
    let (revision, _) = load_page_revision(db, &user, id, Role::Read).await?;
    Ok(Json(PageRevisionDetail {
        summary: page_summary(db, &revision).await,
        blocks: revisions::content::<PageContent>(&revision)
            .map(|c| c.blocks)
            .unwrap_or_default(),
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
        let db = state.writer().await?;
        let (revision, mut page) = load_page_revision(&*db, &user, id, Role::Write).await?;
        let content = revisions::content::<PageContent>(&revision)
            .ok_or_else(|| ApiError::conflict("this version was removed by an admin"))?;
        let tx = db.begin().await?;
        pages::save_content(&tx, &mut page, user.id, &content.blocks, true).await?;
        tx.commit().await?;
        let page_id = page.id;
        (detail(&*db, page).await?, page_id)
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
    let db = state.db();
    access::require_task_access(db, &user, id, Role::Read).await?;
    let mut out = vec![];
    for r in &revisions::of(db, id).await? {
        out.push(task_summary(db, r).await);
    }
    Ok(Json(out))
}

async fn load_task_revision(
    db: &impl Db,
    user: &User,
    id: uuid::Uuid,
    min: Role,
) -> Result<(Revision, uuid::Uuid), ApiError> {
    let revision = load_revision(db, id, revisions::TASK).await?;
    let workspace = access::require_task_access(db, user, revision.target_id, min).await?;
    Ok((revision, workspace))
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
    let db = state.db();
    let (revision, _) = load_task_revision(db, &user, id, Role::Read).await?;
    Ok(Json(TaskRevisionDetail {
        summary: task_summary(db, &revision).await,
        description: revisions::content::<TaskContent>(&revision)
            .map(|c| c.description)
            .unwrap_or_default(),
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
    let db = state.writer().await?;
    let (revision, workspace_id) = load_task_revision(&*db, &user, id, Role::Write).await?;
    let content = revisions::content::<TaskContent>(&revision)
        .ok_or_else(|| ApiError::conflict("this version was removed by an admin"))?;
    let task = tasks::Entity::find_by_id(revision.target_id)
        .one_or_not_found(&*db)
        .await?;
    if task.title != content.title || task.description != content.description {
        let tx = db.begin().await?;
        tasks::ActiveModel {
            id: Unchanged(task.id),
            title: Set(content.title.clone()),
            description: Set(content.description.clone()),
            ..Default::default()
        }
        .update(&tx)
        .await?;
        let new = revisions::record_task(&tx, task.id, user.id, &content).await?;
        audit::record(
            &tx,
            audit::Entry::admin(Some(user.id), "task_edited")
                .target("task", task.id)
                .in_workspace(workspace_id)
                .data(serde_json::json!({ "revision_id": new.id, "restored_from": revision.id })),
        )
        .await?;
        tx.commit().await?;
    }
    let task = tasks::Entity::find_by_id(revision.target_id)
        .one_or_not_found(&*db)
        .await?;
    let mut responses = hydrate_tasks(&*db, vec![task]).await?;
    Ok(Json(responses.swap_remove(0)))
}

/// One version of a comment, with its text.
#[derive(Serialize, ToSchema)]
pub struct CommentRevision {
    #[serde(flatten)]
    pub summary: RevisionSummary,
    /// The comment's text in this version (empty when redacted).
    pub body: String,
}

/// The versions of a comment, newest first. Only the author can change a comment, so these are its edits.
#[utoipa::path(
    get,
    path = "/comments/{id}/revisions",
    tag = "history",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Comment id")),
    responses(
        (status = 200, description = "Versions, newest first", body = Vec<CommentRevision>),
        (status = 404, description = "No such comment, or no access")
    )
)]
pub async fn list_comment_revisions(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<CommentRevision>>, ApiError> {
    let db = state.db();
    let comment = comments::Entity::find_by_id(id)
        .one_or_not_found(db)
        .await?;
    if comment.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    access::require_task_access(db, &user, comment.task_id, Role::Read).await?;
    let mut out = vec![];
    for r in &revisions::of(db, id).await? {
        let body = revisions::content::<CommentContent>(r)
            .map(|c| c.body)
            .unwrap_or_default();
        out.push(CommentRevision {
            summary: summary(db, r, String::new()).await,
            body,
        });
    }
    Ok(Json(out))
}
