use crate::db::counts;
use crate::db::prelude::*;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use entity::{labels, task_labels, tasks};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::api::required;
use crate::audit;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::state::AppState;
use crate::{time, trash};
use entity::{Label, Role};

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
    let db = state.db();
    access::require_access(db, &user, workspace_id, Role::Read).await?;
    let labels: Vec<Label> = labels::Entity::find()
        .filter(labels::Column::WorkspaceId.eq(workspace_id))
        .filter(labels::Column::DeletedAt.is_null())
        .order_by_asc(Func::lower(Expr::col(labels::Column::Name)))
        .all(db)
        .await?;
    if labels.is_empty() {
        return Ok(Json(vec![]));
    }

    // Only count tasks that are not in the trash.
    let task_counts = counts(
        db,
        task_labels::Entity::find()
            .join(JoinType::InnerJoin, task_labels::Relation::Tasks.def())
            .filter(task_labels::Column::LabelId.is_in(labels.iter().map(|l| l.id)))
            .filter(tasks::Column::DeletedAt.is_null()),
        task_labels::Column::LabelId,
    )
    .await?;
    Ok(Json(
        labels
            .into_iter()
            .map(|l| LabelWithCount {
                task_count: task_counts.get(&l.id).copied().unwrap_or(0),
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
    let name = required(&input.name, "name")?;

    let db = state.writer().await?;
    access::require_access(&*db, &user, workspace_id, Role::Write).await?;

    let label = labels::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        workspace_id: Set(workspace_id),
        name: Set(name),
        color: Set(input.color.unwrap_or_else(|| "#71717a".to_string())),
        ..Default::default()
    }
    .insert(&*db)
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
    let db = state.writer().await?;
    let label = labels::Entity::find_by_id(id)
        .one_or_not_found(&*db)
        .await?;
    access::require_access(&*db, &user, label.workspace_id, Role::Write).await?;
    if label.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }

    let mut update = labels::ActiveModel::from(label);
    if let Some(name) = input.name {
        update.name = Set(required(&name, "name")?);
    }
    if let Some(color) = input.color {
        update.color = Set(color);
    }
    Ok(Json(update.update(&*db).await?))
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
    let db = state.writer().await?;
    let label = labels::Entity::find_by_id(id)
        .one_or_not_found(&*db)
        .await?;
    access::require_access(&*db, &user, label.workspace_id, Role::Write).await?;
    if label.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }

    // Soft delete: tasks keep the association, which reappears on restore.
    let tx = db.begin().await?;
    trash::delete_label(&tx, label.id, time::now()).await?;
    audit::deleted(
        &tx,
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
