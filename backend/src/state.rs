use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::storage::FileStores;

/// Who has which page open for editing: page id -> user id -> (last ping, editing without live sync).
pub type PresenceMap = HashMap<uuid::Uuid, HashMap<uuid::Uuid, (Instant, bool)>>;

/// Shared application state. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    /// The connection pool. Each request takes its own handle ([`AppState::db`]), so reads run in parallel
    /// (SQLite in WAL mode).
    db: toasty::Db,
    /// Held by anything that writes, so writes take turns in the app instead of meeting SQLite's
    /// "database is locked". Reads never take it.
    writes: Arc<Mutex<()>>,
    /// Set the `Secure` flag on session cookies (for HTTPS deployments).
    pub secure_cookies: bool,
    /// Where uploaded files live.
    pub files: Arc<FileStores>,
    /// The live editing rooms (see `live`).
    pub rooms: Arc<crate::live::Rooms>,
    /// Who has which page open for editing. Lives in memory only.
    pub presence: Arc<std::sync::Mutex<PresenceMap>>,
    /// Recent failed logins, for slowing down guessing (see `auth::limits`). In memory only.
    pub login_limits: Arc<crate::auth::LoginLimits>,
}

/// A database handle for a request that writes. Dropping it lets the next writer in.
pub struct Writer {
    pub db: toasty::Db,
    _turn: OwnedMutexGuard<()>,
}

impl std::ops::Deref for Writer {
    type Target = toasty::Db;
    fn deref(&self) -> &toasty::Db {
        &self.db
    }
}

impl std::ops::DerefMut for Writer {
    fn deref_mut(&mut self) -> &mut toasty::Db {
        &mut self.db
    }
}

impl AppState {
    /// State with a throwaway local file store (tests, and anything that does not touch files).
    pub fn new(db: toasty::Db) -> Self {
        Self::with_files(db, FileStores::temporary())
    }

    pub fn with_files(db: toasty::Db, files: FileStores) -> Self {
        Self {
            db,
            writes: Arc::default(),
            secure_cookies: std::env::var("WOODMOUSE_SECURE_COOKIES").is_ok_and(|v| v != "0"),
            files: Arc::new(files),
            presence: Arc::default(),
            rooms: Arc::default(),
            login_limits: Arc::default(),
        }
    }

    /// A handle for reading.
    pub fn db(&self) -> toasty::Db {
        self.db.clone()
    }

    /// A handle for writing: waits until no other request is writing.
    pub async fn writer(&self) -> Writer {
        let turn = self.writes.clone().lock_owned().await;
        Writer {
            db: self.db.clone(),
            _turn: turn,
        }
    }
}
