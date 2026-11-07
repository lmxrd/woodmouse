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
    info(title = "woodmouse", description = "woodmouse API"),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    fn operations() -> Vec<(String, String, serde_json::Value)> {
        let spec: serde_json::Value =
            serde_json::from_str(&crate::routes::api_doc().to_pretty_json().unwrap()).unwrap();
        let mut found = Vec::new();
        for (path, item) in spec["paths"].as_object().unwrap() {
            for (method, operation) in item.as_object().unwrap() {
                found.push((method.to_uppercase(), path.clone(), operation.clone()));
            }
        }
        found
    }

    #[test]
    fn operations_have_unique_ids_tags_and_summaries() {
        let operations = operations();
        let ids: BTreeSet<_> = operations
            .iter()
            .map(|(_, _, op)| op["operationId"].as_str().expect("an operationId"))
            .collect();
        assert_eq!(ids.len(), operations.len(), "operation ids must be unique");
        for (method, path, op) in &operations {
            assert!(op["tags"][0].is_string(), "{method} {path} has no tag");
            assert!(op["summary"].is_string(), "{method} {path} has no summary");
        }
    }
}
