//! Saved views: named view settings for a project, private or shared with the workspace.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{Role, SavedView, User};
use crate::routes::auth::PublicUser;
use crate::state::AppState;
use crate::time;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_views, create_view))
        .routes(routes!(update_view, delete_view))
}

const MAX_PARAMS_BYTES: usize = 4096;

#[derive(Serialize, ToSchema)]
pub struct SavedViewResponse {
    pub id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub name: String,
    /// The view settings: a JSON object with the project URL's search-param keys.
    #[schema(value_type = Object)]
    pub params: serde_json::Value,
    /// Shared with everyone in the workspace (otherwise only the owner sees it).
    pub is_shared: bool,
    pub owner: Option<PublicUser>,
    /// Whether the caller may edit or delete it (the owner, or a workspace admin for shared views).
    pub can_edit: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateView {
    pub name: String,
    #[schema(value_type = Object)]
    pub params: serde_json::Value,
    #[serde(default)]
    pub is_shared: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateView {
    pub name: Option<String>,
    #[schema(value_type = Option<Object>)]
    pub params: Option<serde_json::Value>,
    pub is_shared: Option<bool>,
}

fn validate(name: &str, params: &serde_json::Value) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(ApiError::bad_request("name must be 1 to 80 characters"));
    }
    if !params.is_object() || params.to_string().len() > MAX_PARAMS_BYTES {
        return Err(ApiError::bad_request("params must be a small JSON object"));
    }
    Ok(name.to_string())
}

/// May this user see the view? Shared views are visible to every member; private ones to the owner.
pub(crate) fn visible_to(view: &SavedView, user: &User) -> bool {
    view.is_shared || view.owner_id == user.id
}

async fn respond(
    db: &mut dyn toasty::Executor,
    user: &User,
    workspace_id: uuid::Uuid,
    views: Vec<SavedView>,
) -> Result<Vec<SavedViewResponse>, ApiError> {
    let is_admin = access::require_access(db, user, workspace_id, Role::Admin)
        .await
        .is_ok();
    let mut out = Vec::with_capacity(views.len());
    for view in views {
        let owner = User::get_by_id(db, &view.owner_id).await.ok();
        out.push(SavedViewResponse {
            id: view.id,
            project_id: view.project_id,
            can_edit: view.owner_id == user.id || (view.is_shared && is_admin),
            owner: owner.as_ref().map(PublicUser::from),
            params: serde_json::from_str(&view.params).unwrap_or(serde_json::Value::Null),
            name: view.name,
            is_shared: view.is_shared,
        });
    }
    Ok(out)
}

/// The saved views of a project that the caller may see: the shared ones, and their own.
#[utoipa::path(
    get,
    path = "/projects/{id}/views",
    tag = "views",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Project id")),
    responses(
        (status = 200, description = "Saved views", body = [SavedViewResponse]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or no access")
    )
)]
pub async fn list_views(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<SavedViewResponse>>, ApiError> {
    let db = &mut state.db();
    let workspace_id = access::require_project_access(db, &user, id, Role::Read).await?;
    let mut views: Vec<SavedView> = SavedView::filter_by_project_id(id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|v| v.deleted_at.is_none() && visible_to(v, &user))
        .collect();
    views.sort_by_key(|v| v.name.to_lowercase());
    Ok(Json(respond(db, &user, workspace_id, views).await?))
}

/// Save view settings. Anyone who can see the project may keep a private view; sharing needs write access.
#[utoipa::path(
    post,
    path = "/projects/{id}/views",
    tag = "views",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Project id")),
    request_body = CreateView,
    responses(
        (status = 201, description = "View saved", body = SavedViewResponse),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No project with that id, or not enough access")
    )
)]
pub async fn create_view(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<CreateView>,
) -> Result<(StatusCode, Json<SavedViewResponse>), ApiError> {
    let name = validate(&input.name, &input.params)?;
    let mut db = state.writer().await;
    let min = if input.is_shared {
        Role::Write
    } else {
        Role::Read
    };
    let workspace_id = access::require_project_access(&mut *db, &user, id, min).await?;
    let view = toasty::create!(SavedView {
        workspace_id,
        project_id: id,
        owner_id: user.id,
        name,
        params: input.params.to_string(),
        is_shared: input.is_shared,
        created_at: time::now(),
    })
    .exec(&mut *db)
    .await?;
    let mut out = respond(&mut *db, &user, workspace_id, vec![view]).await?;
    Ok((StatusCode::CREATED, Json(out.swap_remove(0))))
}

/// A view the caller may change: visible to them, and theirs (or shared with them as an admin).
async fn editable(
    db: &mut dyn toasty::Executor,
    user: &User,
    id: uuid::Uuid,
) -> Result<SavedView, ApiError> {
    let view = SavedView::get_by_id(db, &id).await?;
    if view.deleted_at.is_some() || !visible_to(&view, user) {
        return Err(ApiError::not_found());
    }
    access::require_project_access(db, user, view.project_id, Role::Read).await?;
    let is_admin = access::require_access(db, user, view.workspace_id, Role::Admin)
        .await
        .is_ok();
    if view.owner_id != user.id && !(view.is_shared && is_admin) {
        return Err(ApiError::not_found());
    }
    Ok(view)
}

/// Rename a view, change its settings, or share or unshare it. Sharing needs write access.
#[utoipa::path(
    patch,
    path = "/views/{id}",
    tag = "views",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Saved view id")),
    request_body = UpdateView,
    responses(
        (status = 200, description = "View updated", body = SavedViewResponse),
        (status = 400, description = "Invalid input"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such view, or not yours to change")
    )
)]
pub async fn update_view(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdateView>,
) -> Result<Json<SavedViewResponse>, ApiError> {
    let mut db = state.writer().await;
    let mut view = editable(&mut *db, &user, id).await?;
    let name = validate(
        input.name.as_deref().unwrap_or(&view.name),
        input.params.as_ref().unwrap_or(&serde_json::json!({})),
    )?;
    if input.is_shared == Some(true) {
        access::require_project_access(&mut *db, &user, view.project_id, Role::Write).await?;
    }
    let workspace_id = view.workspace_id;
    let mut update = view.update().name(name);
    if let Some(params) = &input.params {
        update = update.params(params.to_string());
    }
    if let Some(shared) = input.is_shared {
        update = update.is_shared(shared);
    }
    update.exec(&mut *db).await?;
    let mut out = respond(&mut *db, &user, workspace_id, vec![view]).await?;
    Ok(Json(out.swap_remove(0)))
}

/// Delete a view (soft delete: it moves to Trash).
#[utoipa::path(
    delete,
    path = "/views/{id}",
    tag = "views",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Saved view id")),
    responses(
        (status = 204, description = "View deleted"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such view, or not yours to delete")
    )
)]
pub async fn delete_view(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut db = state.writer().await;
    let view = editable(&mut *db, &user, id).await?;
    let mut tx = db.transaction().await?;
    SavedView::filter_by_id(id)
        .update()
        .deleted_at(Some(time::now()))
        .exec(&mut tx)
        .await?;
    audit::deleted(&mut tx, user.id, view.workspace_id, "view", id, &view.name).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
