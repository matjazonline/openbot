//! Runtime capabilities for entering the host's effect protocol. None are wire DTOs.
use super::{AuthorizedInvocation, ExecutionOutcome, ProviderData};
use crate::domain::integrations::*;
use async_trait::async_trait;

/// The host retains reservation, replay, fencing, receipt and effect ownership.
#[async_trait]
pub trait BusinessDispatch: Send + Sync {
    async fn dispatch(
        &self,
        invocation: &AuthorizedInvocation,
        call: &dyn PreparedBusinessCall,
    ) -> IntegrationResult<ExecutionOutcome>;
}

/// Private transport state stays behind this runtime capability, never in an outcome.
#[async_trait]
pub trait PreparedBusinessCall: Send + Sync {
    async fn call(&self) -> IntegrationResult<ProviderData>;
    async fn close(&self) -> IntegrationResult<()>;
}

pub(crate) struct DeniedBusinessDispatch;
#[async_trait]
impl BusinessDispatch for DeniedBusinessDispatch {
    async fn dispatch(
        &self,
        _: &AuthorizedInvocation,
        _: &dyn PreparedBusinessCall,
    ) -> IntegrationResult<ExecutionOutcome> {
        Err(IntegrationError::Denied)
    }
}
