//! Quick lookup of pages and tasks by title and content, used by the link and mention pickers.
//! (Full-text search over everything arrives with the search chunk; this ranks in memory, which is
//! plenty for one workspace.)

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::blocks::{BlockData, BlockKind};
use crate::error::ApiError;
use crate::models::{Block, List, Page, Project, Role, Task};
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(search))
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    Page,
    Task,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchParams {
    /// What to look for; empty returns the most recently changed.
    pub q: Option<String>,
    /// Only pages or only tasks; both when absent.
    pub kind: Option<HitKind>,
    /// At most this many results (default 5, max 50).
    pub limit: Option<usize>,
}

#[derive(Serialize, ToSchema)]
pub struct SearchHit {
    pub kind: HitKind,
    pub id: uuid::Uuid,
    pub title: String,
    /// Tasks only: the project the task lives in (for building its link).
    pub project_id: Option<uuid::Uuid>,
    /// Tasks only.
    pub is_completed: Option<bool>,
    #[serde(skip)]
    score: i64,
    #[serde(skip)]
    recency: i64,
}

/// Title matches beat content matches; earlier and whole-word matches beat later and partial ones.
/// Every term must match somewhere, or the item is not a hit.
fn score(terms: &[String], title: &str, body: &str) -> Option<i64> {
    let title = title.to_lowercase();
    let body = body.to_lowercase();
    let mut total = 0;
    for term in terms {
        let in_title = title.find(term.as_str());
        let points = match in_title {
            Some(0) if title == *term => 1000,
            Some(0) => 800,
            Some(i) if title[..i].ends_with([' ', '-', '_', '/']) => 600,
            Some(_) => 400,
            None if body.contains(term.as_str()) => 100,
            None => return None,
        };
        total += points;
    }
    Some(total)
}

/// Find pages and tasks in a workspace by words in their title or content, best match first.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/search",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id"), SearchParams),
    responses(
        (status = 200, description = "Best matches first", body = Vec<SearchHit>),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such workspace, or not a member")
    )
)]
pub async fn search(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(workspace_id): Path<uuid::Uuid>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<SearchHit>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, workspace_id, Role::Read).await?;
    let limit = params.limit.unwrap_or(5).clamp(1, 50);
    let terms: Vec<String> = params
        .q
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut hits: Vec<SearchHit> = vec![];

    if params.kind != Some(HitKind::Task) {
        let pages: Vec<Page> = Page::filter_by_workspace_id(workspace_id)
            .exec(db)
            .await?
            .into_iter()
            .filter(|p| p.deleted_at.is_none())
            .collect();
        // Content is only read when there is something to match it against.
        let mut bodies: std::collections::HashMap<uuid::Uuid, String> = Default::default();
        if !terms.is_empty() && !pages.is_empty() {
            let blocks = Block::all()
                .filter(in_list(
                    Block::fields().page_id(),
                    pages.iter().map(|p| p.id).collect::<Vec<_>>(),
                ))
                .exec(db)
                .await?;
            for b in blocks {
                let (Some(kind), Ok(spans)) =
                    (BlockKind::parse(&b.kind), serde_json::from_str(&b.spans))
                else {
                    continue;
                };
                let text = BlockData {
                    kind,
                    attrs: Default::default(),
                    spans,
                }
                .plain_text();
                let body = bodies.entry(b.page_id).or_default();
                body.push_str(&text);
                body.push('\n');
            }
        }
        for p in pages {
            let body = bodies.get(&p.id).map_or("", String::as_str);
            let Some(score) = score(&terms, &p.title, body) else {
                continue;
            };
            hits.push(SearchHit {
                kind: HitKind::Page,
                id: p.id,
                title: p.title,
                project_id: None,
                is_completed: None,
                score,
                recency: p.updated_at,
            });
        }
    }

    if params.kind != Some(HitKind::Page) {
        let projects: Vec<Project> = Project::filter_by_workspace_id(workspace_id)
            .exec(db)
            .await?
            .into_iter()
            .filter(|p| p.deleted_at.is_none())
            .collect();
        if !projects.is_empty() {
            let lists: Vec<List> = List::all()
                .filter(in_list(
                    List::fields().project_id(),
                    projects.iter().map(|p| p.id).collect::<Vec<_>>(),
                ))
                .exec(db)
                .await?
                .into_iter()
                .filter(|l| l.deleted_at.is_none())
                .collect();
            if !lists.is_empty() {
                let project_of: std::collections::HashMap<_, _> =
                    lists.iter().map(|l| (l.id, l.project_id)).collect();
                let tasks = Task::all()
                    .filter(in_list(
                        Task::fields().list_id(),
                        lists.iter().map(|l| l.id).collect::<Vec<_>>(),
                    ))
                    .filter(Task::fields().deleted_at().is_none())
                    .exec(db)
                    .await?;
                for t in tasks {
                    let Some(score) = score(&terms, &t.title, &t.description) else {
                        continue;
                    };
                    hits.push(SearchHit {
                        kind: HitKind::Task,
                        id: t.id,
                        project_id: project_of.get(&t.list_id).copied(),
                        is_completed: Some(t.is_completed),
                        title: t.title,
                        score,
                        recency: t.completed_at.unwrap_or(0),
                    });
                }
            }
        }
    }

    // Best score first; ties (and the empty query) go to the most recently changed.
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(b.recency.cmp(&a.recency)));
    hits.truncate(limit);
    Ok(Json(hits))
}
