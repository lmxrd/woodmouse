//! Notifications, watching tasks and pages, and each person's notification settings.

use crate::db::Db;
use crate::db::prelude::*;
use crate::time::DateTime;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use entity::users;
use entity::{notification_settings, notifications, watches};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::routes::auth::PublicUser;
use crate::state::AppState;
use crate::time;
use entity::{NotificationSetting, Role};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_notifications))
        .routes(routes!(unread_notification_count))
        .routes(routes!(mark_notification_read))
        .routes(routes!(mark_all_notifications_read))
        .routes(routes!(
            get_notification_settings,
            update_notification_settings
        ))
        .routes(routes!(get_task_watch, set_task_watch))
        .routes(routes!(get_page_watch, set_page_watch))
}

#[derive(Serialize, ToSchema)]
pub struct NotificationView {
    pub id: uuid::Uuid,
    /// `task_assigned`, `comment_added`, `mentioned`, `task_changed`, `page_changed`, `due_soon`, `overdue`.
    pub kind: String,
    pub workspace_id: uuid::Uuid,
    /// `task` or `page`: what it opens.
    pub target_type: Option<String>,
    pub target_id: Option<uuid::Uuid>,
    pub actor: Option<PublicUser>,
    /// What the UI needs to phrase it (titles at the time, project id, ...).
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime,
    pub read: bool,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct NotificationFilters {
    /// Only unread ones.
    pub unread: Option<bool>,
    /// How many (default 30, at most 100).
    pub limit: Option<usize>,
}

/// Your notifications, newest first.
#[utoipa::path(
    get,
    path = "/notifications",
    tag = "notifications",
    security(("session_cookie" = [])),
    params(NotificationFilters),
    responses((status = 200, description = "Notifications", body = [NotificationView]))
)]
pub async fn list_notifications(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Query(filters): Query<NotificationFilters>,
) -> Result<Json<Vec<NotificationView>>, ApiError> {
    let db = state.db();
    let mut query = notifications::Entity::find().filter(notifications::Column::UserId.eq(user.id));
    if filters.unread == Some(true) {
        query = query.filter(notifications::Column::ReadAt.is_null());
    }
    let rows = query
        .order_by_desc(notifications::Column::CreatedAt)
        .order_by_desc(notifications::Column::Id)
        .limit(filters.limit.unwrap_or(30).clamp(1, 100) as u64)
        .all(db)
        .await?;
    let actors = rows.load_one(users::Entity, db).await?;
    Ok(Json(
        rows.into_iter()
            .zip(actors)
            .map(|(n, actor)| NotificationView {
                actor: actor.as_ref().map(PublicUser::from),
                data: n.data,
                read: n.read_at.is_some(),
                id: n.id,
                kind: n.kind,
                workspace_id: n.workspace_id,
                target_type: n.target_type,
                target_id: n.target_id,
                created_at: n.created_at,
            })
            .collect(),
    ))
}

#[derive(Serialize, ToSchema)]
pub struct UnreadCount {
    pub count: usize,
}

