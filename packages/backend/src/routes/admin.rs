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
use crate::config;
use crate::error::ApiError;
use crate::integrity::{self, Problem};
use crate::models::{
    AuditLog, EmailSettings, Project, Revision, Role, User, Workspace, WorkspaceMember,
};
use crate::routes::auth::{PublicUser, validate_identity};
use crate::routes::members::issue_invite;
use crate::state::AppState;
use crate::{access, jobs, revisions, time, trash};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(admin_list_users))
        .routes(routes!(admin_create_user))
        .routes(routes!(admin_update_user))
        .routes(routes!(admin_reissue_invite))
        .routes(routes!(admin_list_workspaces))
        .routes(routes!(admin_restore_workspace))
        .routes(routes!(admin_audit_log))
        .routes(routes!(admin_redact_revision))
        .routes(routes!(admin_integrity))
        .routes(routes!(admin_backup))
        .routes(routes!(
            admin_get_email_settings,
            admin_update_email_settings
        ))
        .routes(routes!(admin_test_email))
}

/// A user as the site admin sees them.
#[derive(Serialize, ToSchema)]
pub struct AdminUser {
    #[serde(flatten)]
    pub user: PublicUser,
    /// Has not set a password yet: an invite link is waiting to be used.
    pub pending: bool,
    /// The workspaces they are a member of (not deleted ones).
    pub memberships: Vec<AdminMembership>,
}

/// One membership of a user.
#[derive(Serialize, ToSchema)]
pub struct AdminMembership {
    pub workspace_id: uuid::Uuid,
    pub workspace_name: String,
    pub role: Role,
}

impl AdminUser {
    fn new(user: &User, memberships: Vec<AdminMembership>) -> Self {
        Self {
            user: PublicUser::from(user),
            pending: user.password_hash.is_empty(),
            memberships,
        }
    }
}

/// The live-workspace memberships of `user_id`, by workspace name.
async fn memberships_of(
    db: &mut dyn toasty::Executor,
    user_id: uuid::Uuid,
) -> Result<Vec<AdminMembership>, ApiError> {
    let mut out = vec![];
    for m in WorkspaceMember::filter_by_user_id(user_id).exec(db).await? {
        let workspace = Workspace::get_by_id(db, &m.workspace_id).await?;
        if workspace.deleted_at.is_none() {
            out.push(AdminMembership {
                workspace_id: workspace.id,
                workspace_name: workspace.name,
                role: m.role,
            });
        }
    }
    out.sort_by_key(|m| m.workspace_name.to_lowercase());
    Ok(out)
}

