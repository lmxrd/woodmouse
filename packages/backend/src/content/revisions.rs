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
pub async fn of(
    db: &mut dyn toasty::Executor,
    target_id: uuid::Uuid,
) -> Result<Vec<Revision>, ApiError> {
    let mut revisions = Revision::filter_by_target_id(target_id).exec(db).await?;
    revisions.sort_by_key(|r| std::cmp::Reverse((r.created_at, r.id)));
    Ok(revisions)
}

/// The newest revision of something: its current content and last editor.
pub async fn newest(
    db: &mut dyn toasty::Executor,
    target_id: uuid::Uuid,
) -> Result<Option<Revision>, ApiError> {
    Ok(of(db, target_id).await?.into_iter().next())
}

async fn insert(
    db: &mut dyn toasty::Executor,
    target_type: &'static str,
    target_id: uuid::Uuid,
    actor_id: uuid::Uuid,
    content: String,
) -> Result<Revision, ApiError> {
    let now = time::now();
    Ok(toasty::create!(Revision {
        target_type: target_type.to_string(),
        target_id,
        actor_id,
        created_at: now,
        updated_at: now,
        content,
    })
    .exec(db)
    .await?)
}

/// Record a task's text after a change (or at creation).
pub async fn record_task(
    db: &mut dyn toasty::Executor,
    task_id: uuid::Uuid,
    actor_id: uuid::Uuid,
    content: &TaskContent,
) -> Result<Revision, ApiError> {
    insert(db, TASK, task_id, actor_id, encode(content)?).await
}

/// Record a comment's body after a change (or at creation).
pub async fn record_comment(
    db: &mut dyn toasty::Executor,
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
    db: &mut dyn toasty::Executor,
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
            newest
                .update()
                .content(text)
                .updated_at(now)
                .exec(db)
                .await?;
            return Ok((newest, false));
        }
    }
    Ok((insert(db, PAGE, page_id, actor_id, text).await?, true))
}

/// Remove a revision's content (something sensitive was saved). The revision stays, marked redacted.
pub async fn redact(
    db: &mut dyn toasty::Executor,
    mut revision: Revision,
) -> Result<Revision, ApiError> {
    revision
        .update()
        .content(String::new())
        .redacted_at(Some(time::now()))
        .exec(db)
        .await?;
    Ok(revision)
}
