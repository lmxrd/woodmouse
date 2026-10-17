//! Small helpers shared by the route handlers: PATCH field semantics, text validation and file downloads.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;

use crate::error::ApiError;

/// serde for a PATCH field that may be absent (leave it), `null` (clear it) or a value (set it).
pub fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
}

/// Trimmed text that must not be empty (`what` names it in the error).
pub fn required(value: &str, what: &str) -> Result<String, ApiError> {
    match value.trim() {
        "" => Err(ApiError::bad_request(format!("{what} is required"))),
        text => Ok(text.to_string()),
    }
}

/// Trimmed text of at most `max` characters; it may be empty.
pub fn limited(value: &str, what: &str, max: usize) -> Result<String, ApiError> {
    let text = value.trim();
    if text.chars().count() > max {
        return Err(ApiError::bad_request(format!(
            "{what} can be at most {max} characters"
        )));
    }
    Ok(text.to_string())
}

/// [`required`] and [`limited`] together.
pub fn required_max(value: &str, what: &str, max: usize) -> Result<String, ApiError> {
    limited(&required(value, what)?, what, max)
}

/// The longest description of a workspace or project.
const MAX_DESCRIPTION: usize = 2000;

/// A workspace's or project's description: trimmed, at most 2000 characters, may be empty.
pub fn description(value: &str) -> Result<String, ApiError> {
    limited(value, "description", MAX_DESCRIPTION)
}

/// Percent-encode text for a URL path segment or an RFC 5987 header value: only unreserved characters stay.
pub fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A response that the browser saves as a file called `name`.
pub fn attachment(name: &str, content_type: &str, body: impl Into<Body>) -> Response {
    // A header-safe fallback name (printable ASCII, no quotes), and the real one for browsers that read it.
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut response = Response::new(body.into());
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    let disposition = format!(
        "attachment; filename=\"{safe}\"; filename*=UTF-8''{}",
        percent_encode(name)
    );
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}
