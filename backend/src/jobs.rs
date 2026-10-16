//! Background jobs, started once by the server: deleting expired sessions every day, and (when
//! `BACKUP_DIR` is set) a daily backup of the database.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::auth;
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
    if let Ok(dir) = std::env::var("BACKUP_DIR") {
        let dir = PathBuf::from(dir);
        match backup(state, &dir).await {
            Ok(path) => {
                tracing::info!(path = %path.display(), "backup written");
                prune_backups(&dir);
            }
            Err(_) => tracing::warn!("the daily backup failed"),
        }
    }
}

/// Start the jobs: the first round runs a minute after startup, then every 24 hours.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let mut every_day = tokio::time::interval(Duration::from_secs(24 * 3600));
        loop {
            every_day.tick().await;
            run_daily(&state).await;
        }
    });
}
