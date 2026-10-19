use crate::db::prelude::*;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use entity::lists;
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::api::required;
use crate::audit;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::state::AppState;
use crate::{ordering, time, trash};
use entity::{List, Role};

/// All list routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_lists))
        .routes(routes!(create_list))
        .routes(routes!(get_list))
        .routes(routes!(update_list))
        .routes(routes!(delete_list))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateList {
    pub name: String,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateList {
    pub name: Option<String>,
}

/// List the lists in a project.
#[utoipa::path(
    get,
    path = "/projects/{project_id}/lists",
    tag = "lists",
    security(("session_cookie" = [])),
    params(("project_id" = uuid::Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "The project's lists", body = [List]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or no access")
    )
)]
pub async fn list_lists(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<uuid::Uuid>,
) -> Result<Json<Vec<List>>, ApiError> {
    let db = state.db();
    access::require_project_access(db, &user, project_id, Role::Read).await?;
    let lists: Vec<List> = lists::Entity::find()
        .filter(lists::Column::ProjectId.eq(project_id))
        .filter(lists::Column::DeletedAt.is_null())
        .order_by_asc(lists::Column::Position)
        .order_by_asc(lists::Column::Id)
        .all(db)
        .await?;
    Ok(Json(lists))
}

/// Create a list in a project.
#[utoipa::path(
    post,
    path = "/projects/{project_id}/lists",
    tag = "lists",
    security(("session_cookie" = [])),
    params(("project_id" = uuid::Uuid, Path, description = "Project id")),
    request_body = CreateList,
    responses(
        (status = 201, description = "List created", body = List),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or no access")
    )
)]
pub async fn create_list(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(project_id): Path<uuid::Uuid>,
    Json(input): Json<CreateList>,
) -> Result<(StatusCode, Json<List>), ApiError> {
    let name = required(&input.name, "name")?;

    let db = state.writer().await?;
    access::require_project_access(&*db, &user, project_id, Role::Write).await?;

    let position = ordering::append(
        lists::Entity::find()
            .filter(lists::Column::ProjectId.eq(project_id))
            .all(&*db)
            .await?
            .iter()
            .map(|l| l.position),
    );
    let list = lists::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        project_id: Set(project_id),
        name: Set(name),
        position: Set(position),
        ..Default::default()
    }
    .insert(&*db)
    .await?;
    Ok((StatusCode::CREATED, Json(list)))
}

/// Fetch one list.
#[utoipa::path(
    get,
    path = "/lists/{id}",
    tag = "lists",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "List id")),
    responses(
        (status = 200, description = "The list", body = List),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No list with that id, or no access")
    )
)]
pub async fn get_list(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<List>, ApiError> {
    let db = state.db();
    let (list, _) = access::require_list(db, &user, id, Role::Read).await?;
    Ok(Json(list))
}

/// Rename a list.
#[utoipa::path(
    patch,
    path = "/lists/{id}",
    tag = "lists",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "List id")),
    request_body = UpdateList,
    responses(
        (status = 200, description = "List updated", body = List),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No list with that id, or no access")
    )
)]
pub async fn update_list(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdateList>,
) -> Result<Json<List>, ApiError> {
    let db = state.writer().await?;
    let (list, _) = access::require_list(&*db, &user, id, Role::Write).await?;

    let mut update = lists::ActiveModel::from(list);
    if let Some(name) = input.name {
        update.name = Set(required(&name, "name")?);
    }
    Ok(Json(update.update(&*db).await?))
}

/// Delete a list and its tasks.
#[utoipa::path(
    delete,
    path = "/lists/{id}",
    tag = "lists",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "List id")),
    responses(
        (status = 204, description = "List deleted"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No list with that id, or no access")
    )
)]
pub async fn delete_list(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let db = state.writer().await?;
    let (list, workspace_id) = access::require_list(&*db, &user, id, Role::Write).await?;

    // Soft delete: the list and its tasks move to Trash together.
    let tx = db.begin().await?;
    trash::delete_list(&tx, &list, time::now()).await?;
    audit::deleted(&tx, user.id, workspace_id, "list", id, &list.name).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
