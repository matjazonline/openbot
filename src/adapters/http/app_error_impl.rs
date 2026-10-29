use crate::app_error::AppError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Error messages can contain provider responses, request URLs, or caller text. The
        // request span and response event supply correlation/status; log only a fixed category.
        let error_kind = match &self {
            AppError::Execution(_) => "execution",
            AppError::Database(_) => "database",
            AppError::DatabaseTimeout(_) => "database_timeout",
            AppError::InvalidCredentials => "invalid_credentials",
            AppError::ServiceUnavailable(_) => "service_unavailable",
            AppError::BadRequest(_) => "bad_request",
            AppError::NotFound(_) => "not_found",
            AppError::Conflict(_) => "conflict",
            AppError::Superseded(_) => "superseded",
            AppError::Timeout(_) => "timeout",
            AppError::Internal(_) => "internal",
        };
        tracing::error!(error_kind, "Request failed");

        match self {
            AppError::Execution(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "Execution stopped").into_response()
            }
            AppError::Database(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
            }
            AppError::DatabaseTimeout(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "The database is busy. Try again shortly.",
            )
                .into_response(),
            AppError::InvalidCredentials => {
                (StatusCode::UNAUTHORIZED, "Invalid credentials").into_response()
            }
            AppError::ServiceUnavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Service is busy. Try again shortly.",
            )
                .into_response(),
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, message).into_response(),
            AppError::Conflict(message) | AppError::Superseded(message) => {
                (StatusCode::CONFLICT, message).into_response()
            }
            AppError::Timeout(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Execution timed out").into_response()
            }
            AppError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
            }
        }
    }
}
