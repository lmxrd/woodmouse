use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use sea_orm::DatabaseConnection;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::error::ApiError;
use crate::storage::FileStores;

/// Who has which page open for editing: page id -> user id -> (last ping, editing without live sync).
pub type PresenceMap = HashMap<uuid::Uuid, HashMap<uuid::Uuid, (Instant, bool)>>;

/// Shared application state. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    /// The connection pool, with SeaORM on top. Reads use it directly ([`AppState::db`]) and run in
    /// parallel (SQLite in WAL mode).
    db: DatabaseConnection,
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
    /// How email goes out (SMTP; tests record instead).
    pub mailer: Arc<dyn crate::mail::Mailer>,
}

/// The connection for a request that writes. Dropping it lets the next writer in.
pub struct Writer {
    db: DatabaseConnection,
    _turn: OwnedMutexGuard<()>,
}

impl Writer {
    /// Start a transaction; commit it, or it rolls back when dropped.
    pub async fn begin(&self) -> Result<sea_orm::DatabaseTransaction, ApiError> {
        Ok(sea_orm::TransactionTrait::begin(&self.db).await?)
    }
}

impl std::ops::Deref for Writer {
    type Target = DatabaseConnection;
    fn deref(&self) -> &DatabaseConnection {
        &self.db
    }
}

impl AppState {
    /// State with a throwaway local file store (tests, and anything that does not touch files).
    pub fn new(db: DatabaseConnection) -> Self {
        Self::with_files(db, FileStores::temporary())
    }

    pub fn with_files(db: DatabaseConnection, files: FileStores) -> Self {
        Self {
            mailer: crate::mail::SmtpMailer::shared(db.clone()),
            db,
            writes: Arc::default(),
            secure_cookies: std::env::var("WOODMOUSE_SECURE_COOKIES").is_ok_and(|v| v != "0"),
            files: Arc::new(files),
            presence: Arc::default(),
            rooms: Arc::default(),
            login_limits: Arc::default(),
        }
    }

    /// Open the database at `url` (`sqlite:<path>`, created when missing): apply the migrations, restore
    /// the search triggers, and set up the file storage from the environment. What the server and the
    /// development tools (`seed`) start from.
    pub async fn open(url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let db = crate::db::orm(crate::db::connect(url).await?);
        // The schema: the migrations in `src/migrations/`, each applied once.
        <migration::Migrator as sea_orm_migration::MigratorTrait>::up(&db, None).await?;
        crate::search::ensure_triggers(&db)
            .await
            .map_err(|_| "could not create the search triggers")?;
        tracing::info!(url, "database ready");
        let files = FileStores::from_env()?;
        tracing::info!(
            max_upload_bytes = files.max_upload_bytes,
            "file storage ready"
        );
        Ok(Self::with_files(db, files))
    }

    /// The same state with another mailer (tests).
    pub fn with_mailer(mut self, mailer: Arc<dyn crate::mail::Mailer>) -> Self {
        self.mailer = mailer;
        self
    }

    /// The database, for reading.
    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    /// The connection for writing: waits until no other request is writing.
    pub async fn writer(&self) -> Result<Writer, ApiError> {
        Ok(Writer {
            _turn: self.writes.clone().lock_owned().await,
            db: self.db.clone(),
        })
    }
}
