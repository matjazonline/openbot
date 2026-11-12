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
            AppError::WorkflowRole(_) => "workflow_role",
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
            AppError::WorkflowRole(refusal) => (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({"code":refusal.kind.code(),"error":refusal.to_string(),"step":refusal.step,"role":refusal.role})),
            ).into_response(),
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

#[cfg(test)]
mod role_tests {
    use super::*;
    use crate::domain::workflow::StepId;
    use crate::entities::department::{DepartmentRole, RoleAdmissionRefusal, RoleRefusalKind};

    #[tokio::test]
    async fn workflow_role_http_refusal_is_a_bounded_named_bad_request() {
        let refusal = RoleAdmissionRefusal::new(
            StepId::parse("draft").unwrap(),
            DepartmentRole::Lead,
            RoleRefusalKind::NoHolder,
        );
        let response = AppError::WorkflowRole(refusal).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["code"], "workflow.role_no_holder");
        assert_eq!(value["step"], "draft");
        assert_eq!(value["role"], "lead");
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert!(value["error"].as_str().unwrap().contains("draft"));
    }
}