/// Every account, oldest first.
#[utoipa::path(
    get,
    path = "/admin/users",
    tag = "admin",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "All users", body = [AdminUser]),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_list_users(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<Vec<AdminUser>>, ApiError> {
    access::require_super_admin(&user)?;
    let db = &mut state.db();
    let mut users = User::all().exec(db).await?;
    users.sort_by_key(|u| u.id);
    let mut out = vec![];
    for u in &users {
        out.push(AdminUser::new(u, memberships_of(db, u.id).await?));
    }
    Ok(Json(out))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateUser {
    #[serde(default)]
    pub name: String,
    pub email: String,
    /// Access to every workspace at this role, if any.
    pub global_role: Option<Role>,
    /// Workspaces to add them to, with their roles.
    #[serde(default)]
    pub memberships: Vec<NewMembership>,
}

#[derive(Deserialize, ToSchema)]
pub struct NewMembership {
    pub workspace_id: uuid::Uuid,
    pub role: Role,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedUser {
    pub user: AdminUser,
    /// One-time link token for `/invite/{token}`, where they choose a password.
    pub invite_token: String,
}

/// Create an account without a password; the response carries the invite link token to share.
#[utoipa::path(
    post,
    path = "/admin/users",
    tag = "admin",
    security(("session_cookie" = [])),
    request_body = CreateUser,
    responses(
        (status = 201, description = "The new user and their invite", body = CreatedUser),
        (status = 400, description = "Invalid email"),
        (status = 404, description = "Not the super admin"),
        (status = 409, description = "An account with that email exists")
    )
)]
pub async fn admin_create_user(
    State(state): State<AppState>,
    CurrentUser { user: admin, .. }: CurrentUser,
    Json(input): Json<CreateUser>,
) -> Result<(StatusCode, Json<CreatedUser>), ApiError> {
    access::require_super_admin(&admin)?;
    validate_identity(&input.email)?;
    let email = input.email.trim().to_lowercase();
    let mut db = state.writer().await;
    if User::get_by_email(&mut *db, &email).await.is_ok() {
        return Err(ApiError::conflict("an account with that email exists"));
    }
    let mut tx = db.transaction().await?;
    let created = toasty::create!(User {
        email,
        name: input.name.trim().to_string(),
        password_hash: String::new(),
        super_admin: false,
        active: true,
        global_role: input.global_role,
    })
    .exec(&mut tx)
    .await?;
    audit::record(
        &mut tx,
        Entry::admin(Some(admin.id), "user_created").target("user", created.id),
    )
    .await?;
    for m in &input.memberships {
        let workspace = Workspace::get_by_id(&mut tx, &m.workspace_id).await?;
        if workspace.deleted_at.is_some() {
            return Err(ApiError::not_found());
        }
        toasty::create!(WorkspaceMember {
            user_id: created.id,
            workspace_id: m.workspace_id,
            role: m.role,
        })
        .exec(&mut tx)
        .await?;
        audit::record(
            &mut tx,
            Entry::feed(m.workspace_id, admin.id, "member_added")
                .target("user", created.id)
                .data(serde_json::json!({ "role": m.role })),
        )
        .await?;
    }
    let invite_token = issue_invite(&mut tx, created.id).await?;
    tx.commit().await?;
    crate::mail::send_invite(
        state.mailer.clone(),
        created.email.clone(),
        created.name.clone(),
        invite_token.clone(),
    );
    let memberships = memberships_of(&mut *db, created.id).await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedUser {
            user: AdminUser::new(&created, memberships),
            invite_token,
        }),
    ))
}

#[derive(Serialize, ToSchema)]
pub struct Invite {
    pub invite_token: String,
}