/// How many notifications you have not read.
#[utoipa::path(
    get,
    path = "/notifications/unread-count",
    tag = "notifications",
    security(("session_cookie" = [])),
    responses((status = 200, description = "The unread count", body = UnreadCount))
)]
pub async fn unread_notification_count(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<UnreadCount>, ApiError> {
    let count = notifications::Entity::find()
        .filter(notifications::Column::UserId.eq(user.id))
        .filter(notifications::Column::ReadAt.is_null())
        .count(state.db())
        .await? as usize;
    Ok(Json(UnreadCount { count }))
}

/// Mark one notification read.
#[utoipa::path(
    post,
    path = "/notifications/{id}/read",
    tag = "notifications",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Notification id")),
    responses(
        (status = 204, description = "Marked read"),
        (status = 404, description = "Not yours")
    )
)]
pub async fn mark_notification_read(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let db = state.writer().await?;
    let n = notifications::Entity::find_by_id(id)
        .one_or_not_found(&*db)
        .await?;
    if n.user_id != user.id {
        return Err(ApiError::not_found());
    }
    if n.read_at.is_none() {
        notifications::ActiveModel {
            id: Unchanged(id),
            read_at: Set(Some(time::now())),
            ..Default::default()
        }
        .update(&*db)
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Mark all your notifications read.
#[utoipa::path(
    post,
    path = "/notifications/read-all",
    tag = "notifications",
    security(("session_cookie" = [])),
    responses((status = 204, description = "All marked read"))
)]
pub async fn mark_all_notifications_read(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<StatusCode, ApiError> {
    let db = state.writer().await?;
    let now = time::now();
    for n in notifications::Entity::find()
        .filter(notifications::Column::UserId.eq(user.id))
        .all(&*db)
        .await?
        .into_iter()
        .filter(|n| n.read_at.is_none())
    {
        notifications::ActiveModel {
            id: Unchanged(n.id),
            read_at: Set(Some(now)),
            ..Default::default()
        }
        .update(&*db)
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Your notification preferences.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Settings {
    /// Email each notification as it happens.
    pub email_instant: bool,
    /// Email a daily summary.
    pub digest: bool,
    /// The local hour (0-23, in your timezone) the summary is sent.
    pub digest_hour: i64,
    /// Whether the server can send email at all.
    #[serde(default)]
    pub email_available: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateSettings {
    pub email_instant: Option<bool>,
    pub digest: Option<bool>,
    pub digest_hour: Option<i64>,
}

async fn settings_of(db: &impl Db, user_id: uuid::Uuid) -> Result<NotificationSetting, ApiError> {
    Ok(notification_settings::Entity::find()
        .filter(notification_settings::Column::UserId.eq(user_id))
        .one(db)
        .await?
        .unwrap_or(NotificationSetting {
            user_id,
            email_instant: false,
            digest: false,
            digest_hour: 8,
            last_digest_at: None,
        }))
}

/// Your notification preferences.
#[utoipa::path(
    get,
    path = "/notification-settings",
    tag = "notifications",
    security(("session_cookie" = [])),
    responses((status = 200, description = "Preferences", body = Settings))
)]
pub async fn get_notification_settings(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
) -> Result<Json<Settings>, ApiError> {
    let s = settings_of(state.db(), user.id).await?;
    Ok(Json(Settings {
        email_instant: s.email_instant,
        digest: s.digest,
        digest_hour: s.digest_hour,
        email_available: state.mailer.configured().await,
    }))
}

/// Change your notification preferences.
#[utoipa::path(
    patch,
    path = "/notification-settings",
    tag = "notifications",
    security(("session_cookie" = [])),
    request_body = UpdateSettings,
    responses(
        (status = 200, description = "Updated preferences", body = Settings),
        (status = 400, description = "Invalid hour")
    )
)]
pub async fn update_notification_settings(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Json(input): Json<UpdateSettings>,
) -> Result<Json<Settings>, ApiError> {
    if input.digest_hour.is_some_and(|h| !(0..=23).contains(&h)) {
        return Err(ApiError::bad_request("the hour must be 0 to 23"));
    }
    let db = state.writer().await?;
    let current = settings_of(&*db, user.id).await?;
    let exists = notification_settings::Entity::find()
        .filter(notification_settings::Column::UserId.eq(user.id))
        .one(&*db)
        .await?
        .is_some();
    let (instant, digest, hour) = (
        input.email_instant.unwrap_or(current.email_instant),
        input.digest.unwrap_or(current.digest),
        input.digest_hour.unwrap_or(current.digest_hour),
    );
    if exists {
        notification_settings::ActiveModel {
            user_id: Unchanged(user.id),
            email_instant: Set(instant),
            digest: Set(digest),
            digest_hour: Set(hour),
            ..Default::default()
        }
        .update(&*db)
        .await?;
    } else {
        notification_settings::ActiveModel {
            user_id: Set(user.id),
            email_instant: Set(instant),
            digest: Set(digest),
            digest_hour: Set(hour),
            ..Default::default()
        }
        .insert(&*db)
        .await?;
    }
    Ok(Json(Settings {
        email_instant: instant,
        digest,
        digest_hour: hour,
        email_available: state.mailer.configured().await,
    }))
}

