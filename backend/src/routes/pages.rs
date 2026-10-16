//! The wiki: a tree of pages per workspace, each a flat list of blocks (see [`crate::blocks`]).

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::{self, CurrentUser};
use crate::blocks::{BlockData, BlockKind, validate_blocks};
use crate::error::ApiError;
use crate::models::{Block, ContentLink, Page, Role, User};
use crate::revisions;
use crate::routes::auth::PublicUser;
use crate::state::AppState;
use crate::trash;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_pages, create_page))
        .routes(routes!(get_page, update_page, delete_page))
        .routes(routes!(save_blocks))
        .routes(routes!(ping_presence))
}

const MAX_TITLE: usize = 200;

/// One page in the tree (without its content).
#[derive(Serialize, ToSchema)]
pub struct PageSummary {
    pub id: uuid::Uuid,
    pub parent_id: Option<uuid::Uuid>,
    pub title: String,
    pub position: f64,
    /// Unix epoch seconds.
    pub updated_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct PageCrumb {
    pub id: uuid::Uuid,
    pub title: String,
}

#[derive(Serialize, ToSchema)]
pub struct PageDetail {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub parent_id: Option<uuid::Uuid>,
    pub title: String,
    pub position: f64,
    /// Unix epoch seconds.
    pub created_at: i64,
    /// Unix epoch seconds of the last change to the title or content.
    pub updated_at: i64,
    pub updated_by: Option<PublicUser>,
    /// Send this back as `base_version` when saving content.
    pub version: i64,
    /// The pages above this one, from the root down (empty for a top-level page).
    pub ancestors: Vec<PageCrumb>,
    /// The content, in order.
    pub blocks: Vec<BlockData>,
}

#[derive(Deserialize, ToSchema)]
pub struct CreatePage {
    pub title: Option<String>,
    /// The parent page; absent for a top-level page.
    pub parent_id: Option<uuid::Uuid>,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdatePage {
    pub title: Option<String>,
    /// Move under another page; `null` moves to the top level.
    #[serde(default, deserialize_with = "double_option")]
    pub parent_id: Option<Option<uuid::Uuid>>,
    /// Ordering among siblings. When moving without a position, the page goes last.
    pub position: Option<f64>,
}

#[derive(Deserialize, ToSchema)]
pub struct SaveBlocks {
    pub blocks: Vec<BlockData>,
    /// The `version` the edit started from. If the page has moved on, the save is refused (409)
    /// unless `force` is set. Omit to skip the check.
    pub base_version: Option<i64>,
    /// Overwrite whatever is saved, even if the page changed in the meantime.
    #[serde(default)]
    pub force: bool,
}

fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
}

fn clean_title(title: &str) -> Result<String, ApiError> {
    let title = title.trim();
    if title.chars().count() > MAX_TITLE {
        return Err(ApiError::bad_request(format!(
            "title can be at most {MAX_TITLE} characters"
        )));
    }
    Ok(if title.is_empty() {
        "Untitled".to_string()
    } else {
        title.to_string()
    })
}

fn block_data(block: &Block) -> Result<BlockData, ApiError> {
    let corrupt = |what: &str| {
        tracing::error!(block = %block.id, what, "stored block could not be read");
        ApiError::internal("stored page content is unreadable")
    };
    Ok(BlockData {
        kind: BlockKind::parse(&block.kind).ok_or_else(|| corrupt("kind"))?,
        attrs: serde_json::from_str(&block.attrs).map_err(|_| corrupt("attrs"))?,
        spans: serde_json::from_str(&block.spans).map_err(|_| corrupt("spans"))?,
    })
}

pub(crate) async fn blocks_of(
    db: &mut toasty::Db,
    page_id: uuid::Uuid,
) -> Result<Vec<BlockData>, ApiError> {
    let mut rows = Block::filter_by_page_id(page_id).exec(db).await?;
    rows.sort_by(|a, b| a.position.total_cmp(&b.position));
    rows.iter().map(block_data).collect()
}

pub(crate) async fn live_pages(
    db: &mut toasty::Db,
    workspace_id: uuid::Uuid,
) -> Result<Vec<Page>, ApiError> {
    Ok(Page::filter_by_workspace_id(workspace_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|p| p.deleted_at.is_none())
        .collect())
}

pub(crate) async fn detail(db: &mut toasty::Db, page: Page) -> Result<PageDetail, ApiError> {
    let pages = live_pages(db, page.workspace_id).await?;
    let by_id: HashMap<uuid::Uuid, &Page> = pages.iter().map(|p| (p.id, p)).collect();
    let mut ancestors = vec![];
    let mut cursor = page.parent_id;
    while let Some(id) = cursor {
        let Some(parent) = by_id.get(&id) else { break };
        ancestors.push(PageCrumb {
            id,
            title: parent.title.clone(),
        });
        cursor = parent.parent_id;
        if ancestors.len() > 64 {
            break; // a cycle cannot form through the API, but never loop forever on bad data
        }
    }
    ancestors.reverse();
    let updated_by = User::get_by_id(db, &page.updated_by)
        .await
        .ok()
        .map(|u| PublicUser::from(&u));
    Ok(PageDetail {
        id: page.id,
        workspace_id: page.workspace_id,
        parent_id: page.parent_id,
        title: page.title.clone(),
        position: page.position,
        created_at: page.created_at,
        updated_at: page.updated_at,
        updated_by,
        version: page.version,
        ancestors,
        blocks: blocks_of(db, page.id).await?,
    })
}

/// The workspace's pages as a flat list (parents before children are not guaranteed; build the tree from
/// `parent_id`), ordered by `position`.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/pages",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "The page tree, flattened", body = [PageSummary]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn list_pages(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<PageSummary>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Read).await?;
    let mut pages = live_pages(db, id).await?;
    pages.sort_by(|a, b| a.position.total_cmp(&b.position).then(a.id.cmp(&b.id)));
    Ok(Json(
        pages
            .into_iter()
            .map(|p| PageSummary {
                id: p.id,
                parent_id: p.parent_id,
                title: p.title,
                position: p.position,
                updated_at: p.updated_at,
            })
            .collect(),
    ))
}

