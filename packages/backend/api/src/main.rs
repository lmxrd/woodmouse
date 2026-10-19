//! woodmouse API: axum + utoipa (OpenAPI) + sqlx on SQLite.
//!
//! Endpoints (all under `/api`):
//!   GET  /api/health     - liveness probe
//!   GET  /api/auth/*     - bootstrap (first user), login/logout, me
//!   /                    - embedded CSR frontend (SPA fallback)
//!   /swagger-ui         - Swagger UI (only with WOODMOUSE_SWAGGER_UI set)
//!
//! Commands: `woodmouse` serves; `woodmouse healthcheck` asks the running server's health endpoint; `woodmouse check` reports orphaned rows.

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

    // The container healthcheck: no database, no server — only a request to the one already running.
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return run_healthcheck().await;
    }

    // `sqlite:<path>` opens a file-backed database (created when missing).
    let db_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:woodmouse.db".to_string());

    let state = AppState::open(&db_url).await?;

    match std::env::args().nth(1).as_deref() {
        None | Some("serve") => {}
        Some("check") => return run_check(&state).await,
        Some(other) => {
            return Err(format!("unknown command `{other}` (try check)").into());
        }
    }
    woodmouse::jobs::spawn(state.clone());

    let (router, openapi) = routes::api(state.clone())
        .with_state(state.clone())
        .split_for_parts();

    // The API's description (swagger-ui) is served only when asked for:
    // WOODMOUSE_SWAGGER_UI set and not "0", like WOODMOUSE_SECURE_COOKIES.
    let swagger_ui = std::env::var("WOODMOUSE_SWAGGER_UI").is_ok_and(|v| v != "0");
    let app = if swagger_ui {
        router.merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi))
    } else {
        router
    };

    let app = app
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
    if swagger_ui {
        tracing::info!("listening on http://{addr} (swagger-ui at /swagger-ui)");
    } else {
        tracing::info!("listening on http://{addr}");
    }

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

/// `GET /api/health` on this machine, by hand (the runtime image ships no wget or curl).
/// Succeeds on a 200, fails otherwise.
async fn run_healthcheck() -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    stream
        .write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .await?;
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).await?;
    if &head == b"HTTP/1.1 200" || &head == b"HTTP/1.0 200" {
        Ok(())
    } else {
        Err("the health endpoint did not answer 200".into())
    }
}

async fn run_check(state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let problems = woodmouse::integrity::check(state.db())
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