/// Whether you watch something.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Watching {
    pub watching: bool,
}

async fn is_watching(
    db: &impl Db,
    user_id: uuid::Uuid,
    target_type: &str,
    target_id: uuid::Uuid,
) -> Result<bool, ApiError> {
    Ok(watches::Entity::find()
        .filter(watches::Column::UserId.eq(user_id))
        .all(db)
        .await?
        .iter()
        .any(|w| w.target_type == target_type && w.target_id == target_id))
}

async fn set_watch(
    db: &impl Db,
    user_id: uuid::Uuid,
    target_type: &str,
    target_id: uuid::Uuid,
    watching: bool,
) -> Result<(), ApiError> {
    let existing = is_watching(db, user_id, target_type, target_id).await?;
    if watching && !existing {
        watches::ActiveModel {
            user_id: Set(user_id),
            target_type: Set(target_type.to_string()),
            target_id: Set(target_id),
            created_at: Set(time::now()),
        }
        .insert(db)
        .await?;
    } else if !watching && existing {
        for w in watches::Entity::find()
            .filter(watches::Column::UserId.eq(user_id))
            .all(db)
            .await?
            .into_iter()
            .filter(|w| w.target_type == target_type && w.target_id == target_id)
        {
            watches::Entity::delete_many()
                .filter(watches::Column::UserId.eq(w.user_id))
                .filter(watches::Column::TargetType.eq(w.target_type))
                .filter(watches::Column::TargetId.eq(w.target_id))
                .exec(db)
                .await?;
        }
    }
    Ok(())
}

/// Whether you watch a task.
#[utoipa::path(
    get,
    path = "/tasks/{id}/watch",
    tag = "notifications",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    responses((status = 200, description = "Watch state", body = Watching), (status = 404, description = "No such task"))
)]
pub async fn get_task_watch(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Watching>, ApiError> {
    let db = state.db();
    access::require_task_access(db, &user, id, Role::Read).await?;
    Ok(Json(Watching {
        watching: is_watching(db, user.id, crate::notify::TASK, id).await?,
    }))
}

/// Start or stop watching a task.
#[utoipa::path(
    put,
    path = "/tasks/{id}/watch",
    tag = "notifications",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    request_body = Watching,
    responses((status = 200, description = "Watch state", body = Watching), (status = 404, description = "No such task"))
)]
pub async fn set_task_watch(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<Watching>,
) -> Result<Json<Watching>, ApiError> {
    let db = state.writer().await?;
    access::require_task_access(&*db, &user, id, Role::Read).await?;
    set_watch(&*db, user.id, crate::notify::TASK, id, input.watching).await?;
    Ok(Json(input))
}

/// Whether you watch a wiki page.
#[utoipa::path(
    get,
    path = "/pages/{id}/watch",
    tag = "notifications",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    responses((status = 200, description = "Watch state", body = Watching), (status = 404, description = "No such page"))
)]
pub async fn get_page_watch(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Watching>, ApiError> {
    let db = state.db();
    access::require_page_access(db, &user, id, Role::Read).await?;
    Ok(Json(Watching {
        watching: is_watching(db, user.id, crate::notify::PAGE, id).await?,
    }))
}

/// Start or stop watching a wiki page.
#[utoipa::path(
    put,
    path = "/pages/{id}/watch",
    tag = "notifications",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    request_body = Watching,
    responses((status = 200, description = "Watch state", body = Watching), (status = 404, description = "No such page"))
)]
pub async fn set_page_watch(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<Watching>,
) -> Result<Json<Watching>, ApiError> {
    let db = state.writer().await?;
    access::require_page_access(&*db, &user, id, Role::Read).await?;
    set_watch(&*db, user.id, crate::notify::PAGE, id, input.watching).await?;
    Ok(Json(input))
}
