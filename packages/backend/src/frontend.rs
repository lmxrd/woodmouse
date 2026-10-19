//! Module of the *main binary only* — deliberately not part of the library,
//! so the `specs` binary (and everything else that links the lib) builds
//! without the frontend bundle. Spec generation must never require a
//! frontend build (see the `gen:spec` / `gen:sdk` mise tasks).

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use mime_guess::from_path;
use rust_embed::RustEmbed;

/// The vite build output (`pnpm build` in `frontend/`), embedded into the
/// binary at compile time. In debug builds rust-embed reads the files from
/// disk instead, so `mise run build:frontend` is enough for `cargo run`.
#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct FrontendAssets;

/// Serve an embedded file; unknown paths return the app shell so client-side
/// routing (TanStack Router) works on deep links.
///
/// Registered as the axum fallback, so it only sees requests that no API
/// route matched.
pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match FrontendAssets::get(path) {
        Some(file) => {
            let mime = from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref())],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => match FrontendAssets::get("index.html") {
            Some(index) => (
                [(header::CONTENT_TYPE, "text/html")],
                index.data.into_owned(),
            )
                .into_response(),
            None => (
                StatusCode::NOT_FOUND,
                "frontend bundle missing - run `mise run build:frontend`",
            )
                .into_response(),
        },
    }
}
