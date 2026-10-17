//! woodmouse API: axum + utoipa (OpenAPI) + toasty (ORM) on SQLite.
//!
//! Endpoints (all under `/api`):
//!   GET  /api/health     - liveness probe
//!   GET  /api/auth/*     - bootstrap (first user), login/logout, me
//!   /                    - embedded CSR frontend (SPA fallback)
//!   /swagger-ui         - Swagger UI
//!
//! Commands: `woodmouse` serves; `woodmouse seed` fills an empty database with mock data; `woodmouse check` reports orphaned rows; `woodmouse backup [DIR]` writes a
//! consistent copy of the database (default `BACKUP_DIR`, or `backups`).

// The embedded frontend bundle — a module of this binary, not the library,
// so spec generation (cargo specs) never requires a built frontend.
mod frontend;

use std::future::IntoFuture;

use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use utoipa_swagger_ui::SwaggerUi;

use woodmouse::{routes, state::AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // The URL scheme selects the driver; `sqlite:<path>` opens a file-backed
    // database. Use `sqlite::memory:` for a throwaway in-process database.
    let db_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:woodmouse.db".to_string());

    let mut db = toasty::Db::builder()
        // Discovers every #[derive(Model)] struct in this crate.
        .models(toasty::models!(crate::*))
        .connect(&db_url)
        .await?;

    // Bring the schema up to date. A database made before migrations existed (by `push_schema`) has the
    // tables but no migration record: `mise run clean` starts it fresh.
    let applied = woodmouse::migrations::MIGRATIONS.apply(&db).await?;
    tracing::info!(
        applied = applied.applied(),
        already_applied = applied.skipped(),
        "schema is up to date"
    );
    woodmouse::search::ensure_index(&mut db)
        .await
        .map_err(|_| "could not create the search index")?;

    // The sqlite driver sets no pragmas itself. WAL persists in the database
    // file, so issuing it once at startup covers every pool connection (see
    // backend/tests/spike.rs). In-process writes are additionally serialized
    // by the AppState mutex.
    {
        let mode = toasty::sql::query("PRAGMA journal_mode=WAL")
            .exec(&mut db)
            .await?;
        tracing::info!(url = %db_url, journal_mode = ?mode, "database ready");
    }

    let files = woodmouse::storage::FileStores::from_env()?;
    tracing::info!(
        max_upload_bytes = files.max_upload_bytes,
        "file storage ready"
    );
    let state = AppState::with_files(db, files);

    match std::env::args().nth(1).as_deref() {
        None | Some("serve") => {}
        Some("check") => return run_check(&state).await,
        Some("backup") => return run_backup(&state, std::env::args().nth(2)).await,
        Some("seed") => {
            let router = routes::api(state.clone())
                .with_state(state)
                .split_for_parts()
                .0;
            return woodmouse::seed::run(router).await.map_err(Into::into);
        }
        Some(other) => {
            return Err(format!("unknown command `{other}` (try check or backup)").into());
        }
    }
    woodmouse::jobs::spawn(state.clone());

    let (router, openapi) = routes::api(state.clone())
        .with_state(state.clone())
        .split_for_parts();

    let app = router
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi))
        // Everything the API does not match is served from the embedded
        // frontend bundle, falling back to index.html for SPA routes.
        .fallback(frontend::handler)
        .layer(TraceLayer::new_for_http());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr} (swagger-ui at /swagger-ui)");

    // Connection info lets the login slow-down tell addresses apart (kept in memory only).
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = stopped.await;
    })
    .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result?,
        () = shutdown_signal() => {
            // Keep the last seconds of live typing, then let requests finish. Open WebSockets can keep
            // the server from finishing, so give it a few seconds.
            state.rooms.save_all(&state).await;
            let _ = stop.send(());
            let _ = tokio::time::timeout(std::time::Duration::from_secs(10), &mut server).await;
        }
    }
    // Leave the database file complete, with an empty write-ahead log.
    woodmouse::jobs::checkpoint(&state).await;
    Ok(())
}

async fn run_check(state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let problems = woodmouse::integrity::check(&mut state.db())
        .await
        .map_err(|_| "the check failed")?;
    for p in &problems {
        println!("{} {}: {}", p.table, p.id, p.relation);
    }
    if problems.is_empty() {
        println!("no problems found");
        Ok(())
    } else {
        Err(format!("{} problem(s) found", problems.len()).into())
    }
}

async fn run_backup(
    state: &AppState,
    dir: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = dir
        .map(std::path::PathBuf::from)
        .or_else(woodmouse::config::backup_dir)
        .unwrap_or_else(|| "backups".into());
    let path = woodmouse::jobs::backup(state, &dir)
        .await
        .map_err(|_| "the backup failed")?;
    println!("{}", path.display());
    Ok(())
}

/// Resolves on Ctrl+C or SIGTERM (what a container runtime sends on stop).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install the SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
