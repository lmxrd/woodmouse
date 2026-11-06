//! woodmouse API: axum + utoipa (OpenAPI) + sqlx on SQLite.
//!
//! Endpoints (all under `/api`):
//!   GET  /api/health     - liveness probe
//!   GET  /api/auth/*     - bootstrap (first user), login/logout, me
//!   /                    - embedded CSR frontend (SPA fallback)
//!   /swagger-ui         - Swagger UI (only with SWAGGER_UI set)
//!
//! Commands: `woodmouse` serves; `woodmouse healthcheck` asks the running server's health endpoint; `woodmouse check` reports orphaned rows.

// The embedded frontend bundle — a module of this binary, not the library,
// so spec generation (cargo specs) never requires a built frontend.
mod frontend;

use std::future::IntoFuture;

use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use utoipa_swagger_ui::SwaggerUi;

use anyhow::Context;
use api::{routes, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    let state = AppState::open(&db_url).await.context("could not start")?;

    match std::env::args().nth(1).as_deref() {
        None | Some("serve") => {}
        Some("check") => return run_check(&state).await,
        Some(other) => {
            anyhow::bail!("unknown command `{other}` (try check)");
        }
    }
    api::jobs::spawn(state.clone());

    let (router, openapi) = routes::api(state.clone())
        .with_state(state.clone())
        .split_for_parts();

    // The API's description (swagger-ui) is served only when asked for (`SWAGGER_UI`).
    let swagger_ui = api::config::flag("SWAGGER_UI")
        .map_err(anyhow::Error::msg)?
        .unwrap_or(false);
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
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("could not listen on {addr}"))?;
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
        signal = shutdown_signal() => {
            signal?;
            // Keep the last seconds of live typing, then let requests finish. Open WebSockets can keep
            // the server from finishing, so give it a few seconds.
            state.rooms.save_all(&state).await;
            let _ = stop.send(());
            let _ = tokio::time::timeout(std::time::Duration::from_secs(10), &mut server).await;
        }
    }
    // Leave the database file complete, with an empty write-ahead log.
    api::jobs::checkpoint(&state).await;
    Ok(())
}

/// `GET /api/health` on this machine, by hand (the runtime image ships no wget or curl).
/// Succeeds on a 200, fails otherwise.
async fn run_healthcheck() -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8000);
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .context("the server is not answering")?;
    stream
        .write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .await?;
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).await?;
    if &head == b"HTTP/1.1 200" || &head == b"HTTP/1.0 200" {
        Ok(())
    } else {
        anyhow::bail!("the health endpoint did not answer 200")
    }
}

async fn run_check(state: &AppState) -> anyhow::Result<()> {
    let problems = api::integrity::check(state.db())
        .await
        .context("the check failed")?;
    for p in &problems {
        println!("{} {}: {}", p.table, p.id, p.relation);
    }
    if problems.is_empty() {
        println!("no problems found");
        Ok(())
    } else {
        anyhow::bail!("{} problem(s) found", problems.len())
    }
}

/// Resolves on Ctrl+C or SIGTERM (what a container runtime sends on stop); an error if neither can be
/// listened for.
async fn shutdown_signal() -> anyhow::Result<()> {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .context("could not listen for Ctrl+C")
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .context("could not listen for SIGTERM")?
            .recv()
            .await;
        Ok::<(), anyhow::Error>(())
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<anyhow::Result<()>>();
    tokio::select! {
        result = ctrl_c => result?,
        result = terminate => result?,
    }
    tracing::info!("shutting down");
    Ok(())
}
