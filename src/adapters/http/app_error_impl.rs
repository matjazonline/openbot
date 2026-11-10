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
            AppError::KnowledgeTrigger { .. } => "knowledge_trigger",
            AppError::KnowledgeRelease { .. } => "knowledge_release",
            AppError::Execution(_) => "execution",
            AppError::Database(_) => "database",
            AppError::DatabaseTimeout(_) => "database_timeout",
            AppError::WorkflowGuard(_) => "workflow_guard",
            AppError::InvalidCredentials => "invalid_credentials",
            AppError::McpAuthentication => "mcp_authentication",
            AppError::ServiceUnavailable(_) => "service_unavailable",
            AppError::BadRequest(_) => "bad_request",
            AppError::NotFound(_) => "not_found",
            AppError::Forbidden(_) => "forbidden",
            AppError::Conflict(_) => "conflict",
            AppError::Superseded(_) => "superseded",
            AppError::Timeout(_) => "timeout",
            AppError::Internal(_) => "internal",
        };
        tracing::error!(error_kind, "Request failed");

        let knowledge_code = self.knowledge_code();
        match self {
            AppError::KnowledgeTrigger { .. } => (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({"code":knowledge_code,"error":"The triggering message exceeds this audience's access level"})),
            ).into_response(),
            AppError::KnowledgeRelease { .. } => (
                StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({"code":knowledge_code,"error":"The destination cannot receive this run's content"})),
            ).into_response(),
            AppError::Execution(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "Execution stopped").into_response()
            }
            AppError::Database(_) | AppError::WorkflowGuard(_) => {
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
            AppError::McpAuthentication => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "MCP service rejected the configured credentials",
            )
                .into_response(),
            AppError::ServiceUnavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Service is busy. Try again shortly.",
            )
                .into_response(),
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, message).into_response(),
            AppError::Forbidden(message) => (StatusCode::FORBIDDEN, message).into_response(),
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
