use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Application error type: converts to an HTTP response.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// Invalid request payload (validation failures).
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    /// Missing or invalid session.
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "authentication required")
    }

    /// Resource does not exist *or* is outside the caller's access — the
    /// same status for both, so existence is not leaked.
    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not found")
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl From<toasty::Error> for ApiError {
    fn from(err: toasty::Error) -> Self {
        if err.is_record_not_found() {
            Self::not_found()
        } else {
            tracing::error!(error = %err, "database error");
            Self::internal("internal error")
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}
