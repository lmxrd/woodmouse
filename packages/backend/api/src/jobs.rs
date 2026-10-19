//! Background jobs, started once by the server: deleting expired sessions every day, sending
//! notification email, and the deadline and digest checks.

use crate::db::prelude::*;
use entity::{notification_settings, notifications, projects, task_assignees, tasks, users};
use std::time::Duration;

use crate::auth;
use crate::config;
use crate::state::AppState;
use crate::time;

/// Move everything in the write-ahead log into the database file and empty the log. Run when the server
/// stops, so the database file alone is complete and the `-wal` file holds nothing.
pub async fn checkpoint(state: &AppState) {
    let Ok(db) = state.writer().await else {
        return;
    };
    match db
        .execute_unprepared("PRAGMA wal_checkpoint(TRUNCATE)")
        .await
    {
        Ok(_) => tracing::info!("database checkpointed"),
        Err(e) => tracing::warn!(%e, "could not checkpoint the database"),
    }
}

/// Run one round of the daily jobs.
pub async fn run_daily(state: &AppState) {
    let Ok(db) = state.writer().await else {
        return;
    };
    {
        match auth::delete_expired_sessions(&*db).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(deleted = n, "removed expired sessions"),
            Err(_) => tracing::warn!("removing expired sessions failed"),
        }
    }
}

/// Start the jobs: the first round runs a minute after startup, then every 24 hours; email goes out every
/// minute and deadlines and digests are checked every ten.
pub fn spawn(state: AppState) {
    let daily = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let mut every_day = tokio::time::interval(Duration::from_secs(24 * 3600));
        loop {
            every_day.tick().await;
            run_daily(&daily).await;
        }
    });
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        let mut count: u64 = 0;
        loop {
            tick.tick().await;
            let now = time::now();
            if count.is_multiple_of(10) {
                deadline_notifications(&state, now).await;
                send_digests(&state, now).await;
            }
            send_pending_emails(&state, now).await;
            count += 1;
        }
    });
}

/// The address the app is reached at, for links in email (`PUBLIC_URL`), without a trailing slash.
fn public_base() -> String {
    config::public_url().unwrap_or_default()
}

/// The person's local date at `now` (their timezone, or UTC while they have none).
fn local_now(user: &entity::User, now: crate::time::DateTime) -> crate::time::DateTime {
    let zone = user
        .timezone
        .as_deref()
        .and_then(|name| jiff::tz::TimeZone::get(name).ok())
        .unwrap_or(jiff::tz::TimeZone::UTC);
    time::timestamp(now).to_zoned(zone).datetime().into()
}

/// Notify people about tasks assigned to them that are due tomorrow or overdue, once per task and due
/// date. "Tomorrow" and "overdue" follow each person's own timezone.
pub async fn deadline_notifications(state: &AppState, now: crate::time::DateTime) {
    let Ok(db) = state.writer().await else {
        return;
    };
    let db = &*db;
    let result: Result<(), crate::error::ApiError> = async {
        for user in users::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .filter(|u| u.active)
        {
            let local_today = local_now(&user, now).date();
            let assigned: Vec<uuid::Uuid> = task_assignees::Entity::find()
                .filter(task_assignees::Column::UserId.eq(user.id))
                .all(db)
                .await?
                .into_iter()
                .map(|a| a.task_id)
                .collect();
            for task_id in assigned {
                let Ok(task) = tasks::Entity::find_by_id(task_id)
                    .one_or_not_found(db)
                    .await
                else {
                    continue;
                };
                let Some(due) = task.due_date else { continue };
                if task.deleted_at.is_some() || task.completed_at.is_some() {
                    continue;
                }
                let due_day = due.date();
                let kind = if due_day < local_today {
                    "overdue"
                } else if due_day == local_today.tomorrow().unwrap_or(local_today) {
                    "due_soon"
                } else {
                    continue;
                };
                let due_text = due_day.to_string();
                let already = notifications::Entity::find()
                    .filter(notifications::Column::UserId.eq(user.id))
                    .all(db)
                    .await?
                    .iter()
                    .any(|n| {
                        n.kind == kind && n.target_id == Some(task.id) && n.data["due"] == due_text
                    });
                if already {
                    continue;
                }
                let Ok(project) = projects::Entity::find_by_id(task.project_id)
                    .one_or_not_found(db)
                    .await
                else {
                    continue;
                };
                if project.deleted_at.is_some() {
                    continue;
                }
                crate::notify::notify(
                    db,
                    [user.id],
                    crate::notify::Notice {
                        workspace_id: project.workspace_id,
                        kind,
                        target_type: crate::notify::TASK,
                        target_id: task.id,
                        actor: None,
                        data: serde_json::json!({
                            "title": task.title,
                            "project_id": task.project_id,
                            "due": due_text,
                        }),
                    },
                )
                .await?;
            }
        }
        Ok(())
    }
    .await;
    if result.is_err() {
        tracing::warn!("creating deadline notifications failed");
    }
}

