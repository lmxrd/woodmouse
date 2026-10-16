//! Version history of page content and task text.
//!
//! A version is a full copy of what was there *before* an edit. One is kept when the edit starts a new
//! editing session: someone else is editing, or the page was untouched for ten minutes. A long unbroken
//! session also leaves one version an hour. Restoring always keeps the current content first, so a
//! restore can be undone. Versions are never deleted (a page's or task's history goes with it only if
//! the workspace is).
//!
//! Whole copies rather than deltas: pages and descriptions are a few kilobytes, a version is only taken
//! once per session, and any version can be shown or restored without replaying a chain.

use crate::blocks::BlockData;
use crate::error::ApiError;
use crate::models::{Page, PageRevision, Task, TaskRevision};

/// A pause this long (seconds) ends an editing session.
const SESSION_GAP_SECS: i64 = 600;
/// Another editor starts a new session only if the last version is at least this old, so people typing at
/// the same time do not produce a version per change of hands.
const MIN_BETWEEN_VERSIONS_SECS: i64 = 120;
/// A session this long without a version gets one anyway.
const MAX_BETWEEN_VERSIONS_SECS: i64 = 3600;

/// Keep the page's current `old` content as a version if this edit starts a new session (or `force`).
pub async fn snapshot_page(
    db: &mut toasty::Db,
    page: &Page,
    old: &[BlockData],
    editor: uuid::Uuid,
    force: bool,
) -> Result<(), ApiError> {
    if old.is_empty() {
        return Ok(());
    }
    let now = crate::auth::now_secs();
    let revisions = PageRevision::filter_by_page_id(page.id).exec(db).await?;
    let newest = revisions.iter().max_by_key(|r| (r.created_at, r.id));
    let json = serde_json::to_string(old).map_err(|_| ApiError::internal("encoding failed"))?;
    if newest.is_some_and(|r| r.blocks == json) {
        return Ok(());
    }
    let recent = newest.is_some_and(|r| now - r.created_at < MIN_BETWEEN_VERSIONS_SECS);
    let due = force
        || (!recent && (page.updated_by != editor || now - page.updated_at >= SESSION_GAP_SECS))
        || newest.is_some_and(|r| now - r.created_at >= MAX_BETWEEN_VERSIONS_SECS);
    if due {
        toasty::create!(PageRevision {
            page_id: page.id,
            saved_at: page.updated_at,
            saved_by: page.updated_by,
            title: page.title.clone(),
            blocks: json,
            created_at: now,
        })
        .exec(db)
        .await?;
    }
    Ok(())
}

/// Keep the task's current title and description as a version if this edit starts a new session
/// (the first ever edit always does, preserving the original text), or `force`.
pub async fn snapshot_task(
    db: &mut toasty::Db,
    task: &Task,
    editor: uuid::Uuid,
    force: bool,
) -> Result<(), ApiError> {
    let now = crate::auth::now_secs();
    let revisions = TaskRevision::filter_by_task_id(task.id).exec(db).await?;
    let newest = revisions.iter().max_by_key(|r| (r.created_at, r.id));
    if newest.is_some_and(|r| r.title == task.title && r.description == task.description) {
        return Ok(());
    }
    let due = force
        || newest.is_none()
        || task.text_edited_by != Some(editor)
        || task
            .text_edited_at
            .is_some_and(|at| now - at >= SESSION_GAP_SECS)
        || newest.is_some_and(|r| now - r.created_at >= MAX_BETWEEN_VERSIONS_SECS);
    if due {
        toasty::create!(TaskRevision {
            task_id: task.id,
            saved_at: task.text_edited_at,
            saved_by: task.text_edited_by,
            title: task.title.clone(),
            description: task.description.clone(),
            created_at: now,
        })
        .exec(db)
        .await?;
    }
    Ok(())
}