/// A parent must be a live page of the same workspace.
async fn check_parent(
    db: &mut toasty::Db,
    workspace_id: uuid::Uuid,
    parent_id: uuid::Uuid,
) -> Result<(), ApiError> {
    let parent = Page::get_by_id(db, &parent_id).await?;
    if parent.deleted_at.is_some() || parent.workspace_id != workspace_id {
        return Err(ApiError::not_found());
    }
    Ok(())
}

/// Create a page at the end of its siblings.
#[utoipa::path(
    post,
    path = "/workspaces/{id}/pages",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    request_body = CreatePage,
    responses(
        (status = 201, description = "Page created", body = PageDetail),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such workspace or parent page, or not enough access")
    )
)]
pub async fn create_page(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CreatePage>,
) -> Result<(StatusCode, Json<PageDetail>), ApiError> {
    let title = clean_title(input.title.as_deref().unwrap_or(""))?;
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Write).await?;
    if let Some(parent_id) = input.parent_id {
        check_parent(db, id, parent_id).await?;
    }
    let siblings = live_pages(db, id).await?;
    let position = siblings
        .iter()
        .filter(|p| p.parent_id == input.parent_id)
        .map(|p| p.position)
        .fold(0.0, f64::max)
        + 1.0;
    let now = auth::now_secs();
    let page = toasty::create!(Page {
        workspace_id: id,
        parent_id: input.parent_id,
        title,
        position,
        created_by: user.id,
        created_at: now,
        updated_by: user.id,
        updated_at: now,
        version: 0,
    })
    .exec(db)
    .await?;
    Ok((StatusCode::CREATED, Json(detail(db, page).await?)))
}

