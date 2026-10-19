//! Version history of tasks (title and description), wiki pages (title and blocks) and comments (body).
//!
//! A revision holds the content *after* a change and who made it, so the newest revision is the current
//! content and says who edited it last and when. Tasks and comments save rarely: every save is a
//! revision. Pages save every few seconds while someone types, so a page's newest revision is updated in
//! place while one editing session lasts; a session ends after a pause of ten minutes or when someone
//! else edits (unless the newest revision started less than two minutes ago, so people typing at the
//! same time do not produce a revision per change of hands).
//!
//! Revisions are whole copies, not deltas: any version can be shown or restored on its own. Admins can
//! redact a revision's content; the audit log keeps the record that an edit happened.

use crate::db::Db;
use crate::db::prelude::*;
use crate::models::revisions;
use serde::{Deserialize, Serialize};

use crate::blocks::BlockData;
use crate::error::ApiError;
use crate::models::Revision;
use crate::time;

/// A pause this long (seconds) ends an editing session.
const SESSION_GAP_SECS: i64 = 600;
/// A different editor continues the newest revision if it started less than this long ago.
const SHARED_SESSION_SECS: i64 = 120;

pub const TASK: &str = "task";
pub const PAGE: &str = "page";
pub const COMMENT: &str = "comment";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TaskContent {
    pub title: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PageContent {
    pub title: String,
    pub blocks: Vec<BlockData>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CommentContent {
    pub body: String,
}

fn encode<T: Serialize>(content: &T) -> Result<String, ApiError> {
    serde_json::to_string(content).map_err(|_| ApiError::internal("encoding failed"))
}

/// Read a revision's content; `None` once redacted.
pub fn content<T: for<'de> Deserialize<'de>>(revision: &Revision) -> Option<T> {
    if revision.redacted_at.is_some() {
        return None;
    }
    serde_json::from_str(&revision.content).ok()
}

/// All revisions of something, newest first.
pub async fn of(db: &impl Db, target_id: uuid::Uuid) -> Result<Vec<Revision>, ApiError> {
    let mut revisions = revisions::Entity::find()
        .filter(revisions::Column::TargetId.eq(target_id))
        .all(db)
        .await?;
    revisions.sort_by_key(|r| std::cmp::Reverse((r.created_at, r.id)));
    Ok(revisions)
}

/// The newest revision of something: its current content and last editor.
pub async fn newest(db: &impl Db, target_id: uuid::Uuid) -> Result<Option<Revision>, ApiError> {
    Ok(of(db, target_id).await?.into_iter().next())
}

async fn insert(
    db: &impl Db,
    target_type: &'static str,
    target_id: uuid::Uuid,
    actor_id: uuid::Uuid,
    content: String,
) -> Result<Revision, ApiError> {
    let now = time::now();
    Ok(revisions::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        target_type: Set(target_type.to_string()),
        target_id: Set(target_id),
        actor_id: Set(actor_id),
        created_at: Set(now),
        updated_at: Set(now),
        content: Set(content),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

/// Record a task's text after a change (or at creation).
pub async fn record_task(
    db: &impl Db,
    task_id: uuid::Uuid,
    actor_id: uuid::Uuid,
    content: &TaskContent,
) -> Result<Revision, ApiError> {
    insert(db, TASK, task_id, actor_id, encode(content)?).await
}

/// Record a comment's body after a change (or at creation).
pub async fn record_comment(
    db: &impl Db,
    comment_id: uuid::Uuid,
    actor_id: uuid::Uuid,
    body: &str,
) -> Result<Revision, ApiError> {
    insert(
        db,
        COMMENT,
        comment_id,
        actor_id,
        encode(&CommentContent {
            body: body.to_string(),
        })?,
    )
    .await
}

/// Record a page's content after a save. Within one editing session the newest revision is updated in
/// place; `new_session` (a restore, a save outside live editing) always starts a new one. Returns the
/// revision and whether it is a new one.
pub async fn record_page(
    db: &impl Db,
    page_id: uuid::Uuid,
    actor_id: uuid::Uuid,
    content: &PageContent,
    new_session: bool,
) -> Result<(Revision, bool), ApiError> {
    let now = time::now();
    let text = encode(content)?;
    if !new_session && let Some(mut newest) = newest(db, page_id).await? {
        let paused = time::seconds_between(newest.updated_at, now) >= SESSION_GAP_SECS;
        let other_editor = newest.actor_id != actor_id
            && time::seconds_between(newest.created_at, now) >= SHARED_SESSION_SECS;
        if !paused && !other_editor && newest.redacted_at.is_none() {
            revisions::ActiveModel {
                id: Unchanged(newest.id),
                content: Set(text.clone()),
                updated_at: Set(now),
                ..Default::default()
            }
            .update(db)
            .await?;
            newest.content = text;
            newest.updated_at = now;
            return Ok((newest, false));
        }
    }
    Ok((insert(db, PAGE, page_id, actor_id, text).await?, true))
}

/// Remove a revision's content (something sensitive was saved). The revision stays, marked redacted.
pub async fn redact(db: &impl Db, mut revision: Revision) -> Result<Revision, ApiError> {
    let now = time::now();
    revisions::ActiveModel {
        id: Unchanged(revision.id),
        content: Set(String::new()),
        redacted_at: Set(Some(now)),
        ..Default::default()
    }
    .update(db)
    .await?;
    revision.content = String::new();
    revision.redacted_at = Some(now);
    Ok(revision)
}
