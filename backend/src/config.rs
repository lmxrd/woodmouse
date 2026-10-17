//! Settings read from the environment.

use std::path::PathBuf;

/// The value with surrounding whitespace removed; `None` while it is empty.
pub fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// An environment variable; `None` while it is unset or blank.
pub fn var(name: &str) -> Option<String> {
    non_empty(std::env::var(name).ok())
}

/// Where the app is reached from (`PUBLIC_URL`), without a trailing slash.
pub fn public_url() -> Option<String> {
    var("PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string())
}

/// Where backups go (`BACKUP_DIR`), when configured.
pub fn backup_dir() -> Option<PathBuf> {
    var("BACKUP_DIR").map(PathBuf::from)
}
