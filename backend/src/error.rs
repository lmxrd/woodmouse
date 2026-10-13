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
}

impl From<toasty::Error> for ApiError {
    fn from(err: toasty::Error) -> Self {
        if err.is_record_not_found() {
            Self::new(StatusCode::NOT_FOUND, "record not found")
        } else {
            tracing::error!(error = %err, "database error");
            Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}
