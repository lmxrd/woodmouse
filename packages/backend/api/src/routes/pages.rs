//! The wiki: a tree of pages per workspace, each a list of blocks stored as JSON on the page (see
//! [`crate::blocks`]).

use crate::db::Db;
use crate::db::prelude::*;
use entity::{users, wiki_pages};
use std::collections::HashMap;

use crate::time::DateTime;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::api::limited;
use crate::audit;
use crate::auth::CurrentUser;
use crate::blocks::{BlockData, validate_blocks};
use crate::error::ApiError;
use crate::revisions::{self, PageContent};
use crate::routes::auth::PublicUser;
use crate::state::AppState;
use crate::{ordering, pages, time, trash};
use entity::{Role, WikiPage};

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
    pub position: i64,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTime,
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
    pub position: i64,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime,
    /// The last change to the title or content.
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTime,
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
    #[serde(default, deserialize_with = "crate::api::double_option")]
    pub parent_id: Option<Option<uuid::Uuid>>,
    /// Place right after this sibling; `null` places first. When moving to another parent without
    /// `after_id`, the page goes last.
    #[serde(default, deserialize_with = "crate::api::double_option")]
    pub after_id: Option<Option<uuid::Uuid>>,
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

/// A page title: at most 200 characters; an empty one becomes "Untitled".
fn clean_title(title: &str) -> Result<String, ApiError> {
    let title = limited(title, "title", MAX_TITLE)?;
    Ok(if title.is_empty() {
        "Untitled".to_string()
    } else {
        title
    })
}

