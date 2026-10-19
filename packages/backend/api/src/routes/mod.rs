pub mod activity;
pub mod admin;
pub mod auth;
pub mod calendar;
pub mod comments;
pub mod export;
pub mod file_versions;
pub mod files;
pub mod health;
pub mod history;
pub mod invites;
pub mod labels;
pub mod lists;
pub mod live;
pub mod members;
pub mod notifications;
pub mod pages;
pub mod projects;
pub mod references;
pub mod saved_views;
pub mod search;
pub mod sprints;
pub mod subtasks;
pub mod task_links;
pub mod tasks;
pub mod themes;
pub mod transfer;
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
///
/// Building the routes needs no state ([`api_doc`] writes the OpenAPI
/// document of the same routes without one); the auth layer and the upload
/// limit that [`api`] adds on top change no operation.
pub fn api(state: AppState) -> OpenApiRouter<AppState> {
    // Session required — enforced by the layer for every route below.
    let protected = protected(Some(state.files.max_upload_bytes)).route_layer(
        middleware::from_extractor_with_state::<CurrentUser, _>(state),
    );
    nest(public().merge(protected))
}

/// The OpenAPI document of [`api`], for the `specs` binary (`cargo specs`).
/// The same routes without the layers that need state: no database is
/// opened (and no query could run anyway).
pub fn api_doc() -> utoipa::openapi::OpenApi {
    nest(public().merge(protected(None))).split_for_parts().1
}

/// Everything under `/api`.
fn nest(routes: OpenApiRouter<AppState>) -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api", routes)
}

/// No session required.
fn public() -> OpenApiRouter<AppState> {
    health::router()
        .merge(auth::public_router())
        .merge(invites::public_router())
        .merge(calendar::public_router())
}

/// Session required — enforced by the auth layer that [`api`] adds for
/// every route here. `max_upload_bytes` is the file routes' upload limit;
/// `None` (building them for [`api_doc`]) applies none.
fn protected(max_upload_bytes: Option<u64>) -> OpenApiRouter<AppState> {
    auth::protected_router()
        .merge(admin::router())
        .merge(workspaces::router())
        .merge(members::router())
        .merge(projects::router())
        .merge(lists::router())
        .merge(tasks::router())
        .merge(subtasks::router())
        .merge(sprints::router())
        .merge(notifications::router())
        .merge(transfer::router())
        .merge(comments::router())
        .merge(activity::router())
        .merge(saved_views::router())
        .merge(pages::router())
        .merge(history::router())
        .merge(live::router())
        .merge(references::router())
        .merge(export::router())
        .merge(calendar::router())
        .merge(themes::router())
        .merge(search::router())
        .merge(files::router(max_upload_bytes))
        .merge(file_versions::router(max_upload_bytes))
        .merge(task_links::router())
        .merge(labels::router())
        .merge(trash::router())
}
