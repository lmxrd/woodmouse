//! Saving a wiki page's content: the one place that replaces it, so everything derived from the content
//! (references, the search index, the revision and its audit entry) is kept in step with it. Call it with
//! the transaction of the change.

use crate::audit::{self, Entry};
use crate::blocks::{self, BlockData};
use crate::db::Db;
use crate::db::prelude::*;
use crate::error::ApiError;
use crate::revisions::{self, PageContent};
use crate::time;
use entity::wiki_pages;
use entity::{File, WikiPage};
use entity::{content_links, file_refs, files};

/// The page's blocks.
pub fn blocks_of(page: &WikiPage) -> Result<Vec<BlockData>, ApiError> {
    blocks::decode(&page.content)
}

/// Replace a page's content with `new` (a no-op when nothing differs). Returns whether it changed.
/// `new_session` (a restore, a save outside live editing) always starts a new revision; otherwise saves
/// in one editing session share one.
pub async fn save_content(
    db: &impl Db,
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
    let content = blocks::encode(&new);
    let now = time::now();
    wiki_pages::ActiveModel {
        id: Unchanged(page.id),
        content: Set(content.clone()),
        updated_by: Set(editor),
        updated_at: Set(now),
        version: Set(next_version),
        ..Default::default()
    }
    .update(db)
    .await?;
    // The caller's copy of the page stays in step with the row.
    page.content = content;
    page.updated_by = editor;
    page.updated_at = now;
    page.version = next_version;

    // The references the content makes, and its text for search.
    let old_mentions: Vec<uuid::Uuid> = content_links::Entity::find()
        .filter(content_links::Column::SourcePageId.eq(id))
        .all(db)
        .await?
        .into_iter()
        .filter(|l| l.target_kind == "task")
        .map(|l| l.target_id)
        .collect();
    content_links::Entity::delete_many()
        .filter(content_links::Column::SourcePageId.eq(id))
        .exec(db)
        .await?;
    for (kind, target) in blocks::references(&new) {
        content_links::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            source_page_id: Set(id),
            target_id: Set(target),
            target_kind: Set(kind.to_string()),
        }
        .insert(db)
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
        let Ok(file) = files::Entity::find_by_id(*file_id)
            .one_or_not_found(db)
            .await
        else {
            continue;
        };
        if file.workspace_id == workspace_id {
            attach_file(db, id, workspace_id, &title, &file, editor, true).await?;
        }
    }
    for reference in file_refs::Entity::find()
        .filter(file_refs::Column::TargetId.eq(id))
        .all(db)
        .await?
    {
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
    db: &impl Db,
    page_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    page_title: &str,
    file: &File,
    actor: uuid::Uuid,
    embedded: bool,
) -> Result<bool, ApiError> {
    let already = file_refs::Entity::find()
        .filter(file_refs::Column::FileId.eq(file.id))
        .all(db)
        .await?
        .iter()
        .any(|r| r.target_type == PAGE && r.target_id == page_id);
    if already {
        return Ok(false);
    }
    file_refs::ActiveModel {
        file_id: Set(file.id),
        target_type: Set(PAGE.to_string()),
        target_id: Set(page_id),
        embedded: Set(embedded),
    }
    .insert(db)
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
    db: &impl Db,
    page: &WikiPage,
    file_id: uuid::Uuid,
    actor: uuid::Uuid,
) -> Result<bool, ApiError> {
    let Some(reference) = file_refs::Entity::find()
        .filter(file_refs::Column::FileId.eq(file_id))
        .all(db)
        .await?
        .into_iter()
        .find(|r| r.target_type == PAGE && r.target_id == page.id)
    else {
        return Ok(false);
    };
    file_refs::Entity::delete_many()
        .filter(file_refs::Column::FileId.eq(reference.file_id))
        .filter(file_refs::Column::TargetType.eq(reference.target_type))
        .filter(file_refs::Column::TargetId.eq(reference.target_id))
        .exec(db)
        .await?;
    let name = files::Entity::find_by_id(file_id)
        .one_or_not_found(db)
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
