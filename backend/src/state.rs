use std::sync::Arc;

use tokio::sync::Mutex;

use crate::storage::FileStores;

/// Shared application state.
///
/// Toasty's query methods take `&mut Db`, so the handle sits behind a tokio
/// mutex. `Db` is `Clone` (cheap, shares the connection pool), so the state
/// can be cloned freely.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<toasty::Db>>,
    /// Set the `Secure` flag on session cookies (for HTTPS deployments).
    pub secure_cookies: bool,
    /// Where uploaded files live.
    pub files: Arc<FileStores>,
}

impl AppState {
    /// State with a throwaway local file store (tests, and anything that does not touch files).
    pub fn new(db: toasty::Db) -> Self {
        Self::with_files(db, FileStores::temporary())
    }

    pub fn with_files(db: toasty::Db, files: FileStores) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            secure_cookies: std::env::var("WOODMOUSE_SECURE_COOKIES").is_ok_and(|v| v != "0"),
            files: Arc::new(files),
        }
    }
}
