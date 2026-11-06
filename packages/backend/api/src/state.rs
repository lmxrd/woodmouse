use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use sea_orm::{DatabaseConnection, EntityTrait};
use tokio::sync::{Mutex, OwnedMutexGuard};

use anyhow::Context;

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
    /// The site is served over https (`PUBLIC_URL` starts with `https://`): every cookie gets `Secure` and
    /// the `__Host-` prefix. Plain cookies otherwise (local development).
    pub secure_cookies: bool,
    /// `TRUST_PROXY`: a reverse proxy sets `X-Forwarded-For`, so the client's address is read from it (for
    /// the login limits). Off, the connection's address is used and the header is ignored.
    pub trust_proxy: bool,
    /// Where uploaded files live.
    pub files: Arc<FileStores>,
    /// The live editing rooms (see `live`).
    pub rooms: Arc<crate::live::Rooms>,
    /// Who has which page open for editing. Lives in memory only.
    pub presence: Arc<std::sync::Mutex<PresenceMap>>,
    /// Limits on sign-in and code attempts (`crate::limits`). In memory only.
    pub limits: Arc<crate::limits::Limits>,
    /// How email goes out (SMTP; tests record instead).
    pub mailer: Arc<dyn crate::mail::Mailer>,
    /// Single sign-on: the settings in force, the provider's metadata, the login cookie's key.
    pub oidc: Arc<crate::oidc::Oidc>,
    /// Signs the two-factor sign-in challenge (`totp::Challenge`). Made at start: a restart only means
    /// signing in again from the password.
    pub challenge_key: axum_extra::extract::cookie::Key,
    /// `TOTP_REQUIRED`: every sign-in of someone without two-factor sets it up first (`totp::second_step`).
    pub totp_required: bool,
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
            secure_cookies: crate::config::public_url().is_some_and(|u| u.starts_with("https://")),
            trust_proxy: false,
            files: Arc::new(files),
            presence: Arc::default(),
            rooms: Arc::default(),
            limits: Arc::default(),
            oidc: Arc::new(crate::oidc::Oidc::new(Default::default())),
            challenge_key: axum_extra::extract::cookie::Key::generate(),
            totp_required: false,
        }
    }

    /// Open the database file at `path` (created when missing): apply the migrations, restore
    /// the search triggers, and set up the file storage from the environment. What the server and the
    /// development tools (`seed`) start from.
    pub async fn open(path: &str) -> anyhow::Result<Self> {
        // The schema: the migrations in the `migration` crate, each applied once, before the app's
        // connections are opened (see `db::migrated`).
        let db = crate::db::migrated(path)
            .await
            .with_context(|| format!("could not open and migrate the database {path}"))?;
        crate::search::ensure_triggers(&db)
            .await
            .map_err(|_| anyhow::anyhow!("could not create the search triggers"))?;
        tracing::info!(path, "database ready");
        let files = FileStores::from_env().map_err(anyhow::Error::msg)?;
        tracing::info!(
            max_upload_bytes = files.max_upload_bytes,
            "file storage ready"
        );
        let oidc = crate::oidc::Oidc::new(
            crate::oidc::EnvSettings::from_env().map_err(anyhow::Error::msg)?,
        );
        let totp_required = crate::config::flag("TOTP_REQUIRED")
            .map_err(anyhow::Error::msg)?
            .unwrap_or(false);
        crate::auth::warm_up();
        oidc.apply(
            entity::oidc_settings::Entity::find_by_id(1)
                .one(&db)
                .await?
                .as_ref(),
        );
        let mut state = Self::with_files(db, files).with_oidc(oidc);
        state.totp_required = totp_required;
        state.trust_proxy = crate::config::flag("TRUST_PROXY")
            .map_err(anyhow::Error::msg)?
            .unwrap_or(false);
        Ok(state)
    }

    /// The same state with other single sign-on settings (the server's from the environment; tests).
    pub fn with_oidc(mut self, oidc: crate::oidc::Oidc) -> Self {
        self.oidc = Arc::new(oidc);
        self
    }

    /// The same state with other limits (tests).
    pub fn with_limits(mut self, limits: crate::limits::Limits) -> Self {
        self.limits = Arc::new(limits);
        self
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
