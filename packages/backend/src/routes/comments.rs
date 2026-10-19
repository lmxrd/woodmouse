//! Comments on tasks (markdown text). Authors edit their own; authors and admins delete (softly).

use crate::db::Db;
use crate::db::prelude::*;
use crate::models::revisions as revision_rows;
use crate::models::{comments, tasks, users};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Comment, Revision, Role, Task, User};
use crate::revisions;
use crate::routes::auth::PublicUser;
use crate::state::AppState;
use crate::time;
use crate::time::DateTime;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_comments, add_comment))
        .routes(routes!(update_comment, delete_comment))
}

#[derive(Serialize, ToSchema)]
pub struct CommentView {
    pub id: uuid::Uuid,
    pub task_id: uuid::Uuid,
    pub author: Option<PublicUser>,
    /// Markdown.
    pub body: String,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime,
    /// The last edit, if it was edited (the newest of its revisions).
    #[serde(with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub edited_at: Option<DateTime>,
}

#[derive(Deserialize, ToSchema)]
pub struct CommentBody {
    /// Markdown.
    pub body: String,
}

fn validate_body(body: &str) -> Result<String, ApiError> {
    let body = body.trim();
    if body.is_empty() {
        return Err(ApiError::bad_request("comment cannot be empty"));
    }
    Ok(body.to_string())
}

async fn views(db: &impl Db, comments: Vec<Comment>) -> Result<Vec<CommentView>, ApiError> {
    let authors = comments.load_one(users::Entity, db).await?;
    let comment_ids: Vec<uuid::Uuid> = comments.iter().map(|c| c.id).collect();
    let revision_rows: Vec<Revision> = if comment_ids.is_empty() {
        vec![]
    } else {
        revision_rows::Entity::find()
            .filter(revision_rows::Column::TargetId.is_in(comment_ids.iter().copied()))
            .all(db)
            .await?
    };
    Ok(comments
        .into_iter()
        .zip(authors)
        .map(|(c, author)| {
            let mine: Vec<&Revision> = revision_rows
                .iter()
                .filter(|r| r.target_type == revisions::COMMENT && r.target_id == c.id)
                .collect();
            let edited_at = (mine.len() > 1)
                .then(|| mine.iter().map(|r| r.created_at).max())
                .flatten();
            (c, author, edited_at)
        })
        .map(|(c, author, edited_at)| CommentView {
            id: c.id,
            task_id: c.task_id,
            author: author.as_ref().map(PublicUser::from),
            body: c.body,
            created_at: c.created_at,
            edited_at,
        })
        .collect())
}

/// A comment the caller may see (its task is live and readable); returns the workspace id.
async fn load(
    db: &impl Db,
    user: &User,
    id: uuid::Uuid,
    min_role: Role,
) -> Result<(Comment, uuid::Uuid), ApiError> {
    let comment = comments::Entity::find_by_id(id)
        .one_or_not_found(db)
        .await?;
    if comment.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    let workspace_id = access::require_task_access(db, user, comment.task_id, min_role).await?;
    Ok((comment, workspace_id))
}

