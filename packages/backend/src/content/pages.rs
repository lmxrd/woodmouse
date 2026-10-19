//! Saving a wiki page's content: the one place that replaces it, so everything derived from the content
//! (references, the search index, the revision and its audit entry) is kept in step with it. Call it with
//! the transaction of the change.

use crate::audit::{self, Entry};
use crate::blocks::{self, BlockData};
use crate::error::ApiError;
use crate::models::{ContentLink, File, FileRef, WikiPage};
use crate::revisions::{self, PageContent};
use crate::time;

/// The page's blocks.
pub fn blocks_of(page: &WikiPage) -> Result<Vec<BlockData>, ApiError> {
    blocks::decode(&page.content)
}

/// Replace a page's content with `new` (a no-op when nothing differs). Returns whether it changed.
/// `new_session` (a restore, a save outside live editing) always starts a new revision; otherwise saves
/// in one editing session share one.
pub async fn save_content(
    db: &mut dyn toasty::Executor,
    page: &mut WikiPage,
    editor: uuid::Uuid,
    new: &[BlockData],
    new_session: bool,
) -> Result<bool, ApiError> {
    let mut new = new.to_vec();
    blocks::trim_trailing_empty(&mut new);
    if blocks_of(page)? == new {
        return Ok(false);
    }
    let id = page.id;
    let workspace_id = page.workspace_id;
    let title = page.title.clone();
    let next_version = page.version + 1;
    page.update()
        .content(blocks::encode(&new))
        .updated_by(editor)
        .updated_at(time::now())
        .version(next_version)
        .exec(db)
        .await?;

    // The references the content makes, and its text for search.
    let old_mentions: Vec<uuid::Uuid> = ContentLink::filter_by_source_page_id(id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|l| l.target_kind == "task")
        .map(|l| l.target_id)
        .collect();
    ContentLink::filter_by_source_page_id(id)
        .delete()
        .exec(db)
        .await?;
    for (kind, target) in blocks::references(&new) {
        toasty::create!(ContentLink {
            source_page_id: id,
            target_id: target,
            target_kind: kind.to_string(),
        })
        .exec(db)
        .await?;
    }
    crate::search::index_page_body(db, id, &new).await?;

    // Newly mentioned tasks tell the people on them; the page's watchers hear that it changed.
    for (kind, target) in blocks::references(&new) {
        if kind == "task" && !old_mentions.contains(&target) {
            let audience = crate::notify::task_audience(db, target).await?;
            crate::notify::notify(
                db,
                audience,
                crate::notify::Notice {
                    workspace_id,
                    kind: "mentioned",
                    target_type: crate::notify::PAGE,
                    target_id: id,
                    actor: Some(editor),
                    data: serde_json::json!({ "title": title, "task_id": target }),
                },
            )
            .await?;
        }
    }
    let watching = crate::notify::watchers(db, crate::notify::PAGE, id).await?;
    crate::notify::notify(
        db,
        watching,
        crate::notify::Notice {
            workspace_id,
            kind: "page_changed",
            target_type: crate::notify::PAGE,
            target_id: id,
            actor: Some(editor),
            data: serde_json::json!({ "title": title }),
        },
    )
    .await?;

    // Files the content shows are attached to the page, so they count as used (never cleaned up) and
    // appear in its attachments. The reference follows the content: removing the block detaches the
    // file, while a file attached by hand stays until it is detached.
    let shown = blocks::files(&new);
    for file_id in &shown {
        let Ok(file) = File::get_by_id(db, file_id).await else {
            continue;
        };
        if file.workspace_id == workspace_id {
            attach_file(db, id, workspace_id, &title, &file, editor, true).await?;
        }
    }
    for reference in FileRef::filter_by_target_id(id).exec(db).await? {
        if reference.target_type == PAGE
            && reference.embedded
            && !shown.contains(&reference.file_id)
        {
            detach_file(db, page, reference.file_id, editor).await?;
        }
    }

    let (revision, created) = revisions::record_page(
        db,
        id,
        editor,
        &PageContent { title, blocks: new },
        new_session,
    )
    .await?;
    if created {
        audit::record(
            db,
            Entry::admin(Some(editor), "page_edited")
                .target("page", id)
                .in_workspace(workspace_id)
                .data(serde_json::json!({ "revision_id": revision.id })),
        )
        .await?;
    }
    Ok(true)
}

/// `FileRef::target_type` of a wiki page.
pub const PAGE: &str = "page";

/// Attach a library file to a page and record it in the activity feed. `embedded` marks a file the
/// page's content itself shows, whose reference then follows the content. Returns whether it was
/// newly attached (attaching twice is harmless and records nothing). Call with the transaction of the
/// change.
pub async fn attach_file(
    db: &mut dyn toasty::Executor,
    page_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    page_title: &str,
    file: &File,
    actor: uuid::Uuid,
    embedded: bool,
) -> Result<bool, ApiError> {
    let already = FileRef::filter_by_file_id(file.id)
        .exec(db)
        .await?
        .iter()
        .any(|r| r.target_type == PAGE && r.target_id == page_id);
    if already {
        return Ok(false);
    }
    toasty::create!(FileRef {
        file_id: file.id,
        target_type: PAGE.to_string(),
        target_id: page_id,
        embedded,
    })
    .exec(db)
    .await?;
    audit::record(
        db,
        Entry::feed(workspace_id, actor, "page_file_attached")
            .target("page", page_id)
            .data(
                serde_json::json!({ "title": page_title, "file": file.name, "file_id": file.id }),
            ),
    )
    .await?;
    Ok(true)
}

/// Detach a file from a page and record it. Returns whether it was attached.
pub async fn detach_file(
    db: &mut dyn toasty::Executor,
    page: &WikiPage,
    file_id: uuid::Uuid,
    actor: uuid::Uuid,
) -> Result<bool, ApiError> {
    let Some(reference) = FileRef::filter_by_file_id(file_id)
        .exec(db)
        .await?
        .into_iter()
        .find(|r| r.target_type == PAGE && r.target_id == page.id)
    else {
        return Ok(false);
    };
    reference.delete().exec(db).await?;
    let name = File::get_by_id(db, &file_id)
        .await
        .map(|f| f.name)
        .unwrap_or_default();
    audit::record(
        db,
        Entry::feed(page.workspace_id, actor, "page_file_detached")
            .target("page", page.id)
            .data(serde_json::json!({ "title": page.title, "file": name, "file_id": file_id })),
    )
    .await?;
    Ok(true)
}
