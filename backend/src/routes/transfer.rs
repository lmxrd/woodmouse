//! Export a workspace's data as JSON (chosen parts) or tasks as CSV, and import either one.

use axum::Json;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;

use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Role, Workspace};
use crate::state::AppState;
use crate::transfer::{self, Export, ImportOptions, ImportReport, Parts, UserChoice};

/// Imports can be large documents.
const MAX_IMPORT_BYTES: usize = 20 * 1024 * 1024;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(export_workspace))
        .routes(routes!(export_tasks_csv))
        .merge(
            OpenApiRouter::new()
                .routes(routes!(import_workspace_data))
                .layer(DefaultBodyLimit::max(MAX_IMPORT_BYTES)),
        )
}

fn attachment(name: &str, content_type: &str, body: String) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type).expect("static"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    response
}

fn ids(text: &Option<String>) -> Result<Vec<uuid::Uuid>, ApiError> {
    text.as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse()
                .map_err(|_| ApiError::bad_request("invalid project id"))
        })
        .collect()
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ExportQuery {
    /// Comma separated parts: `projects`, `labels`, `wiki`, `members` (default: projects, labels, wiki).
    pub parts: Option<String>,
    /// Comma separated project ids; absent means every project.
    pub project_ids: Option<String>,
}

/// Export chosen parts of the workspace as a JSON document (labels, projects with their lists, sprints,
/// tasks, subtasks and comments, the wiki, and for admins the member list).
#[utoipa::path(
    get,
    path = "/workspaces/{id}/export",
    tag = "transfer",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id"), ExportQuery),
    responses(
        (status = 200, description = "A JSON file", body = Export, content_type = "application/json"),
        (status = 400, description = "Unknown part"),
        (status = 404, description = "No workspace with that id, or no access (members need an admin)")
    )
)]
pub async fn export_workspace(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let db = &mut state.db();
    let role = access::require_access(db, &user, id, Role::Read).await?;
    let wanted: Vec<String> = query
        .parts
        .as_deref()
        .unwrap_or("projects,labels,wiki")
        .split(',')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    if let Some(bad) = wanted
        .iter()
        .find(|p| !["projects", "labels", "wiki", "members"].contains(&p.as_str()))
    {
        return Err(ApiError::bad_request(format!("unknown part `{bad}`")));
    }
    let parts = Parts {
        projects: wanted.iter().any(|p| p == "projects"),
        labels: wanted.iter().any(|p| p == "labels"),
        wiki: wanted.iter().any(|p| p == "wiki"),
        members: wanted.iter().any(|p| p == "members"),
    };
    if parts.members && role < Role::Admin {
        return Err(ApiError::not_found());
    }
    let workspace = Workspace::get_by_id(db, &id).await?;
    let export = transfer::build_export(db, &workspace, &parts, &ids(&query.project_ids)?).await?;
    let text = serde_json::to_string_pretty(&export)
        .map_err(|_| ApiError::internal("could not write the export"))?;
    Ok(attachment(
        "woodmouse-export.json",
        "application/json",
        text,
    ))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CsvQuery {
    /// Comma separated project ids; absent means every project.
    pub project_ids: Option<String>,
}

/// Export tasks as CSV: one row per task with its project, list, sprint, parent, dates, estimates, labels
/// and assignees, enough to set the projects up again with an import.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/export/tasks.csv",
    tag = "transfer",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id"), CsvQuery),
    responses(
        (status = 200, description = "A CSV file", body = String, content_type = "text/csv"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn export_tasks_csv(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Query(query): Query<CsvQuery>,
) -> Result<Response, ApiError> {
    let db = &mut state.db();
    access::require_access(db, &user, id, Role::Read).await?;
    let workspace = Workspace::get_by_id(db, &id).await?;
    let parts = Parts {
        projects: true,
        labels: false,
        wiki: false,
        members: false,
    };
    let export = transfer::build_export(db, &workspace, &parts, &ids(&query.project_ids)?).await?;
    let csv = transfer::tasks_csv(&export.projects.unwrap_or_default())?;
    Ok(attachment(
        "woodmouse-tasks.csv",
        "text/csv; charset=utf-8",
        csv,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct ImportRequest {
    /// `csv` (tasks) or `json` (a Woodmouse export).
    pub format: String,
    /// The file's text.
    pub text: String,
    /// Only report what would happen; write nothing.
    #[serde(default)]
    pub dry_run: bool,
    /// CSV only: the project for rows that name none.
    pub project: Option<String>,
    /// What to do about each person in the file who is not a member of the workspace, by email.
    #[serde(default)]
    pub user_map: HashMap<String, UserChoice>,
}

/// Import a CSV of tasks or a Woodmouse JSON export into this workspace: everything or nothing. Projects,
/// lists, sprints and labels that do not exist are created; existing ones are matched by name. Problems in
/// the file, and people who are not workspace members and have no choice in `user_map`, are reported and
/// block the import (422 with the report). With `dry_run` nothing is written.
#[utoipa::path(
    post,
    path = "/workspaces/{id}/import",
    tag = "transfer",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    request_body = ImportRequest,
    responses(
        (status = 200, description = "What was (or would be) imported", body = ImportReport),
        (status = 400, description = "The file cannot be read at all"),
        (status = 404, description = "No workspace with that id, or not enough access"),
        (status = 422, description = "Nothing was imported: the file has problems or people need a choice", body = ImportReport)
    )
)]
pub async fn import_workspace_data(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<ImportRequest>,
) -> Result<Response, ApiError> {
    let mut parse_report = ImportReport::default();
    let data: Export = match input.format.as_str() {
        "csv" => Export {
            version: transfer::VERSION,
            projects: Some(transfer::parse_tasks_csv(
                &input.text,
                input
                    .project
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty()),
                &mut parse_report,
            )?),
            ..Default::default()
        },
        "json" => {
            let data: Export = serde_json::from_str(&input.text)
                .map_err(|e| ApiError::bad_request(format!("the JSON cannot be read: {e}")))?;
            if data.version != transfer::VERSION {
                return Err(ApiError::bad_request(format!(
                    "unsupported export version {}",
                    data.version
                )));
            }
            data
        }
        other => return Err(ApiError::bad_request(format!("unknown format `{other}`"))),
    };
    let mut db = state.writer().await;
    let role = access::require_access(&mut *db, &user, id, Role::Write).await?;
    // Unreadable rows block the import, so it only previews when there are any.
    let blocked_by_file = !parse_report.problems.is_empty();
    let options = ImportOptions {
        dry_run: input.dry_run || blocked_by_file,
        user_map: input
            .user_map
            .into_iter()
            .map(|(email, choice)| (email.trim().to_lowercase(), choice))
            .collect(),
        can_create_users: role >= Role::Admin,
    };
    let mut report = transfer::apply(&mut db, id, user.id, data, &options).await?;
    report.problems.splice(0..0, parse_report.problems);
    report.warnings.splice(0..0, parse_report.warnings);
    if !input.dry_run && blocked_by_file {
        report.dry_run = false;
        report.applied = false;
    }
    if input.dry_run {
        return Ok(Json(report).into_response());
    }
    if !report.applied {
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(report)).into_response());
    }
    let mut tx = db.transaction().await?;
    audit::record(
        &mut tx,
        Entry::admin(Some(user.id), "data_imported")
            .in_workspace(id)
            .data(serde_json::json!({
                "format": input.format,
                "tasks": report.tasks_created,
                "projects": report.projects_created,
                "pages": report.pages_created,
            })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(report).into_response())
}
