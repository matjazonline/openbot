use crate::app_error::AppError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Log the error before it gets converted into a status response.
        tracing::error!(error = ?self, "Request failed");

        match self {
            AppError::Database(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
            }
            AppError::InvalidCredentials => {
                (StatusCode::UNAUTHORIZED, "Invalid credentials").into_response()
            }
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, message).into_response(),
            AppError::Conflict(message) => (StatusCode::CONFLICT, message).into_response(),
            AppError::Timeout(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Execution timed out").into_response()
            }
            AppError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
            }
        }
    }
}
