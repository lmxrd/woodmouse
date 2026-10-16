//! Saving a wiki page's content: the one place that replaces it, so everything derived from the content
//! (references, the search index, the revision and its audit entry) is kept in step with it. Call it with
//! the transaction of the change.

use crate::audit::{self, Entry};
use crate::blocks::{self, BlockData};
use crate::error::ApiError;
use crate::models::{ContentLink, WikiPage};
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
