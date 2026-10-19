//! Search over pages and tasks, for the ⌘K palette and the `@` and `[[` pickers.

use crate::db::prelude::*;
use crate::models::{workspace_members, workspaces};
use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Role, Workspace};
use crate::search::{self, Kind};
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
    /// Words to find in titles and text (each matches as a prefix); empty returns the newest items.
    pub q: Option<String>,
    /// Only pages or only tasks; both when absent.
    pub kind: Option<HitKind>,
    /// At most this many results (default 5, max 50).
    pub limit: Option<usize>,
    /// Search every workspace you belong to, not just this one.
    pub all_workspaces: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct SearchHit {
    pub kind: HitKind,
    pub id: uuid::Uuid,
    pub title: String,
    /// The workspace the page or task is in.
    pub workspace_id: uuid::Uuid,
    /// The workspace's name (useful when searching several).
    pub workspace_name: String,
    /// Tasks only: the project the task lives in.
    pub project_id: Option<uuid::Uuid>,
    /// Tasks only.
    pub is_completed: Option<bool>,
    /// A piece of the matching text, when there is one.
    pub snippet: Option<String>,
}

/// Find pages and tasks by words in their title or text, best match first.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/search",
    tag = "search",
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
    let db = state.db();
    access::require_access(db, &user, workspace_id, Role::Read).await?;
    let workspaces: Vec<Workspace> = if params.all_workspaces == Some(true) {
        let all = workspaces::Entity::find().all(db).await?;
        if user.super_admin {
            all
        } else {
            let member_of: std::collections::HashSet<uuid::Uuid> =
                workspace_members::Entity::find()
                    .filter(workspace_members::Column::UserId.eq(user.id))
                    .all(db)
                    .await?
                    .into_iter()
                    .map(|m| m.workspace_id)
                    .collect();
            all.into_iter()
                .filter(|w| member_of.contains(&w.id))
                .collect()
        }
    } else {
        vec![
            workspaces::Entity::find_by_id(workspace_id)
                .one_or_not_found(db)
                .await?,
        ]
    };
    let ids: Vec<uuid::Uuid> = workspaces.iter().map(|w| w.id).collect();
    let kind = params.kind.map(|k| match k {
        HitKind::Page => Kind::Page,
        HitKind::Task => Kind::Task,
    });
    let hits = search::search(
        db,
        &ids,
        params.q.as_deref().unwrap_or(""),
        kind,
        params.limit.unwrap_or(5).clamp(1, 50),
    )
    .await?;
    let name_of = |id: uuid::Uuid| {
        workspaces
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.name.clone())
            .unwrap_or_default()
    };
    Ok(Json(
        hits.into_iter()
            .map(|h| SearchHit {
                kind: if h.kind == Kind::Task {
                    HitKind::Task
                } else {
                    HitKind::Page
                },
                id: h.id,
                title: h.title,
                workspace_name: name_of(h.workspace_id),
                workspace_id: h.workspace_id,
                project_id: h.project_id,
                is_completed: h.is_completed,
                snippet: h.snippet,
            })
            .collect(),
    ))
}