pub(crate) async fn live_pages(
    db: &impl Db,
    workspace_id: uuid::Uuid,
) -> Result<Vec<WikiPage>, ApiError> {
    Ok(wiki_pages::Entity::find()
        .filter(wiki_pages::Column::WorkspaceId.eq(workspace_id))
        .filter(wiki_pages::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .collect())
}

/// The ids and positions of the live children of `parent_id`, in order, leaving out `except`.
fn siblings(
    all: &[WikiPage],
    parent_id: Option<uuid::Uuid>,
    except: Option<uuid::Uuid>,
) -> Vec<(uuid::Uuid, i64)> {
    let mut found: Vec<(uuid::Uuid, i64)> = all
        .iter()
        .filter(|p| p.parent_id == parent_id && Some(p.id) != except)
        .map(|p| (p.id, p.position))
        .collect();
    found.sort_by_key(|(id, position)| (*position, *id));
    found
}

pub(crate) async fn detail(db: &impl Db, page: WikiPage) -> Result<PageDetail, ApiError> {
    let all = live_pages(db, page.workspace_id).await?;
    let by_id: HashMap<uuid::Uuid, &WikiPage> = all.iter().map(|p| (p.id, p)).collect();
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
    let updated_by = users::Entity::find_by_id(page.updated_by)
        .one_or_not_found(db)
        .await
        .ok()
        .map(|u| PublicUser::from(&u));
    let blocks = pages::blocks_of(&page)?;
    Ok(PageDetail {
        id: page.id,
        workspace_id: page.workspace_id,
        parent_id: page.parent_id,
        title: page.title,
        position: page.position,
        created_at: page.created_at,
        updated_at: page.updated_at,
        updated_by,
        version: page.version,
        ancestors,
        blocks,
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
    let db = state.db();
    access::require_access(db, &user, id, Role::Read).await?;
    let mut all = live_pages(db, id).await?;
    all.sort_by_key(|p| (p.position, p.id));
    Ok(Json(
        all.into_iter()
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
    db: &impl Db,
    workspace_id: uuid::Uuid,
    parent_id: uuid::Uuid,
) -> Result<(), ApiError> {
    let parent = wiki_pages::Entity::find_by_id(parent_id)
        .one_or_not_found(db)
        .await?;
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
    let db = state.writer().await?;
    access::require_access(&*db, &user, id, Role::Write).await?;
    if let Some(parent_id) = input.parent_id {
        check_parent(&*db, id, parent_id).await?;
    }
    let all = live_pages(&*db, id).await?;
    let position = ordering::append(
        siblings(&all, input.parent_id, None)
            .iter()
            .map(|(_, p)| *p),
    );
    let now = time::now();
    let tx = db.begin().await?;
    let page = wiki_pages::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        workspace_id: Set(id),
        parent_id: Set(input.parent_id),
        title: Set(title.clone()),
        position: Set(position),
        content: Set(crate::blocks::encode(&[])),
        created_by: Set(user.id),
        created_at: Set(now),
        updated_by: Set(user.id),
        updated_at: Set(now),
        version: Set(0),
        ..Default::default()
    }
    .insert(&tx)
    .await?;
    revisions::record_page(
        &tx,
        page.id,
        user.id,
        &PageContent {
            title,
            blocks: vec![],
        },
        true,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(detail(&*db, page).await?)))
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
    let db = state.db();
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
    let db = state.writer().await?;
    let page = access::require_page_access(&*db, &user, id, Role::Write).await?;
    let all = live_pages(&*db, page.workspace_id).await?;

    let target_parent = input.parent_id.unwrap_or(page.parent_id);
    let moving = target_parent != page.parent_id;
    if let Some(parent_id) = target_parent
        && input.parent_id.is_some()
    {
        check_parent(&*db, page.workspace_id, parent_id).await?;
        if trash::page_subtree(&all, id).contains(&parent_id) {
            return Err(ApiError::bad_request("a page cannot go below itself"));
        }
    }

    let tx = db.begin().await?;
    // Where it goes among its (new) siblings; neighbours are renumbered only when there is no room.
    let mut position: Option<i64> = None;
    if input.after_id.is_some() || moving {
        let others = siblings(&all, target_parent, Some(id));
        let placement = match input.after_id {
            Some(after) => ordering::place(&others, after),
            None => ordering::Placement {
                position: ordering::append(others.iter().map(|(_, p)| *p)),
                renumber: vec![],
            },
        };
        for (sibling, new_position) in &placement.renumber {
            wiki_pages::ActiveModel {
                id: Unchanged(*sibling),
                position: Set(*new_position),
                ..Default::default()
            }
            .update(&tx)
            .await?;
        }
        position = Some(placement.position);
    }

    let title_changed = title.as_ref().is_some_and(|t| *t != page.title);
    if title_changed || moving || position.is_some() {
        let mut update = wiki_pages::ActiveModel {
            id: Unchanged(page.id),
            ..Default::default()
        };
        if let Some(title) = &title
            && title_changed
        {
            update.title = Set(title.clone());
            update.updated_by = Set(user.id);
            update.updated_at = Set(time::now());
        }
        if input.parent_id.is_some() {
            update.parent_id = Set(target_parent);
        }
        if let Some(position) = position {
            update.position = Set(position);
        }
        update.update(&tx).await?;
    }
    if title_changed {
        revisions::record_page(
            &tx,
            id,
            user.id,
            &PageContent {
                title: title.clone().unwrap_or_default(),
                blocks: pages::blocks_of(&page)?,
            },
            false,
        )
        .await?;
    }
    tx.commit().await?;
    let page = wiki_pages::Entity::find_by_id(id)
        .one_or_not_found(&*db)
        .await?;
    Ok(Json(detail(&*db, page).await?))
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
        let db = state.writer().await?;
        let mut page = access::require_page_access(&*db, &user, id, Role::Write).await?;

        if let Some(base) = input.base_version
            && base != page.version
            && !input.force
        {
            return Err(ApiError::conflict(
                "the page was changed by someone else since you opened it",
            ));
        }
        let tx = db.begin().await?;
        let changed = pages::save_content(&tx, &mut page, user.id, &input.blocks, true).await?;
        tx.commit().await?;
        (detail(&*db, page).await?, changed)
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
        let db = state.writer().await?;
        let page = access::require_page_access(&*db, &user, id, Role::Write).await?;
        let tx = db.begin().await?;
        let closing = trash::delete_page(&tx, &page, time::now()).await?;
        audit::deleted(&tx, user.id, page.workspace_id, "page", id, &page.title).await?;
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
    let db = state.db();
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
        if let Ok(u) = users::Entity::find_by_id(other).one_or_not_found(db).await {
            editors.push(Editor {
                user: PublicUser::from(&u),
                fallback,
            });
        }
    }
    Ok(Json(editors))
}
