//! Inspect only typed transport failures. Raw remote protocol errors never become diagnostics.
use super::{failed, http};
use crate::app_error::AppError;
use rmcp::{service::ServiceError, transport::DynamicTransportError};

fn authentication(error: &DynamicTransportError) -> bool {
    matches!(
        error.error.downcast_ref::<http::Error>(),
        Some(http::Error::Client(http::HttpFailure::Authentication))
    )
}

pub(super) fn initialization(error: rmcp::service::ClientInitializeError) -> AppError {
    if let rmcp::service::ClientInitializeError::TransportError { error, .. } = &error
        && authentication(error)
    {
        return AppError::McpAuthentication;
    }
    failed("MCP initialization failed")
}

pub(super) fn service(error: ServiceError, message: &'static str) -> AppError {
    if let ServiceError::TransportSend(error) = &error
        && authentication(error)
    {
        return AppError::McpAuthentication;
    }
    failed(message)
}