/// A page with its content and its path from the top of the tree.
#[utoipa::path(
    get,
    path = "/pages/{id}",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    responses(
        (status = 200, description = "The page", body = PageDetail),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No page with that id, or no access")
    )
)]
pub async fn get_page(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<PageDetail>, ApiError> {
    let db = &mut *state.db.lock().await;
    let page = access::require_page_access(db, &user, id, Role::Read).await?;
    Ok(Json(detail(db, page).await?))
}

/// Rename a page, move it in the tree, or reorder it among its siblings.
#[utoipa::path(
    patch,
    path = "/pages/{id}",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    request_body = UpdatePage,
    responses(
        (status = 200, description = "Page updated", body = PageDetail),
        (status = 400, description = "Invalid input, or a page cannot go below itself"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such page or parent, or not enough access")
    )
)]
pub async fn update_page(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdatePage>,
) -> Result<Json<PageDetail>, ApiError> {
    let title = input.title.as_deref().map(clean_title).transpose()?;
    let db = &mut *state.db.lock().await;
    let mut page = access::require_page_access(db, &user, id, Role::Write).await?;
    let all = live_pages(db, page.workspace_id).await?;

    let mut position = input.position;
    let moving = input.parent_id.is_some_and(|p| p != page.parent_id);
    if let Some(Some(parent_id)) = input.parent_id {
        check_parent(db, page.workspace_id, parent_id).await?;
        if trash::page_subtree(&all, id).contains(&parent_id) {
            return Err(ApiError::bad_request("a page cannot go below itself"));
        }
    }
    if moving && position.is_none() {
        let target = input.parent_id.flatten();
        position = Some(
            all.iter()
                .filter(|p| p.parent_id == target && p.id != id)
                .map(|p| p.position)
                .fold(0.0, f64::max)
                + 1.0,
        );
    }

    if title.is_some() || input.parent_id.is_some() || position.is_some() {
        let mut update = page.update();
        if let Some(title) = &title {
            update = update
                .title(title.clone())
                .updated_by(user.id)
                .updated_at(auth::now_secs());
        }
        if let Some(parent) = input.parent_id {
            update = update.parent_id(parent);
        }
        if let Some(position) = position {
            update = update.position(position);
        }
        update.exec(db).await?;
    }
    Ok(Json(detail(db, page).await?))
}

/// Replace a page's content. Saving the content it already has changes nothing (not even `updated_at`).
#[utoipa::path(
    put,
    path = "/pages/{id}/blocks",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    request_body = SaveBlocks,
    responses(
        (status = 200, description = "The saved page", body = PageDetail),
        (status = 400, description = "A block is invalid"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No page with that id, or not enough access"),
        (status = 409, description = "The page changed since `base_version`")
    )
)]
pub async fn save_blocks(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<SaveBlocks>,
) -> Result<Json<PageDetail>, ApiError> {
    validate_blocks(&input.blocks)?;
    let (page_detail, changed) = {
        let db = &mut *state.db.lock().await;
        let mut page = access::require_page_access(db, &user, id, Role::Write).await?;

        if let Some(base) = input.base_version
            && base != page.version
            && !input.force
        {
            return Err(ApiError::conflict(
                "the page was changed by someone else since you opened it",
            ));
        }
        let before = page.version;
        write_blocks(db, &mut page, user.id, &input.blocks, false).await?;
        let changed = page.version != before;
        (detail(db, page).await?, changed)
    };
    // A save outside live editing replaces what the live document holds: everyone editing live reloads.
    if changed {
        state.rooms.reset(&state, id).await;
    }
    Ok(Json(page_detail))
}

