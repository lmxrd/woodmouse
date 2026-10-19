//! Activity feeds for a workspace, a project, or a single task: the workspace-visible entries of the audit
//! log.

use axum::Json;
use axum::extract::{Path, Query, State};
use jiff::civil::DateTime;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{AuditLog, Role, Visibility};
use crate::routes::auth::{People, PublicUser};
use crate::state::AppState;
use crate::time;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_activity))
}

#[derive(Serialize, ToSchema)]
pub struct ActivityView {
    pub id: uuid::Uuid,
    /// What happened (`task_created`, `comment_added`, ...).
    pub kind: String,
    pub actor: Option<PublicUser>,
    pub project_id: Option<uuid::Uuid>,
    /// The task it is about, if any.
    pub task_id: Option<uuid::Uuid>,
    /// What the UI needs to phrase the entry (task title, old and new values, ...).
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ActivityFilters {
    /// Only this project's activity.
    pub project_id: Option<uuid::Uuid>,
    /// Only this task's activity.
    pub task_id: Option<uuid::Uuid>,
    /// Only entries older than this; pass the last entry's `created_at` to page.
    #[serde(default, with = "time::utc_option")]
    #[param(value_type = Option<String>, format = DateTime)]
    pub before: Option<DateTime>,
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
    let db = &mut state.db();
    access::require_access(db, &user, id, Role::Read).await?;
    let wanted = filters.limit.unwrap_or(50).clamp(1, 200);

    // Newest first, in pages of the log, until enough entries match the project or task filter.
    let mut matched: Vec<AuditLog> = vec![];
    let mut before = filters.before;
    loop {
        let mut query = AuditLog::all()
            .filter(AuditLog::fields().workspace_id().eq(id))
            .filter(AuditLog::fields().visibility().eq(Visibility::Workspace));
        if let Some(before) = before {
            query = query.filter(AuditLog::fields().created_at().lt(before));
        }
        let mut page: Vec<AuditLog> = query
            .order_by(AuditLog::fields().created_at().desc())
            .limit(200)
            .exec(db)
            .await?;
        let exhausted = page.len() < 200;
        before = page.last().map(|e| e.created_at);
        page.retain(|e| {
            let data: serde_json::Value = serde_json::from_str(&e.data).unwrap_or_default();
            filters.task_id.is_none_or(|t| {
                e.target_type.as_deref() == Some("task") && e.target_id == Some(t)
                    || data["task_id"].as_str() == Some(&t.to_string())
            }) && filters
                .project_id
                .is_none_or(|p| data["project_id"].as_str() == Some(&p.to_string()))
        });
        matched.extend(page);
        if matched.len() >= wanted || exhausted {
            break;
        }
    }
    matched.sort_by_key(|e| std::cmp::Reverse((e.created_at, e.id)));
    matched.truncate(wanted);

    let people = People::load(db, matched.iter().filter_map(|a| a.actor_id)).await?;
    Ok(Json(
        matched
            .into_iter()
            .map(|a| {
                let data: serde_json::Value = serde_json::from_str(&a.data).unwrap_or_default();
                ActivityView {
                    id: a.id,
                    kind: a.action,
                    actor: people.public(a.actor_id),
                    project_id: data["project_id"]
                        .as_str()
                        .and_then(|s| uuid::Uuid::parse_str(s).ok()),
                    task_id: (a.target_type.as_deref() == Some("task"))
                        .then_some(a.target_id)
                        .flatten(),
                    data,
                    created_at: a.created_at,
                }
            })
            .collect(),
    ))
}