/// Email notifications to the people who want each one as it happens, and mark every notification dealt
/// with (including those nobody wants emailed, so they are not looked at again). Needs email configured.
pub async fn send_pending_emails(state: &AppState, now: crate::time::DateTime) {
    use entity::Notification;
    if !state.mailer.configured().await {
        return;
    }
    let mut pending: Vec<Notification> = vec![];
    let db = state.db();
    let Ok(all) = notifications::Entity::find()
        .filter(notifications::Column::EmailedAt.is_null())
        .all(db)
        .await
    else {
        return;
    };
    let cutoff = time::add_seconds(now, -2 * 86_400);
    pending.extend(all.into_iter().filter(|n| n.created_at >= cutoff));
    let base = public_base();
    for n in pending {
        let user = users::Entity::find_by_id(n.user_id)
            .one_or_not_found(db)
            .await
            .ok();
        let wants = match &user {
            Some(u) if u.active && !u.password_hash.is_empty() => {
                notification_settings::Entity::find()
                    .filter(notification_settings::Column::UserId.eq(u.id))
                    .one(db)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|s| s.email_instant)
            }
            _ => false,
        };
        if wants && let Some(user) = &user {
            let actor = match n.actor_id {
                Some(id) => users::Entity::find_by_id(id)
                    .one_or_not_found(db)
                    .await
                    .ok()
                    .map(|u| if u.name.is_empty() { u.email } else { u.name }),
                None => None,
            };
            let data = n.data.clone();
            let line = crate::notify::describe(&n.kind, actor.as_deref(), &data);
            let link = format!(
                "{base}{}",
                crate::notify::path(n.workspace_id, n.target_type.as_deref(), n.target_id)
            );
            let sent = state
                .mailer
                .send(crate::mail::Mail {
                    to: user.email.clone(),
                    subject: line.clone(),
                    text: format!("{line}\n\n{link}\n"),
                })
                .await;
            if let Err(e) = sent {
                tracing::warn!(error = %e, "sending a notification email failed");
                continue; // try again next sweep
            }
        }
        let Ok(writer) = state.writer().await else {
            continue;
        };
        let _ = notifications::ActiveModel {
            id: Unchanged(n.id),
            emailed_at: Set(Some(now)),
            ..Default::default()
        }
        .update(&*writer)
        .await;
    }
}

/// Send each person who wants one their daily summary when their local hour comes round: unread
/// notifications since the last one, and their tasks due today or overdue.
pub async fn send_digests(state: &AppState, now: crate::time::DateTime) {
    if !state.mailer.configured().await {
        return;
    }
    let db = state.db();
    if false {
        return;
    };
    let Ok(settings) = notification_settings::Entity::find().all(db).await else {
        return;
    };
    let base = public_base();
    for setting in settings.into_iter().filter(|s| s.digest) {
        let Ok(user) = users::Entity::find_by_id(setting.user_id)
            .one_or_not_found(db)
            .await
        else {
            continue;
        };
        if !user.active || user.password_hash.is_empty() {
            continue;
        }
        let local = local_now(&user, now);
        let due_hour = i64::from(local.hour()) == setting.digest_hour;
        let recently = setting
            .last_digest_at
            .is_some_and(|at| time::seconds_between(at, now) < 20 * 3600);
        if !due_hour || recently {
            continue;
        }
        let since = setting
            .last_digest_at
            .unwrap_or_else(|| time::add_seconds(now, -86_400));
        let mut lines: Vec<String> = vec![];
        let Ok(rows) = notifications::Entity::find()
            .filter(notifications::Column::UserId.eq(user.id))
            .all(db)
            .await
        else {
            continue;
        };
        for n in rows
            .iter()
            .filter(|n| n.read_at.is_none() && n.created_at > since)
        {
            let actor = match n.actor_id {
                Some(id) => users::Entity::find_by_id(id)
                    .one_or_not_found(db)
                    .await
                    .ok()
                    .map(|u| if u.name.is_empty() { u.email } else { u.name }),
                None => None,
            };
            let data = n.data.clone();
            lines.push(format!(
                "- {} ({base}{})",
                crate::notify::describe(&n.kind, actor.as_deref(), &data),
                crate::notify::path(n.workspace_id, n.target_type.as_deref(), n.target_id)
            ));
        }
        let mut due: Vec<String> = vec![];
        if let Ok(assigned) = task_assignees::Entity::find()
            .filter(task_assignees::Column::UserId.eq(user.id))
            .all(db)
            .await
        {
            for a in assigned {
                let Ok(task) = tasks::Entity::find_by_id(a.task_id)
                    .one_or_not_found(db)
                    .await
                else {
                    continue;
                };
                if task.deleted_at.is_some() || task.completed_at.is_some() {
                    continue;
                }
                if task.due_date.is_some_and(|d| d.date() <= local.date()) {
                    due.push(format!("- {}", task.title));
                }
            }
        }
        if lines.is_empty() && due.is_empty() {
            continue;
        }
        let mut text = String::from("Your Woodmouse summary\n");
        if !lines.is_empty() {
            text.push_str("\nWhat happened:\n");
            text.push_str(&lines.join("\n"));
            text.push('\n');
        }
        if !due.is_empty() {
            text.push_str("\nDue today or overdue:\n");
            text.push_str(&due.join("\n"));
            text.push('\n');
        }
        let sent = state
            .mailer
            .send(crate::mail::Mail {
                to: user.email.clone(),
                subject: "Your Woodmouse summary".to_string(),
                text,
            })
            .await;
        if let Err(e) = sent {
            tracing::warn!(error = %e, "sending a digest failed");
            continue;
        }
        let Ok(writer) = state.writer().await else {
            continue;
        };
        let _ = notification_settings::ActiveModel {
            user_id: Unchanged(user.id),
            last_digest_at: Set(Some(now)),
            ..Default::default()
        }
        .update(&*writer)
        .await;
    }
}