/// Delete a page and the pages below it (soft delete: they move to Trash together).
#[utoipa::path(
    delete,
    path = "/pages/{id}",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    responses(
        (status = 204, description = "Page deleted"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No page with that id, or not enough access")
    )
)]
pub async fn delete_page(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let closing = {
        let db = &mut *state.db.lock().await;
        let page = access::require_page_access(db, &user, id, Role::Write).await?;
        let all = live_pages(db, page.workspace_id).await?;
        let closing = trash::page_subtree(&all, id);
        let mut tx = db.transaction().await?;
        trash::delete_page(&mut tx, &page, user.id, auth::now_secs()).await?;
        tx.commit().await?;
        closing
    };
    for page_id in closing {
        state
            .rooms
            .close(page_id, crate::live::close::DELETED)
            .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Another person with the page open for editing.
#[derive(Serialize, ToSchema)]
pub struct Editor {
    pub user: PublicUser,
    /// They are editing without live sync (manual saves), so their saves replace the live version.
    pub fallback: bool,
}

#[derive(Deserialize, ToSchema, Default)]
pub struct PingPresence {
    /// True when editing without live sync.
    #[serde(default)]
    pub fallback: bool,
}

/// How long after its last ping an editor still counts as present.
const PRESENCE_SECS: u64 = 30;

/// Report that you are editing this page; returns the *other* people who pinged in the last 30 seconds.
/// Clients call this every ~10 seconds while the page is open for editing.
#[utoipa::path(
    post,
    path = "/pages/{id}/presence",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    request_body(content = PingPresence, description = "Optional"),
    responses(
        (status = 200, description = "Other current editors", body = Vec<Editor>),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No page with that id, or not enough access")
    )
)]
pub async fn ping_presence(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    body: Option<Json<PingPresence>>,
) -> Result<Json<Vec<Editor>>, ApiError> {
    let fallback = body.is_some_and(|Json(b)| b.fallback);
    let db = &mut *state.db.lock().await;
    access::require_page_access(db, &user, id, Role::Write).await?;
    let others: Vec<(uuid::Uuid, bool)> = {
        let mut presence = state.presence.lock().unwrap_or_else(|e| e.into_inner());
        let now = std::time::Instant::now();
        let page = presence.entry(id).or_default();
        page.insert(user.id, (now, fallback));
        page.retain(|_, (seen, _)| now.duration_since(*seen).as_secs() < PRESENCE_SECS);
        page.iter()
            .filter(|(u, _)| **u != user.id)
            .map(|(u, (_, f))| (*u, *f))
            .collect()
    };
    let mut editors = vec![];
    for (other, fallback) in others {
        if let Ok(u) = User::get_by_id(db, &other).await {
            editors.push(Editor {
                user: PublicUser::from(&u),
                fallback,
            });
        }
    }
    Ok(Json(editors))
}

/// Replace a page's content with `blocks` (a no-op when nothing differs), keeping a version of what was
/// there when [`revisions::should_snapshot`] says so (or always, with `force_snapshot`).
pub(crate) async fn write_blocks(
    db: &mut toasty::Db,
    page: &mut Page,
    editor: uuid::Uuid,
    blocks: &[BlockData],
    force_snapshot: bool,
) -> Result<(), ApiError> {
    let mut blocks = blocks.to_vec();
    crate::blocks::trim_trailing_empty(&mut blocks);
    let blocks = &blocks[..];
    let old = blocks_of(db, page.id).await?;
    if old == blocks {
        return Ok(());
    }
    revisions::snapshot_page(db, page, &old, editor, force_snapshot).await?;
    let id = page.id;
    let mut tx = db.transaction().await?;
    Block::filter_by_page_id(id).delete().exec(&mut tx).await?;
    for (i, block) in blocks.iter().enumerate() {
        toasty::create!(Block {
            page_id: id,
            kind: block.kind.as_str().to_string(),
            position: i as f64,
            attrs: serde_json::to_string(&block.attrs)
                .map_err(|_| ApiError::internal("encoding failed"))?,
            spans: serde_json::to_string(&block.spans)
                .map_err(|_| ApiError::internal("encoding failed"))?,
        })
        .exec(&mut tx)
        .await?;
    }
    // The references the content makes, kept in step with it.
    ContentLink::filter_by_source_page_id(id)
        .delete()
        .exec(&mut tx)
        .await?;
    for (kind, target) in references(blocks) {
        toasty::create!(ContentLink {
            source_page_id: id,
            target_id: target,
            target_kind: kind.to_string(),
        })
        .exec(&mut tx)
        .await?;
    }
    tx.commit().await?;
    crate::search::index_page_body(db, id, blocks).await?;
    let next_version = page.version + 1;
    page.update()
        .updated_by(editor)
        .updated_at(auth::now_secs())
        .version(next_version)
        .exec(db)
        .await?;
    Ok(())
}

/// The distinct pages and tasks a page's content refers to, as (`"page"` or `"task"`, id).
fn references(blocks: &[BlockData]) -> Vec<(&'static str, uuid::Uuid)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    for span in blocks.iter().flat_map(|b| &b.spans) {
        if let Some(m) = &span.mention
            && seen.insert(m.task_id)
        {
            out.push(("task", m.task_id));
        }
        if let Some(p) = &span.page_link
            && seen.insert(p.page_id)
        {
            out.push(("page", p.page_id));
        }
    }
    out
}
