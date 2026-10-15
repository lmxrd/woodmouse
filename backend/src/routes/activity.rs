//! Activity feeds for a workspace, a project, or a single task.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Activity, Role, User};
use crate::routes::auth::PublicUser;
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_activity))
}

#[derive(Serialize, ToSchema)]
pub struct ActivityView {
    pub id: uuid::Uuid,
    pub kind: String,
    pub actor: Option<PublicUser>,
    pub project_id: Option<uuid::Uuid>,
    pub task_id: Option<uuid::Uuid>,
    /// What the UI needs to phrase the entry (task title, old and new values, ...).
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
    /// Unix epoch seconds.
    pub created_at: i64,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ActivityFilters {
    /// Only this project's activity.
    pub project_id: Option<uuid::Uuid>,
    /// Only this task's activity.
    pub task_id: Option<uuid::Uuid>,
    /// Only entries older than this (Unix epoch seconds); pass the last entry's `created_at` to page.
    pub before: Option<i64>,
    /// How many entries to return (default 50, at most 200).
    pub limit: Option<usize>,
}

/// The activity of a workspace, newest first, optionally narrowed to a project or a task.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/activity",
    tag = "activity",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id"), ActivityFilters),
    responses(
        (status = 200, description = "Activity entries", body = [ActivityView]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn list_activity(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Query(filters): Query<ActivityFilters>,
) -> Result<Json<Vec<ActivityView>>, ApiError> {
    let db = &mut *state.db.lock().await;
    access::require_access(db, &user, id, Role::Read).await?;

    let mut entries: Vec<Activity> = if let Some(task_id) = filters.task_id {
        Activity::filter_by_task_id(task_id).exec(db).await?
    } else if let Some(project_id) = filters.project_id {
        Activity::filter_by_project_id(project_id).exec(db).await?
    } else {
        Activity::filter_by_workspace_id(id).exec(db).await?
    };
    // Whatever index was used, never leak another workspace's entries.
    entries.retain(|a| a.workspace_id == id);
    if let Some(before) = filters.before {
        entries.retain(|a| a.created_at < before);
    }
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    entries.truncate(filters.limit.unwrap_or(50).clamp(1, 200));

    let actor_ids: Vec<uuid::Uuid> = entries.iter().map(|a| a.actor_id).collect();
    let actors: Vec<User> = if actor_ids.is_empty() {
        vec![]
    } else {
        User::all()
            .filter(in_list(User::fields().id(), actor_ids))
            .exec(db)
            .await?
    };
    Ok(Json(
        entries
            .into_iter()
            .map(|a| ActivityView {
                id: a.id,
                kind: a.kind,
                actor: actors
                    .iter()
                    .find(|u| u.id == a.actor_id)
                    .map(PublicUser::from),
                project_id: a.project_id,
                task_id: a.task_id,
                data: serde_json::from_str(&a.data).unwrap_or(serde_json::Value::Null),
                created_at: a.created_at,
            })
            .collect(),
    ))
}
