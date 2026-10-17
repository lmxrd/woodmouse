//! Background jobs, started once by the server: deleting expired sessions every day, and (when
//! `BACKUP_DIR` is set) a daily backup of the database.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::auth;
use crate::config;
use crate::error::ApiError;
use crate::state::AppState;
use crate::time;

/// How many daily backups are kept.
const KEEP_BACKUPS: usize = 14;

/// Write a consistent copy of the database to `dir` (created if needed) with `VACUUM INTO`, and return
/// its path. Safe to run while the server is serving.
pub async fn backup(state: &AppState, dir: &Path) -> Result<PathBuf, ApiError> {
    let fail = |what: &str, e: &dyn std::fmt::Display| {
        tracing::error!(%e, "{what}");
        ApiError::internal("backup failed")
    };
    std::fs::create_dir_all(dir).map_err(|e| fail("creating the backup directory", &e))?;
    let stamp = time::now().strftime("%Y%m%d-%H%M%S").to_string();
    let path = dir.join(format!("woodmouse-{stamp}.db"));
    let mut db = state.db();
    toasty::sql::statement("VACUUM INTO ?1")
        .bind(path.to_string_lossy().to_string())
        .exec(&mut db)
        .await
        .map_err(|e| fail("writing the backup", &e))?;
    Ok(path)
}

/// Move everything in the write-ahead log into the database file and empty the log. Run when the server
/// stops, so the database file alone is complete and the `-wal` file holds nothing.
pub async fn checkpoint(state: &AppState) {
    let mut db = state.writer().await;
    match toasty::sql::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .exec(&mut *db)
        .await
    {
        Ok(_) => tracing::info!("database checkpointed"),
        Err(e) => tracing::warn!(%e, "could not checkpoint the database"),
    }
}

/// Keep only the newest [`KEEP_BACKUPS`] backups in `dir`.
fn prune_backups(dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("woodmouse-") && n.ends_with(".db"))
        })
        .collect();
    names.sort();
    let excess = names.len().saturating_sub(KEEP_BACKUPS);
    for old in names.into_iter().take(excess) {
        let _ = std::fs::remove_file(old);
    }
}

