pub mod activity;
pub mod auth;
pub mod comments;
pub mod export;
pub mod files;
pub mod health;
pub mod history;
pub mod invites;
pub mod labels;
pub mod lists;
pub mod live;
pub mod members;
pub mod pages;
pub mod projects;
pub mod references;
pub mod saved_views;
pub mod search;
pub mod subtasks;
pub mod task_links;
pub mod tasks;
pub mod trash;
pub mod workspaces;

use axum::middleware;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use crate::auth::CurrentUser;
use crate::{openapi::ApiDoc, state::AppState};

/// All API routes, registered together with their OpenAPI operations.
///
/// Each feature module registers its own handlers — their `#[utoipa::path]`
/// annotations (with full paths like `/auth/login`) are the single source of
/// truth for both the axum routes and the OpenAPI document.
///
/// Enforcement is fail-closed: every route that is not in the public set is
/// wrapped by the auth layer (`middleware::from_extractor_with_state`), so a
/// handler cannot accidentally ship without authentication — forgetting to
/// mark a new route public makes it 401, not open. Handlers still declare
/// `CurrentUser` themselves because they need the user value (the session is
/// looked up twice per request; negligible at this scale).
///
/// Everything nests under `/api`, which keeps the API cleanly separable from
/// the SPA fallback and lets the dev frontend proxy a single prefix.
pub fn api(state: AppState) -> OpenApiRouter<AppState> {
    let max_upload_bytes = state.files.max_upload_bytes;
    // No session required.
    let public = health::router()
        .merge(auth::public_router())
        .merge(invites::public_router());

    // Session required — enforced by the layer for every route below.
    let protected = auth::protected_router()
        .merge(workspaces::router())
        .merge(members::router())
        .merge(projects::router())
        .merge(lists::router())
        .merge(tasks::router())
        .merge(subtasks::router())
        .merge(comments::router())
        .merge(activity::router())
        .merge(saved_views::router())
        .merge(pages::router())
        .merge(history::router())
        .merge(live::router())
        .merge(references::router())
        .merge(export::router())
        .merge(search::router())
        .merge(files::router(max_upload_bytes))
        .merge(task_links::router())
        .merge(labels::router())
        .merge(trash::router())
        .route_layer(middleware::from_extractor_with_state::<CurrentUser, _>(
            state,
        ));

    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api", public.merge(protected))
}
