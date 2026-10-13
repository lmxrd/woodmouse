use utoipa::OpenApi;

/// Base OpenAPI document: title and metadata only.
///
/// Paths and schemas are collected automatically from the handlers'
/// `#[utoipa::path]` annotations by `utoipa_axum::routes!` (see
/// `src/routes/mod.rs`). Only list components here for types not referenced
/// by any handler.
#[derive(OpenApi)]
#[openapi(info(title = "woodmouse", version = "0.1.0", description = "woodmouse API"))]
pub struct ApiDoc;
