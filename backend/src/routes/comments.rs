//! Comments on tasks (markdown text). Authors edit their own; authors and admins delete (softly).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::activity::{self, Event};
use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::models::{Comment, Role, Task, User};
use crate::routes::auth::PublicUser;
use crate::state::AppState;

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
    /// Unix epoch seconds.
    pub created_at: i64,
    /// Unix epoch seconds of the last edit, if it was edited.
    pub edited_at: Option<i64>,
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

async fn views(db: &mut toasty::Db, comments: Vec<Comment>) -> Result<Vec<CommentView>, ApiError> {
    let author_ids: Vec<uuid::Uuid> = comments.iter().map(|c| c.author_id).collect();
    let authors: Vec<User> = if author_ids.is_empty() {
        vec![]
    } else {
        User::all()
            .filter(in_list(User::fields().id(), author_ids))
            .exec(db)
            .await?
    };
    Ok(comments
        .into_iter()
        .map(|c| CommentView {
            id: c.id,
            task_id: c.task_id,
            author: authors
                .iter()
                .find(|u| u.id == c.author_id)
                .map(PublicUser::from),
            body: c.body,
            created_at: c.created_at,
            edited_at: c.edited_at,
        })
        .collect())
}

/// A comment the caller may see (its task is live and readable); returns the workspace id.
async fn load(
    db: &mut toasty::Db,
    user: &User,
    id: uuid::Uuid,
    min_role: Role,
) -> Result<(Comment, uuid::Uuid), ApiError> {
    let comment = Comment::get_by_id(db, &id).await?;
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
    let db = &mut *state.db.lock().await;
    access::require_task_access(db, &user, id, Role::Read).await?;
    let mut comments: Vec<Comment> = Comment::filter_by_task_id(id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|c| c.deleted_at.is_none())
        .collect();
    comments.sort_by_key(|c| c.created_at);
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
    let db = &mut *state.db.lock().await;
    let workspace_id = access::require_task_access(db, &user, id, Role::Write).await?;
    let comment = toasty::create!(Comment {
        task_id: id,
        author_id: user.id,
        body,
        created_at: auth::now_secs(),
        edited_at: None,
    })
    .exec(db)
    .await?;

    let task = Task::get_by_id(db, &id).await?;
    let project_id = crate::models::List::get_by_id(db, &task.list_id)
        .await?
        .project_id;
    activity::record(
        db,
        Event {
            workspace_id,
            project_id: Some(project_id),
            task_id: Some(id),
            actor_id: user.id,
            kind: "comment_added",
            data: serde_json::json!({ "title": task.title, "preview": comment.body.chars().take(120).collect::<String>() }),
        },
    )
    .await;
    let mut out = views(db, vec![comment]).await?;
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
    let db = &mut *state.db.lock().await;
    let (mut comment, _) = load(db, &user, id, Role::Write).await?;
    if comment.author_id != user.id {
        return Err(ApiError::not_found());
    }
    if comment.body != body {
        comment
            .update()
            .body(body)
            .edited_at(Some(auth::now_secs()))
            .exec(db)
            .await?;
    }
    let mut out = views(db, vec![comment]).await?;
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
    let db = &mut *state.db.lock().await;
    let (comment, workspace_id) = load(db, &user, id, Role::Write).await?;
    if comment.author_id != user.id
        && access::require_access(db, &user, workspace_id, Role::Admin)
            .await
            .is_err()
    {
        return Err(ApiError::not_found());
    }
    Comment::filter_by_id(id)
        .update()
        .deleted_at(Some(auth::now_secs()))
        .deleted_by(Some(user.id))
        .exec(db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Used by Trash: the task a comment belongs to (live or not), for restore checks.
pub(crate) async fn comment_task(db: &mut toasty::Db, id: uuid::Uuid) -> Result<Task, ApiError> {
    let comment = Comment::get_by_id(db, &id).await?;
    Ok(Task::get_by_id(db, &comment.task_id).await?)
}