/// Run one round of the daily jobs.
pub async fn run_daily(state: &AppState) {
    {
        let mut db = state.writer().await;
        match auth::delete_expired_sessions(&mut *db).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(deleted = n, "removed expired sessions"),
            Err(_) => tracing::warn!("removing expired sessions failed"),
        }
    }
    if let Some(dir) = config::backup_dir() {
        match backup(state, &dir).await {
            Ok(path) => {
                tracing::info!(path = %path.display(), "backup written");
                prune_backups(&dir);
            }
            Err(_) => tracing::warn!("the daily backup failed"),
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
fn local_now(user: &crate::models::User, now: jiff::civil::DateTime) -> jiff::civil::DateTime {
    let zone = user
        .timezone
        .as_deref()
        .and_then(|name| jiff::tz::TimeZone::get(name).ok())
        .unwrap_or(jiff::tz::TimeZone::UTC);
    time::timestamp(now).to_zoned(zone).datetime()
}

/// Notify people about tasks assigned to them that are due tomorrow or overdue, once per task and due
/// date. "Tomorrow" and "overdue" follow each person's own timezone.
pub async fn deadline_notifications(state: &AppState, now: jiff::civil::DateTime) {
    use crate::models::{Notification, Project, Task, TaskAssignee, User};
    let mut db = state.writer().await;
    let db = &mut *db;
    let result: Result<(), crate::error::ApiError> = async {
        for user in User::all().exec(db).await?.into_iter().filter(|u| u.active) {
            let local_today = local_now(&user, now).date();
            let assigned: Vec<uuid::Uuid> = TaskAssignee::filter_by_user_id(user.id)
                .exec(db)
                .await?
                .into_iter()
                .map(|a| a.task_id)
                .collect();
            for task_id in assigned {
                let Ok(task) = Task::get_by_id(db, &task_id).await else {
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
                let already = Notification::filter_by_user_id(user.id)
                    .exec(db)
                    .await?
                    .iter()
                    .any(|n| {
                        n.kind == kind
                            && n.target_id == Some(task.id)
                            && serde_json::from_str::<serde_json::Value>(&n.data)
                                .is_ok_and(|d| d["due"] == due_text)
                    });
                if already {
                    continue;
                }
                let Ok(project) = Project::get_by_id(db, &task.project_id).await else {
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
pub async fn send_pending_emails(state: &AppState, now: jiff::civil::DateTime) {
    use crate::models::{Notification, NotificationSetting, User};
    if !state.mailer.configured().await {
        return;
    }
    let mut pending: Vec<Notification> = vec![];
    let mut db = state.db();
    let Ok(all) = Notification::all()
        .filter(Notification::fields().emailed_at().is_none())
        .exec(&mut db)
        .await
    else {
        return;
    };
    let cutoff = time::add_seconds(now, -2 * 86_400);
    pending.extend(all.into_iter().filter(|n| n.created_at >= cutoff));
    let base = public_base();
    for n in pending {
        let user = User::get_by_id(&mut db, &n.user_id).await.ok();
        let wants = match &user {
            Some(u) if u.active && !u.password_hash.is_empty() => {
                NotificationSetting::filter_by_user_id(u.id)
                    .first()
                    .exec(&mut db)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|s| s.email_instant)
            }
            _ => false,
        };
        if wants && let Some(user) = &user {
            let actor = match n.actor_id {
                Some(id) => User::get_by_id(&mut db, &id)
                    .await
                    .ok()
                    .map(|u| if u.name.is_empty() { u.email } else { u.name }),
                None => None,
            };
            let data: serde_json::Value = serde_json::from_str(&n.data).unwrap_or_default();
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
        let mut writer = state.writer().await;
        let _ = Notification::filter_by_id(n.id)
            .update()
            .emailed_at(Some(now))
            .exec(&mut *writer)
            .await;
    }
}

/// Send each person who wants one their daily summary when their local hour comes round: unread
/// notifications since the last one, and their tasks due today or overdue.
pub async fn send_digests(state: &AppState, now: jiff::civil::DateTime) {
    use crate::models::{Notification, NotificationSetting, Task, TaskAssignee, User};
    if !state.mailer.configured().await {
        return;
    }
    let mut db = state.db();
    let Ok(settings) = NotificationSetting::all().exec(&mut db).await else {
        return;
    };
    let base = public_base();
    for setting in settings.into_iter().filter(|s| s.digest) {
        let Ok(user) = User::get_by_id(&mut db, &setting.user_id).await else {
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
        let Ok(rows) = Notification::filter_by_user_id(user.id).exec(&mut db).await else {
            continue;
        };
        for n in rows
            .iter()
            .filter(|n| n.read_at.is_none() && n.created_at > since)
        {
            let actor = match n.actor_id {
                Some(id) => User::get_by_id(&mut db, &id)
                    .await
                    .ok()
                    .map(|u| if u.name.is_empty() { u.email } else { u.name }),
                None => None,
            };
            let data: serde_json::Value = serde_json::from_str(&n.data).unwrap_or_default();
            lines.push(format!(
                "- {} ({base}{})",
                crate::notify::describe(&n.kind, actor.as_deref(), &data),
                crate::notify::path(n.workspace_id, n.target_type.as_deref(), n.target_id)
            ));
        }
        let mut due: Vec<String> = vec![];
        if let Ok(assigned) = TaskAssignee::filter_by_user_id(user.id).exec(&mut db).await {
            for a in assigned {
                let Ok(task) = Task::get_by_id(&mut db, &a.task_id).await else {
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
        let mut writer = state.writer().await;
        let _ = NotificationSetting::filter_by_user_id(user.id)
            .update()
            .last_digest_at(Some(now))
            .exec(&mut *writer)
            .await;
    }
}
