//! Module of the *main binary only*: deliberately not part of the library,
//! so the `specs` binary (and everything else that links the library) builds
//! without the frontend bundle. Spec generation must never require a
//! frontend build (see the `gen:spec` / `gen:sdk` mise tasks).
//!
//! The built files of `frontend/dist`, served with the cache policy they
//! need, and the SPA fallback to index.html.

use axum::http::{StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// The built frontend, embedded in the binary (and read from disk in
/// debug builds, so a dev server needs no rebuild).
#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct Dist;

/// Everything without its own cache policy answers with no-cache, like
/// the API responses and the shell; the hashed files under /assets are
/// immutable.
pub async fn no_cache(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    if response.headers().get(header::CACHE_CONTROL).is_none() {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    }
    response
}

/// The embedded CSR frontend: static files, falling back to index.html
/// for client routes (TanStack Router). Registered as the axum fallback, so
/// it only sees requests that no API route matched.
pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (file, cache) = if path.is_empty() {
        ("index.html", "no-cache")
    } else {
        match Dist::get(path) {
            Some(_) if path.starts_with("assets/") => (path, "public, max-age=31536000, immutable"),
            Some(_) => (path, "no-cache"),
            None => ("index.html", "no-cache"),
        }
    };
    match Dist::get(file) {
        Some(content) => {
            let mime = mime_guess::from_path(file).first_or_octet_stream();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.as_ref()),
                    (header::CACHE_CONTROL, cache),
                ],
                content.data,
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            "frontend not built; run: mise run build:frontend",
        )
            .into_response(),
    }
}
