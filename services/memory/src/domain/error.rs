//! The error taxonomy shared by HTTP and MCP (decisions D6). Empty results are not errors.
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    PayloadTooLarge,
    Unauthenticated,
    CredentialExpired,
    CredentialRevoked,
    Forbidden,
    TenantDisabled,
    GenerationDisabled,
    NotFound,
    IdempotencyConflict,
    RevisionConflict,
    NamespaceDeleted,
    Conflict,
    RequestExpired,
    ClassificationRefused,
    CapacityExceeded,
    SpendExhausted,
    /// Declared in the contract; its use case is not implemented in this release.
    NotImplemented,
    Unavailable,
    Timeout,
}

impl ErrorCode {
    pub const fn status(self) -> u16 {
        match self {
            Self::InvalidRequest => 400,
            Self::Unauthenticated | Self::CredentialExpired | Self::CredentialRevoked => 401,
            Self::Forbidden | Self::TenantDisabled | Self::GenerationDisabled => 403,
            Self::NotFound => 404,
            Self::IdempotencyConflict
            | Self::RevisionConflict
            | Self::NamespaceDeleted
            | Self::Conflict => 409,
            Self::RequestExpired => 410,
            Self::PayloadTooLarge => 413,
            Self::ClassificationRefused => 422,
            Self::CapacityExceeded | Self::SpendExhausted => 429,
            Self::NotImplemented => 501,
            Self::Unavailable => 503,
            Self::Timeout => 504,
        }
    }
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::CapacityExceeded | Self::Unavailable | Self::Timeout
        )
    }
}

/// A service error. `message` is fixed text chosen by the service, never stored content.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ServiceError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl ServiceError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }
    pub fn forbidden() -> Self {
        Self::new(
            ErrorCode::Forbidden,
            "The credential does not grant this request",
        )
    }
    pub fn not_found() -> Self {
        Self::new(ErrorCode::NotFound, "Not found")
    }
    pub fn unauthenticated() -> Self {
        Self::new(
            ErrorCode::Unauthenticated,
            "A valid bearer credential is required",
        )
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unavailable, message)
    }
    pub fn not_implemented(operation: &str) -> Self {
        Self::new(
            ErrorCode::NotImplemented,
            "This operation is declared in the contract but not implemented yet",
        )
        .with_details(serde_json::json!({ "operation": operation }))
    }
}

/// Wire shape: `{"error":{"code","message","retryable","details"},"request_id"}`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorBody {
    pub error: ErrorPayload,
    pub request_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorPayload {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ServiceError {
    pub fn body(&self, request_id: String) -> ErrorBody {
        ErrorBody {
            error: ErrorPayload {
                code: self.code,
                message: self.message.clone(),
                retryable: self.code.retryable(),
                details: self.details.clone(),
            },
            request_id,
        }
    }
}

/// Opaque external identifiers chosen by clients and operators: `[A-Za-z0-9._:-]{1,128}`.
pub fn valid_external_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_contract_error_codes_map_to_distinct_statuses() {
        assert_eq!(ErrorCode::InvalidRequest.status(), 400);
        assert_eq!(ErrorCode::Unauthenticated.status(), 401);
        assert_eq!(ErrorCode::Forbidden.status(), 403);
        assert_eq!(ErrorCode::NotFound.status(), 404);
        assert_eq!(ErrorCode::RequestExpired.status(), 410);
        assert_eq!(ErrorCode::NotImplemented.status(), 501);
        assert_eq!(ErrorCode::Unavailable.status(), 503);
        assert!(ErrorCode::Unavailable.retryable());
        assert!(!ErrorCode::NotImplemented.retryable());
        assert!(!ErrorCode::Forbidden.retryable());
    }

    #[test]
    fn memory_contract_external_ids_are_opaque_and_bounded() {
        assert!(valid_external_id("company-1:agent.2_x"));
        assert!(valid_external_id(&"a".repeat(128)));
        assert!(!valid_external_id(&"a".repeat(129)));
        for bad in ["", "a/b", "a b", "é", "a\n"] {
            assert!(!valid_external_id(bad), "{bad:?}");
        }
    }
}