/// The comments of a task, oldest first.
#[utoipa::path(
    get,
    path = "/tasks/{id}/comments",
    tag = "comments",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    responses(
        (status = 200, description = "The task's comments", body = [CommentView]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No task with that id, or no access")
    )
)]
pub async fn list_comments(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<CommentView>>, ApiError> {
    let db = state.db();
    access::require_task_access(db, &user, id, Role::Read).await?;
    let comments: Vec<Comment> = comments::Entity::find()
        .filter(comments::Column::TaskId.eq(id))
        .filter(comments::Column::DeletedAt.is_null())
        .order_by_asc(comments::Column::CreatedAt)
        .all(db)
        .await?;
    Ok(Json(views(db, comments).await?))
}

/// Comment on a task.
#[utoipa::path(
    post,
    path = "/tasks/{id}/comments",
    tag = "comments",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    request_body = CommentBody,
    responses(
        (status = 201, description = "Comment added", body = CommentView),
        (status = 400, description = "Empty comment"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No task with that id, or no access")
    )
)]
pub async fn add_comment(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CommentBody>,
) -> Result<(StatusCode, Json<CommentView>), ApiError> {
    let body = validate_body(&input.body)?;
    let db = state.writer().await?;
    let (task, workspace_id) = access::require_task(&*db, &user, id, Role::Write).await?;
    let tx = db.begin().await?;
    let comment = comments::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        task_id: Set(id),
        author_id: Set(user.id),
        body: Set(body),
        created_at: Set(time::now()),
        ..Default::default()
    }
    .insert(&tx)
    .await?;
    revisions::record_comment(&tx, comment.id, user.id, &comment.body).await?;
    audit::task_event(
        &tx,
        workspace_id,
        task.project_id,
        user.id,
        "comment_added",
        id,
        serde_json::json!({ "title": task.title, "preview": comment.body.chars().take(120).collect::<String>() }),
    )
    .await?;
    // The people on the task, and those who commented on it before, hear about the new comment.
    let mut audience = crate::notify::task_audience(&tx, id).await?;
    audience.extend(crate::notify::commenters(&tx, id).await?);
    crate::notify::notify(
        &tx,
        audience,
        crate::notify::Notice {
            workspace_id,
            kind: "comment_added",
            target_type: crate::notify::TASK,
            target_id: id,
            actor: Some(user.id),
            data: serde_json::json!({
                "title": task.title,
                "project_id": task.project_id,
                "preview": comment.body.chars().take(120).collect::<String>(),
            }),
        },
    )
    .await?;
    tx.commit().await?;
    let mut out = views(&*db, vec![comment]).await?;
    Ok((StatusCode::CREATED, Json(out.swap_remove(0))))
}

/// Edit your own comment.
#[utoipa::path(
    patch,
    path = "/comments/{id}",
    tag = "comments",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Comment id")),
    request_body = CommentBody,
    responses(
        (status = 200, description = "Comment updated", body = CommentView),
        (status = 400, description = "Empty comment"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such comment, no access, or not yours")
    )
)]
pub async fn update_comment(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CommentBody>,
) -> Result<Json<CommentView>, ApiError> {
    let body = validate_body(&input.body)?;
    let db = state.writer().await?;
    let (comment, workspace_id) = load(&*db, &user, id, Role::Write).await?;
    if comment.author_id != user.id {
        return Err(ApiError::not_found());
    }
    if comment.body != body {
        let tx = db.begin().await?;
        comments::ActiveModel {
            id: Unchanged(comment.id),
            body: Set(body.clone()),
            ..Default::default()
        }
        .update(&tx)
        .await?;
        let revision = revisions::record_comment(&tx, id, user.id, &body).await?;
        audit::record(
            &tx,
            Entry::admin(Some(user.id), "comment_edited")
                .target("comment", id)
                .in_workspace(workspace_id)
                .data(serde_json::json!({ "revision_id": revision.id })),
        )
        .await?;
        tx.commit().await?;
    }
    let comment = comments::Entity::find_by_id(id)
        .one_or_not_found(&*db)
        .await?;
    let mut out = views(&*db, vec![comment]).await?;
    Ok(Json(out.swap_remove(0)))
}

/// Delete a comment (soft delete: it moves to Trash). Authors and workspace admins may delete.
#[utoipa::path(
    delete,
    path = "/comments/{id}",
    tag = "comments",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Comment id")),
    responses(
        (status = 204, description = "Comment deleted"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such comment, no access, or not yours")
    )
)]
pub async fn delete_comment(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let db = state.writer().await?;
    let (comment, workspace_id) = load(&*db, &user, id, Role::Write).await?;
    if comment.author_id != user.id
        && access::require_access(&*db, &user, workspace_id, Role::Admin)
            .await
            .is_err()
    {
        return Err(ApiError::not_found());
    }
    let tx = db.begin().await?;
    comments::ActiveModel {
        id: Unchanged(id),
        deleted_at: Set(Some(time::now())),
        ..Default::default()
    }
    .update(&tx)
    .await?;
    audit::deleted(&tx, user.id, workspace_id, "comment", id, "").await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Used by Trash: the task a comment belongs to (live or not), for restore checks.
pub(crate) async fn comment_task(db: &impl Db, id: uuid::Uuid) -> Result<Task, ApiError> {
    let comment = comments::Entity::find_by_id(id)
        .one_or_not_found(db)
        .await?;
    tasks::Entity::find_by_id(comment.task_id)
        .one_or_not_found(db)
        .await
}
