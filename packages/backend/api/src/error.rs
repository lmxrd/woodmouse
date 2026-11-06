//! The API's error type. Every handler returns `Result<_, ApiError>`; the error becomes a status and a
//! `{"error": "..."}` body ([`ErrorBody`]), in sentence case ("Not found"). Client errors carry their own
//! message; every 500 (a database error, [`ApiError::internal`], anything from `anyhow`) is logged with its
//! cause and answered with a bare "Internal server error", so nothing about the server leaks. A row that is
//! not there is a 404.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

/// Why a request failed.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Invalid request payload (validation failures).
    #[error("{0}")]
    BadRequest(String),
    /// Missing or invalid session.
    #[error("Authentication required")]
    Unauthorized,
    /// Signed in, but not allowed to do this.
    #[error("{0}")]
    Forbidden(String),
    /// Does not exist *or* is outside the caller's access: the same status for both, so existence is not
    /// leaked.
    #[error("Not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    /// The request body (an upload) exceeds the configured limit.
    #[error("{0}")]
    PayloadTooLarge(String),
    /// Too many attempts (`crate::limits`): answered with `Retry-After`.
    #[error("Too many attempts, try again later")]
    TooManyRequests { retry_after: u64 },
    /// Something on our side failed: the message is for the log, the client gets "Internal server error".
    #[error("{0}")]
    Internal(String),
    /// Any other status (an extractor's rejection, such as a malformed upload).
    #[error("{message}")]
    Status { status: StatusCode, message: String },
    #[error("database error")]
    Sqlx(#[source] sqlx::Error),
    #[error("database error")]
    Db(#[source] sea_orm::DbErr),
    /// Anything else that went wrong.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// What every 500 says.
const INTERNAL: &str = "Internal server error";

/// The body of every error response.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    /// What went wrong, for people.
    pub error: String,
}

impl ApiError {
    /// An error with any status. A known status becomes its own variant.
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        let message = sentence(message.into());
        match status {
            StatusCode::BAD_REQUEST => Self::BadRequest(message),
            StatusCode::FORBIDDEN => Self::Forbidden(message),
            StatusCode::CONFLICT => Self::Conflict(message),
            StatusCode::PAYLOAD_TOO_LARGE => Self::PayloadTooLarge(message),
            StatusCode::INTERNAL_SERVER_ERROR => Self::Internal(message),
            status => Self::Status { status, message },
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(sentence(message.into()))
    }

    pub fn unauthorized() -> Self {
        Self::Unauthorized
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(sentence(message.into()))
    }

    pub fn not_found() -> Self {
        Self::NotFound
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(sentence(message.into()))
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::PayloadTooLarge(sentence(message.into()))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Status { status, .. } => *status,
            Self::Internal(_) | Self::Sqlx(_) | Self::Db(_) | Self::Other(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

/// The message with its first letter capitalized: every client message is in sentence case.
fn sentence(message: String) -> String {
    let mut chars = message.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => first.to_uppercase().chain(chars).collect(),
        _ => message,
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => Self::NotFound,
            err => Self::Sqlx(err),
        }
    }
}

impl From<sea_orm::DbErr> for ApiError {
    fn from(err: sea_orm::DbErr) -> Self {
        match err {
            sea_orm::DbErr::RecordNotFound(_) => Self::NotFound,
            err => Self::Db(err),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let error = match &self {
            Self::Sqlx(_) | Self::Db(_) | Self::Other(_) => {
                // `{:#}` on anyhow prints the whole chain; the database variants name their source.
                let cause = match &self {
                    Self::Other(e) => format!("{e:#}"),
                    Self::Sqlx(e) => e.to_string(),
                    Self::Db(e) => e.to_string(),
                    _ => unreachable!(),
                };
                tracing::error!(error = %self, cause, "request failed");
                INTERNAL.to_string()
            }
            Self::Internal(message) => {
                tracing::error!(error = %message, "request failed");
                INTERNAL.to_string()
            }
            other => other.to_string(),
        };
        match self {
            Self::TooManyRequests { retry_after } => (
                status,
                [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
                Json(ErrorBody { error }),
            )
                .into_response(),
            _ => (status, Json(ErrorBody { error })).into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_errors_say_nothing_and_missing_rows_are_404() {
        assert_eq!(
            ApiError::from(sea_orm::DbErr::RecordNotFound("x".into())).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            ApiError::from(sqlx::Error::RowNotFound).status(),
            StatusCode::NOT_FOUND
        );
        let err = ApiError::from(sea_orm::DbErr::Custom("table secrets is locked".into()));
        assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let err = ApiError::from(anyhow::anyhow!("disk full").context("writing the export"));
        assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            ApiError::internal("disk full").status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn new_picks_the_variant() {
        assert!(matches!(
            ApiError::new(StatusCode::FORBIDDEN, "No"),
            ApiError::Forbidden(_)
        ));
        assert_eq!(
            ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "X").status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}
