use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct Health {
    pub status: String,
}

/// Liveness probe.
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses((status = 200, description = "Service is healthy", body = Health))
)]
pub async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".to_string(),
    })
}

/// Database round-trip probe: fails (500) if the connection is broken.
#[allow(dead_code)]
#[utoipa::path(
    get,
    path = "/health/db",
    tag = "health",
    responses(
        (status = 200, description = "Database query succeeded"),
        (status = 500, description = "Database unavailable")
    )
)]
pub async fn health_db(
    State(state): State<AppState>,
) -> Result<StatusCode, crate::error::ApiError> {
    let db = &mut *state.db.lock().await;
    let _: Vec<crate::models::Todo> = crate::models::Todo::all().exec(db).await?;
    Ok(StatusCode::OK)
}
