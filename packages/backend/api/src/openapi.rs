use utoipa::{Modify, OpenApi};

/// Registers the session-cookie security scheme in the OpenAPI document so
/// consumers (swagger-ui "Authorize") know how to authenticate.
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .get_or_insert_with(utoipa::openapi::Components::new);
        components.add_security_scheme(
            "session_cookie",
            utoipa::openapi::security::SecurityScheme::ApiKey(
                utoipa::openapi::security::ApiKey::Cookie(
                    // `__Host-woodmouse_session` when the site is https (`PUBLIC_URL` starting with `https://`).
                    utoipa::openapi::security::ApiKeyValue::with_description(
                        "woodmouse_session",
                        "The session cookie; named `__Host-woodmouse_session` over https",
                    ),
                ),
            ),
        );
    }
}

/// Base OpenAPI document: title, metadata, and the security scheme.
///
/// Paths and schemas are collected automatically from the handlers'
/// `#[utoipa::path]` annotations by `utoipa_axum::routes!` (see
/// `src/routes/mod.rs`). Only list components here for types not referenced
/// by any handler.
#[derive(OpenApi)]
#[openapi(
    info(title = "woodmouse", version = "0.1.0", description = "woodmouse API"),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;
