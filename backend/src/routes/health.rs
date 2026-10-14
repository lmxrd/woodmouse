use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

/// All health routes, registered next to their handlers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(health))
}

#[derive(Serialize, ToSchema)]
pub struct Health {
    /// The service itself: this endpoint answering proves the process is
    /// alive.
    pub status: String,
    /// Database round-trip: `ok` or `unreachable`.
    pub db: String,
}

/// Liveness probe with a database round-trip. The body reports the two
/// concerns separately; the HTTP status is 200 only when both are healthy
/// (503 when the database is unreachable).
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Service and database are healthy", body = Health),
        (status = 503, description = "Database unreachable", body = Health)
    )
)]
pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let db = &mut *state.db.lock().await;
    let db_status = match toasty::sql::query("SELECT 1").exec(db).await {
        Ok(_) => "ok",
        Err(err) => {
            tracing::error!(error = %err, "health check: database unreachable");
            "unreachable"
        }
    };

    let code = if db_status == "ok" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        code,
        Json(Health {
            status: "ok".into(),
            db: db_status.into(),
        }),
    )
}
