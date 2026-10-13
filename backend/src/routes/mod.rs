pub mod health;
pub mod todos;

use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{openapi::ApiDoc, state::AppState};

/// All API routes, registered together with their OpenAPI operations.
///
/// `utoipa_axum::routes!` derives the axum routes *and* the OpenAPI paths and
/// schemas from each handler's `#[utoipa::path]` annotation — the annotation
/// is the single source of truth. Group handlers that share a path in one
/// `routes!` call.
pub fn api() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health::health))
        .routes(routes!(health::health_db))
        .routes(routes!(todos::list_todos, todos::create_todo))
        .routes(routes!(todos::get_todo))
}
