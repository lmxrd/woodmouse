//! The site admin: only the super admin gets in (everyone else sees 404). Users (enable, disable, global
//! role), deleted workspaces, the audit log, revision redaction, the integrity check and backups.

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use jiff::civil::DateTime;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::audit::{self, Entry};
use crate::auth::{self, CurrentUser};
use crate::error::ApiError;
use crate::integrity::{self, Problem};
use crate::models::{AuditLog, Revision, Role, User, Workspace};
use crate::routes::auth::PublicUser;
use crate::state::AppState;
use crate::{access, jobs, revisions, time, trash};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(admin_list_users))
        .routes(routes!(admin_update_user))
        .routes(routes!(admin_list_deleted_workspaces))
        .routes(routes!(admin_restore_workspace))
        .routes(routes!(admin_audit_log))
        .routes(routes!(admin_redact_revision))
        .routes(routes!(admin_integrity))
        .routes(routes!(admin_backup))
}

/// Every account, oldest first.
#[utoipa::path(
    get,
    path = "/admin/users",
    tag = "admin",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "All users", body = [PublicUser]),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_list_users(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<Vec<PublicUser>>, ApiError> {
    access::require_super_admin(&user)?;
    let mut users = User::all().exec(&mut state.db()).await?;
    users.sort_by_key(|u| u.id);
    Ok(Json(users.iter().map(PublicUser::from).collect()))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateUser {
    /// Enable or disable the account. Disabling ends its sessions and blocks every way in.
    pub active: Option<bool>,
    /// Access to every workspace at this role; `null` removes it.
    #[serde(default, deserialize_with = "double_option")]
    pub global_role: Option<Option<Role>>,
}

fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
}

/// Enable or disable an account, or change its global role. The super admin's own account cannot be
/// changed here.
#[utoipa::path(
    patch,
    path = "/admin/users/{id}",
    tag = "admin",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "User id")),
    request_body = UpdateUser,
    responses(
        (status = 200, description = "The updated user", body = PublicUser),
        (status = 400, description = "The super admin cannot be changed"),
        (status = 404, description = "Not the super admin, or no such user")
    )
)]
pub async fn admin_update_user(
    State(state): State<AppState>,
    CurrentUser { user: admin, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdateUser>,
) -> Result<Json<PublicUser>, ApiError> {
    access::require_super_admin(&admin)?;
    let mut db = state.writer().await;
    let mut user = User::get_by_id(&mut *db, &id).await?;
    if user.super_admin {
        return Err(ApiError::bad_request("the super admin cannot be changed"));
    }
    let (was_active, was_role) = (user.active, user.global_role);
    let mut tx = db.transaction().await?;
    let mut update = user.update();
    let mut changed = false;
    if let Some(active) = input.active.filter(|a| *a != was_active) {
        update = update.active(active);
        changed = true;
        audit::record(
            &mut tx,
            Entry::admin(
                Some(admin.id),
                if active {
                    "user_enabled"
                } else {
                    "user_disabled"
                },
            )
            .target("user", id),
        )
        .await?;
        if !active {
            auth::delete_sessions_of(&mut tx, id, None).await?;
        }
    }
    if let Some(role) = input.global_role.filter(|r| *r != was_role) {
        update = update.global_role(role);
        changed = true;
        audit::record(
            &mut tx,
            Entry::admin(Some(admin.id), "role_changed")
                .target("user", id)
                .data(serde_json::json!({ "global_role": { "from": was_role, "to": role } })),
        )
        .await?;
    }
    if changed {
        update.exec(&mut tx).await?;
    }
    tx.commit().await?;
    let user = User::get_by_id(&mut *db, &id).await?;
    Ok(Json(PublicUser::from(&user)))
}

/// A workspace in the site trash.
#[derive(Serialize, ToSchema)]
pub struct DeletedWorkspace {
    pub id: uuid::Uuid,
    pub name: String,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub deleted_at: DateTime,
}

/// Deleted workspaces, newest deletion first.
#[utoipa::path(
    get,
    path = "/admin/workspaces/deleted",
    tag = "admin",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "Deleted workspaces", body = [DeletedWorkspace]),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_list_deleted_workspaces(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<Vec<DeletedWorkspace>>, ApiError> {
    access::require_super_admin(&user)?;
    let mut out: Vec<DeletedWorkspace> = Workspace::all()
        .exec(&mut state.db())
        .await?
        .into_iter()
        .filter_map(|w| {
            Some(DeletedWorkspace {
                id: w.id,
                name: w.name,
                deleted_at: w.deleted_at?,
            })
        })
        .collect();
    out.sort_by_key(|w| std::cmp::Reverse(w.deleted_at));
    Ok(Json(out))
}

/// Bring a deleted workspace back, with everything in it.
#[utoipa::path(
    post,
    path = "/admin/workspaces/{id}/restore",
    tag = "admin",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 204, description = "Restored"),
        (status = 404, description = "Not the super admin, or not a deleted workspace")
    )
)]
pub async fn admin_restore_workspace(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    access::require_super_admin(&user)?;
    let mut db = state.writer().await;
    let workspace = Workspace::get_by_id(&mut *db, &id).await?;
    if workspace.deleted_at.is_none() {
        return Err(ApiError::not_found());
    }
    let mut tx = db.transaction().await?;
    trash::restore_workspace(&mut tx, id).await?;
    audit::restored(&mut tx, user.id, id, "workspace", id, &workspace.name).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// One entry of the audit log.
#[derive(Serialize, ToSchema)]
pub struct AuditEntry {
    pub id: uuid::Uuid,
    pub action: String,
    pub actor: Option<PublicUser>,
    pub target_type: Option<String>,
    pub target_id: Option<uuid::Uuid>,
    pub workspace_id: Option<uuid::Uuid>,
    /// `workspace` or `admin`.
    pub visibility: String,
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditFilters {
    /// Only this action (`login`, `role_changed`, ...).
    pub action: Option<String>,
    /// Only entries by this person.
    pub actor_id: Option<uuid::Uuid>,
    /// Only entries older than this; pass the last entry's `created_at` to page.
    #[serde(default, with = "time::utc_option")]
    #[param(value_type = Option<String>, format = DateTime)]
    pub before: Option<DateTime>,
    /// How many entries (default 100, at most 500).
    pub limit: Option<usize>,
}

/// The whole audit log, newest first.
#[utoipa::path(
    get,
    path = "/admin/audit",
    tag = "admin",
    security(("session_cookie" = [])),
    params(AuditFilters),
    responses(
        (status = 200, description = "Audit entries", body = [AuditEntry]),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_audit_log(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Query(filters): Query<AuditFilters>,
) -> Result<Json<Vec<AuditEntry>>, ApiError> {
    access::require_super_admin(&user)?;
    let db = &mut state.db();
    let mut query = AuditLog::all();
    if let Some(action) = filters.action {
        query = query.filter(AuditLog::fields().action().eq(action));
    }
    if let Some(actor) = filters.actor_id {
        query = query.filter(AuditLog::fields().actor_id().eq(actor));
    }
    if let Some(before) = filters.before {
        query = query.filter(AuditLog::fields().created_at().lt(before));
    }
    let entries: Vec<AuditLog> = query
        .order_by(AuditLog::fields().created_at().desc())
        .limit(filters.limit.unwrap_or(100).clamp(1, 500))
        .exec(db)
        .await?;
    let users = User::all().exec(db).await?;
    Ok(Json(
        entries
            .into_iter()
            .map(|e| AuditEntry {
                actor: e
                    .actor_id
                    .and_then(|id| users.iter().find(|u| u.id == id))
                    .map(PublicUser::from),
                visibility: match e.visibility {
                    crate::models::Visibility::Workspace => "workspace",
                    crate::models::Visibility::Admin => "admin",
                }
                .to_string(),
                data: serde_json::from_str(&e.data).unwrap_or_default(),
                id: e.id,
                action: e.action,
                target_type: e.target_type,
                target_id: e.target_id,
                workspace_id: e.workspace_id,
                created_at: e.created_at,
            })
            .collect(),
    ))
}

/// Remove a version's content (something sensitive was saved). The version and its audit entry stay.
#[utoipa::path(
    post,
    path = "/admin/revisions/{id}/redact",
    tag = "admin",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Revision id")),
    responses(
        (status = 204, description = "Redacted"),
        (status = 404, description = "Not the super admin, or no such version")
    )
)]
pub async fn admin_redact_revision(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    access::require_super_admin(&user)?;
    let mut db = state.writer().await;
    let revision = Revision::get_by_id(&mut *db, &id).await?;
    let (target_type, target_id) = (revision.target_type.clone(), revision.target_id);
    let mut tx = db.transaction().await?;
    revisions::redact(&mut tx, revision).await?;
    audit::record(
        &mut tx,
        Entry::admin(Some(user.id), "revision_redacted")
            .target("revision", id)
            .data(serde_json::json!({ "target_type": target_type, "target_id": target_id })),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Rows that point at something that does not exist.
#[utoipa::path(
    get,
    path = "/admin/integrity",
    tag = "admin",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "Orphaned rows (empty when all is well)", body = [Problem]),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_integrity(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<Vec<Problem>>, ApiError> {
    access::require_super_admin(&user)?;
    Ok(Json(integrity::check(&mut state.db()).await?))
}

#[derive(Serialize, ToSchema)]
pub struct BackupResult {
    /// Where on the server the copy was written.
    pub path: String,
}

/// Write a consistent copy of the database to the server's backup directory (`BACKUP_DIR`, default
/// `backups`). Uploaded files are not included: back up their directory too.
#[utoipa::path(
    post,
    path = "/admin/backup",
    tag = "admin",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "Backup written", body = BackupResult),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_backup(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<BackupResult>, ApiError> {
    access::require_super_admin(&user)?;
    let dir = PathBuf::from(std::env::var("BACKUP_DIR").unwrap_or_else(|_| "backups".to_string()));
    let path = jobs::backup(&state, &dir).await?;
    {
        let mut db = state.writer().await;
        audit::record(&mut *db, Entry::admin(Some(user.id), "backup_created")).await?;
    }
    Ok(Json(BackupResult {
        path: path.display().to_string(),
    }))
}
