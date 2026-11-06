//! Request extractors and a layer that answer in the API's own error shape (`{"error": "..."}`).

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Json, Request};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;

use crate::error::{ApiError, ErrorBody};

/// A JSON request body: `Content-Type: application/json` and a body that fits `T`. Every endpoint that
/// takes a JSON body uses this instead of axum's `Json` (which still reads it, so its limits apply): a body
/// of another type is a 415, one that is not valid JSON for `T` a 400, one that cannot be read at all
/// keeps the status axum gives it.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(body)| ApiJson(body))
            .map_err(rejection)
    }
}

fn rejection(rejection: JsonRejection) -> ApiError {
    match rejection {
        JsonRejection::MissingJsonContentType(_) => ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Expected a JSON body (Content-Type: application/json)",
        ),
        JsonRejection::JsonSyntaxError(_) | JsonRejection::JsonDataError(_) => {
            ApiError::bad_request("Invalid request body")
        }
        other => ApiError::new(other.status(), "The request body could not be read"),
    }
}

/// The longest plain-text error body turned into an error body (an extractor's message is short).
const MAX_TEXT: usize = 4096;

/// Other extractors (a path or query that does not parse, a malformed upload) answer in plain text; this
/// layer turns a plain-text error answer into the error body, so every error the API gives has one shape.
pub async fn error_bodies(response: Response) -> Response {
    let status = response.status();
    let plain = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/plain"));
    if !(status.is_client_error() || status.is_server_error()) || !plain {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let text = axum::body::to_bytes(body, MAX_TEXT)
        .await
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .unwrap_or_default();
    let error = if status.is_server_error() || text.is_empty() {
        status.canonical_reason().unwrap_or("Error").to_string()
    } else {
        ApiError::new(status, text).to_string()
    };
    parts.headers.remove(header::CONTENT_TYPE);
    parts.headers.remove(header::CONTENT_LENGTH);
    let body = axum::Json(ErrorBody { error }).into_response();
    let (json_parts, json_body) = body.into_parts();
    parts.headers.extend(json_parts.headers);
    Response::from_parts(parts, Body::new(json_body))
}
