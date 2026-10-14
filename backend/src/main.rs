//! woodmouse API: axum + utoipa (OpenAPI) + toasty (ORM) on SQLite.
//!
//! Endpoints (all under `/api`):
//!   GET  /api/health     - liveness probe
//!   GET  /api/auth/*     - bootstrap (first user), login/logout, me
//!   /                    - embedded CSR frontend (SPA fallback)
//!   /swagger-ui         - Swagger UI

// The embedded frontend bundle — a module of this binary, not the library,
// so spec generation (cargo specs) never requires a built frontend.
mod frontend;

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

    ensure_schema(&mut db).await?;

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

    let state = AppState::new(db);

    let (router, openapi) = routes::api(state.clone())
        .with_state(state)
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

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Creates the tables and indices straight from the models — the quick
/// path. Real apps evolve a persistent schema with migrations instead.
///
/// toasty 0.11's `push_schema` is not idempotent (plain `CREATE TABLE`, no
/// introspection), so a naive push on every start fails with "table already
/// exists" against a database created by a previous run. Guard it: push
/// only when none of the expected tables are present, and refuse on a
/// partial set (schema drift — see AGENTS.md on moving to migrations).
async fn ensure_schema(db: &mut toasty::Db) -> Result<(), Box<dyn std::error::Error>> {
    let expected: Vec<String> = db
        .schema()
        .db
        .tables
        .iter()
        .map(|t| t.name.clone())
        .collect();
    let placeholders = expected
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let mut query = toasty::sql::query(format!(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ({placeholders})"
    ));
    for name in &expected {
        query = query.bind(name.clone());
    }
    let rows = query.exec(db).await?;
    let existing = match &rows[0] {
        toasty::stmt::Value::Record(rec) => match &rec[0] {
            toasty::stmt::Value::I64(n) => *n as usize,
            other => return Err(format!("unexpected sqlite_master result: {other:?}").into()),
        },
        other => return Err(format!("unexpected sqlite_master result: {other:?}").into()),
    };

    match existing {
        0 => {
            db.push_schema().await?;
            tracing::info!(tables = expected.len(), "created database schema");
        }
        n if n == expected.len() => {
            tracing::debug!(tables = n, "database schema already present");
        }
        n => {
            return Err(format!(
                "database schema drift: {n}/{} expected tables exist. \
                 The quick path cannot migrate; move to toasty migrations \
                 or remove the database file to start fresh",
                expected.len()
            )
            .into());
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    tracing::info!("shutting down");
}
