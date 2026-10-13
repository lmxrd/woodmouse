//! woodmouse API: axum + utoipa (OpenAPI) + toasty (ORM) on SQLite.
//!
//! Endpoints:
//!   GET  /health      - liveness probe
//!   GET  /todos       - list todos
//!   POST /todos       - create a todo
//!   GET  /todos/{id}  - fetch one todo
//!   /                 - embedded CSR frontend (SPA fallback)
//!   /swagger-ui       - Swagger UI

use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use utoipa_swagger_ui::SwaggerUi;

use woodmouse::{frontend, routes, state::AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // The URL scheme selects the driver; `sqlite:<path>` opens a file-backed
    // database. Use `sqlite::memory:` for a throwaway in-process database.
    let db_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:woodmouse.db".to_string());

    let db = toasty::Db::builder()
        // Discovers every #[derive(Model)] struct in this crate.
        .models(toasty::models!(crate::*))
        .connect(&db_url)
        .await?;

    // Creates tables and indices straight from the models — the quick path.
    // Real apps evolve a persistent schema with migrations instead.
    db.push_schema().await?;
    tracing::info!(url = %db_url, "database ready");

    let state = AppState::new(db);

    let (router, openapi) = routes::api().with_state(state).split_for_parts();

    let app = router
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi))
        // Everything the API does not match is served from the embedded
        // frontend bundle, falling back to index.html for SPA routes.
        .fallback(frontend::handler)
        // Allow any origin for local development (e.g. a vite dev server on
        // :5173). Restrict to known origins before serving production.
        .layer(CorsLayer::very_permissive())
        .layer(TraceLayer::new_for_http());

    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr} (swagger-ui at /swagger-ui)");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    tracing::info!("shutting down");
}