/// A fresh invite link for someone who has not set a password yet (the old one stops working).
#[utoipa::path(
    post,
    path = "/admin/users/{id}/invite",
    tag = "admin",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "User id")),
    responses(
        (status = 200, description = "A new invite", body = Invite),
        (status = 404, description = "Not the super admin, or no such user"),
        (status = 409, description = "They already have a password")
    )
)]
pub async fn admin_reissue_invite(
    State(state): State<AppState>,
    CurrentUser { user: admin, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Invite>, ApiError> {
    access::require_super_admin(&admin)?;
    let mut db = state.writer().await;
    let target = User::get_by_id(&mut *db, &id).await?;
    if !target.password_hash.is_empty() {
        return Err(ApiError::conflict("this person already has a password"));
    }
    let mut tx = db.transaction().await?;
    let invite_token = issue_invite(&mut tx, id).await?;
    tx.commit().await?;
    crate::mail::send_invite(
        state.mailer.clone(),
        target.email.clone(),
        target.name.clone(),
        invite_token.clone(),
    );
    Ok(Json(Invite { invite_token }))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateUser {
    /// Enable or disable the account. Disabling ends its sessions and blocks every way in.
    pub active: Option<bool>,
    /// Access to every workspace at this role; `null` removes it.
    #[serde(default, deserialize_with = "crate::api::double_option")]
    pub global_role: Option<Option<Role>>,
    /// Display name; an empty string clears it.
    pub name: Option<String>,
    pub email: Option<String>,
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
        (status = 200, description = "The updated user", body = AdminUser),
        (status = 400, description = "The super admin cannot be changed"),
        (status = 404, description = "Not the super admin, or no such user")
    )
)]
pub async fn admin_update_user(
    State(state): State<AppState>,
    CurrentUser { user: admin, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<UpdateUser>,
) -> Result<Json<AdminUser>, ApiError> {
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
    if let Some(name) = &input.name {
        update = update.name(name.trim().to_string());
        changed = true;
    }
    if let Some(email) = &input.email {
        validate_identity(email)?;
        let email = email.trim().to_lowercase();
        if let Ok(other) = User::get_by_email(&mut tx, &email).await
            && other.id != id
        {
            return Err(ApiError::conflict("an account with that email exists"));
        }
        update = update.email(email);
        changed = true;
    }
    if changed {
        update.exec(&mut tx).await?;
    }
    tx.commit().await?;
    let user = User::get_by_id(&mut *db, &id).await?;
    let memberships = memberships_of(&mut *db, id).await?;
    Ok(Json(AdminUser::new(&user, memberships)))
}

/// A workspace as the site admin sees it.
#[derive(Serialize, ToSchema)]
pub struct AdminWorkspace {
    pub id: uuid::Uuid,
    pub name: String,
    pub description: String,
    pub members: usize,
    pub projects: usize,
    /// Set when the workspace is deleted.
    #[serde(with = "time::utc_option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub deleted_at: Option<DateTime>,
}

/// Every workspace, deleted ones included, by name.
#[utoipa::path(
    get,
    path = "/admin/workspaces",
    tag = "admin",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "All workspaces", body = [AdminWorkspace]),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_list_workspaces(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<Vec<AdminWorkspace>>, ApiError> {
    access::require_super_admin(&user)?;
    let db = &mut state.db();
    let members = WorkspaceMember::all().exec(db).await?;
    let projects = Project::all().exec(db).await?;
    let mut out: Vec<AdminWorkspace> = Workspace::all()
        .exec(db)
        .await?
        .into_iter()
        .map(|w| AdminWorkspace {
            members: members.iter().filter(|m| m.workspace_id == w.id).count(),
            projects: projects
                .iter()
                .filter(|p| p.workspace_id == w.id && p.deleted_at.is_none())
                .count(),
            id: w.id,
            name: w.name,
            description: w.description,
            deleted_at: w.deleted_at,
        })
        .collect();
    out.sort_by_key(|w| w.name.to_lowercase());
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
    let dir = config::backup_dir().unwrap_or_else(|| PathBuf::from("backups"));
    let path = jobs::backup(&state, &dir).await?;
    {
        let mut db = state.writer().await;
        audit::record(&mut *db, Entry::admin(Some(user.id), "backup_created")).await?;
    }
    Ok(Json(BackupResult {
        path: path.display().to_string(),
    }))
}

/// The SMTP settings the super admin edits, next to what the environment provides.
#[derive(Serialize, ToSchema)]
pub struct EmailSettingsView {
    /// What is set in the database (these win over the environment).
    pub host: Option<String>,
    pub port: Option<i64>,
    pub username: Option<String>,
    pub from_address: Option<String>,
    /// `starttls`, `tls` or `none`.
    pub tls: Option<String>,
    pub has_password: bool,
    /// What the environment provides (`SMTP_*`).
    pub env: EnvEmail,
    /// Whether email can be sent with what is in force now.
    pub configured: bool,
}

#[derive(Serialize, ToSchema)]
pub struct EnvEmail {
    pub host: Option<String>,
    pub port: Option<i64>,
    pub username: Option<String>,
    pub from_address: Option<String>,
    pub tls: String,
    pub has_password: bool,
}

async fn email_view(db: &mut dyn toasty::Executor) -> Result<EmailSettingsView, ApiError> {
    let row = EmailSettings::filter_by_id(1)
        .first()
        .exec(&mut *db)
        .await?;
    let env = crate::mail::env_config();
    let effective = crate::mail::effective_config(db).await;
    Ok(EmailSettingsView {
        host: row.as_ref().and_then(|r| r.host.clone()),
        port: row.as_ref().and_then(|r| r.port),
        username: row.as_ref().and_then(|r| r.username.clone()),
        from_address: row.as_ref().and_then(|r| r.sender.clone()),
        tls: row.as_ref().and_then(|r| r.tls.clone()),
        has_password: row.as_ref().is_some_and(|r| r.password.is_some()),
        env: EnvEmail {
            host: env.host,
            port: env.port.map(i64::from),
            username: env.username,
            from_address: env.from,
            tls: env.tls,
            has_password: env.password.is_some(),
        },
        configured: effective.complete(),
    })
}

/// The email settings.
#[utoipa::path(
    get,
    path = "/admin/email",
    tag = "admin",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "Email settings", body = EmailSettingsView),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_get_email_settings(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<EmailSettingsView>, ApiError> {
    access::require_super_admin(&user)?;
    Ok(Json(email_view(&mut state.db()).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateEmailSettings {
    /// Empty or null removes the override (the environment applies).
    pub host: Option<String>,
    pub port: Option<i64>,
    pub username: Option<String>,
    pub from_address: Option<String>,
    pub tls: Option<String>,
    /// Absent keeps the stored password; an empty string removes it.
    pub password: Option<String>,
}

/// Replace the settings stored in the database. A field left empty falls back to the environment.
#[utoipa::path(
    put,
    path = "/admin/email",
    tag = "admin",
    security(("session_cookie" = [])),
    request_body = UpdateEmailSettings,
    responses(
        (status = 200, description = "Saved settings", body = EmailSettingsView),
        (status = 400, description = "Invalid value"),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_update_email_settings(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<UpdateEmailSettings>,
) -> Result<Json<EmailSettingsView>, ApiError> {
    access::require_super_admin(&user)?;
    let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let (host, username, from, tls) = (
        clean(input.host),
        clean(input.username),
        clean(input.from_address),
        clean(input.tls),
    );
    if tls
        .as_deref()
        .is_some_and(|t| !["starttls", "tls", "none"].contains(&t))
    {
        return Err(ApiError::bad_request("tls must be starttls, tls or none"));
    }
    if input.port.is_some_and(|p| !(1..=65535).contains(&p)) {
        return Err(ApiError::bad_request("the port must be 1 to 65535"));
    }
    let mut db = state.writer().await;
    let existing = EmailSettings::filter_by_id(1)
        .first()
        .exec(&mut *db)
        .await?;
    let password = match input.password {
        None => existing.as_ref().and_then(|r| r.password.clone()),
        Some(p) => clean(Some(p)),
    };
    if existing.is_some() {
        EmailSettings::filter_by_id(1)
            .update()
            .host(host)
            .port(input.port)
            .username(username)
            .password(password)
            .sender(from)
            .tls(tls)
            .exec(&mut *db)
            .await?;
    } else {
        toasty::create!(EmailSettings {
            id: 1,
            host,
            port: input.port,
            username,
            password,
            sender: from,
            tls,
        })
        .exec(&mut *db)
        .await?;
    }
    let mut tx = db.transaction().await?;
    audit::record(
        &mut tx,
        Entry::admin(Some(user.id), "email_settings_changed"),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(email_view(&mut *db).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct TestEmail {
    /// Where to send it; defaults to your own address.
    pub to: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct TestEmailResult {
    pub sent_to: String,
}

/// Send a test email with the settings in force.
#[utoipa::path(
    post,
    path = "/admin/email/test",
    tag = "admin",
    security(("session_cookie" = [])),
    request_body = TestEmail,
    responses(
        (status = 200, description = "Sent", body = TestEmailResult),
        (status = 400, description = "Email is not configured, or the server refused it"),
        (status = 404, description = "Not the super admin")
    )
)]
pub async fn admin_test_email(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<TestEmail>,
) -> Result<Json<TestEmailResult>, ApiError> {
    access::require_super_admin(&user)?;
    let to = input
        .to
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(user.email.clone());
    state
        .mailer
        .send(crate::mail::Mail {
            to: to.clone(),
            subject: "Woodmouse test email".to_string(),
            text: "If you can read this, Woodmouse can send email.".to_string(),
        })
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Json(TestEmailResult { sent_to: to }))
}
