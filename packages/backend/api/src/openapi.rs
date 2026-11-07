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
    tags(
        (name = "auth", description = "Signing in and out, the first-run setup, your own account, two-factor sign-in and API keys"),
        (name = "workspaces", description = "Workspaces and their members"),
        (name = "projects", description = "The projects of a workspace"),
        (name = "lists", description = "The lists (columns) of a project"),
        (name = "tasks", description = "Tasks, their subtasks, links and assignees"),
        (name = "sprints", description = "The sprints of a project"),
        (name = "views", description = "Saved views of a project"),
        (name = "labels", description = "The labels of a workspace"),
        (name = "comments", description = "Comments on tasks"),
        (name = "wiki", description = "The wiki's pages"),
        (name = "history", description = "Earlier versions of pages and comments, and going back to one"),
        (name = "files", description = "Uploaded files and their versions"),
        (name = "activity", description = "What happened in a workspace"),
        (name = "search", description = "Searching a workspace"),
        (name = "trash", description = "Deleted things, and restoring them"),
        (name = "notifications", description = "Your notifications and what you are notified of"),
        (name = "calendar", description = "Calendar feeds of the tasks with dates"),
        (name = "transfer", description = "Importing into and exporting from a workspace"),
        (name = "admin", description = "Settings of the whole instance, its users and workspaces (super admin only)"),
        (name = "system", description = "Operations"),
    ),
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
    fn every_tag_is_declared_and_used() {
        let spec: serde_json::Value =
            serde_json::from_str(&crate::routes::api_doc().to_pretty_json().unwrap()).unwrap();
        let declared: BTreeSet<_> = spec["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tag| tag["name"].as_str().unwrap().to_string())
            .collect();
        let used: BTreeSet<_> = operations()
            .iter()
            .map(|(_, _, op)| op["tags"][0].as_str().unwrap().to_string())
            .collect();
        assert_eq!(declared, used);
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
