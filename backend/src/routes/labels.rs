use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use toasty::stmt::in_list;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Label, Role, Task, TaskLabel};
use crate::state::AppState;
use crate::{time, trash};

/// All label routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_labels))
        .routes(routes!(create_label))
        .routes(routes!(update_label))
        .routes(routes!(delete_label))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateLabel {
    pub name: String,
    /// Hex color like `#71717a`. Defaults to a neutral gray.
    pub color: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateLabel {
    pub name: Option<String>,
    pub color: Option<String>,
}

/// A label with the number of tasks that carry it.
#[derive(Serialize, ToSchema)]
pub struct LabelWithCount {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub name: String,
    /// Hex color like `#71717a`.
    pub color: String,
    pub task_count: usize,
}

/// List the labels of a workspace, with how many tasks use each.
#[utoipa::path(
    get,
    path = "/workspaces/{workspace_id}/labels",
    tag = "labels",
    security(("session_cookie" = [])),
    params(("workspace_id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "The workspace's labels", body = [LabelWithCount]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn list_labels(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(workspace_id): Path<uuid::Uuid>,
) -> Result<Json<Vec<LabelWithCount>>, ApiError> {
    let db = &mut state.db();
    access::require_access(db, &user, workspace_id, Role::Read).await?;
    let mut labels: Vec<Label> = Label::filter_by_workspace_id(workspace_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|l| l.deleted_at.is_none())
        .collect();
    labels.sort_by_key(|l| l.name.to_lowercase());
    if labels.is_empty() {
        return Ok(Json(vec![]));
    }

    let label_ids: Vec<uuid::Uuid> = labels.iter().map(|l| l.id).collect();
    let uses = TaskLabel::all()
        .filter(in_list(TaskLabel::fields().label_id(), label_ids))
        .exec(db)
        .await?;
    // Only count tasks that are not in the trash.
    let live_tasks: std::collections::HashSet<uuid::Uuid> = if uses.is_empty() {
        Default::default()
    } else {
        Task::all()
            .filter(in_list(
                Task::fields().id(),
                uses.iter().map(|u| u.task_id).collect::<Vec<_>>(),
            ))
            .exec(db)
            .await?
            .into_iter()
            .filter(|t| t.deleted_at.is_none())
            .map(|t| t.id)
            .collect()
    };
    Ok(Json(
        labels
            .into_iter()
            .map(|l| LabelWithCount {
                task_count: uses
                    .iter()
                    .filter(|u| u.label_id == l.id && live_tasks.contains(&u.task_id))
                    .count(),
                id: l.id,
                workspace_id: l.workspace_id,
                name: l.name,
                color: l.color,
            })
            .collect(),
    ))
}

/// Create a label in the workspace.
#[utoipa::path(
    post,
    path = "/workspaces/{workspace_id}/labels",
    tag = "labels",
    security(("session_cookie" = [])),
    params(("workspace_id" = uuid::Uuid, Path, description = "Workspace id")),
    request_body = CreateLabel,
    responses(
        (status = 201, description = "Label created", body = Label),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn create_label(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(workspace_id): Path<uuid::Uuid>,
    Json(input): Json<CreateLabel>,
) -> Result<(StatusCode, Json<Label>), ApiError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }

    let mut db = state.writer().await;
    access::require_access(&mut *db, &user, workspace_id, Role::Write).await?;

    let label = toasty::create!(Label {
        workspace_id,
        name: name.to_string(),
        color: input.color.unwrap_or_else(|| "#71717a".to_string()),
    })
    .exec(&mut *db)
    .await?;
    Ok((StatusCode::CREATED, Json(label)))
}

/// Rename or recolor a label.
#[utoipa::path(
    patch,
    path = "/labels/{id}",
    tag = "labels",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Label id")),
    request_body = UpdateLabel,
    responses(
        (status = 200, description = "Label updated", body = Label),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No label with that id, or no access")
    )
)]
pub async fn update_label(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdateLabel>,
) -> Result<Json<Label>, ApiError> {
    let mut db = state.writer().await;
    let mut label = Label::get_by_id(&mut *db, &id).await?;
    access::require_access(&mut *db, &user, label.workspace_id, Role::Write).await?;
    if label.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }

    let mut update = label.update();
    if let Some(name) = input.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApiError::bad_request("name is required"));
        }
        update = update.name(name.to_string());
    }
    if let Some(color) = input.color {
        update = update.color(color);
    }
    update.exec(&mut *db).await?;
    Ok(Json(label))
}

/// Delete a label (detaching it from all tasks).
#[utoipa::path(
    delete,
    path = "/labels/{id}",
    tag = "labels",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Label id")),
    responses(
        (status = 204, description = "Label deleted"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No label with that id, or no access")
    )
)]
pub async fn delete_label(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut db = state.writer().await;
    let label = Label::get_by_id(&mut *db, &id).await?;
    access::require_access(&mut *db, &user, label.workspace_id, Role::Write).await?;
    if label.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }

    // Soft delete: tasks keep the association, which reappears on restore.
    let mut tx = db.transaction().await?;
    trash::delete_label(&mut tx, label.id, time::now()).await?;
    audit::deleted(
        &mut tx,
        user.id,
        label.workspace_id,
        "label",
        label.id,
        &label.name,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
