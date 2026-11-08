//! Unconfigured effect ports deny requests; app discovery advertises no available apps.
use super::*;
use crate::domain::integrations::{authority::UseGrant, *};
use async_trait::async_trait;
pub(crate) struct UnavailableIntegrations;
#[async_trait]
impl ConnectGateway for UnavailableIntegrations {
    async fn connectable_apps(&self) -> IntegrationResult<Vec<ConnectApp>> {
        Ok(vec![])
    }
    async fn finish_connect(
        &self,
        _: &VerifiedOwner,
        _: ConnectCallback,
        _: ConnectWork,
    ) -> IntegrationResult<ConnectAttempt> {
        Err(IntegrationError::Unavailable)
    }
    async fn cancel_connect(
        &self,
        _: &VerifiedOwner,
        _: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt> {
        Err(IntegrationError::Unavailable)
    }
    async fn begin_connect(
        &self,
        _: &VerifiedOwner,
        _: BeginConnect,
    ) -> IntegrationResult<ConnectStart> {
        Err(IntegrationError::Unavailable)
    }
    async fn get_connect_attempt(
        &self,
        _: &VerifiedOwner,
        _: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt> {
        Err(IntegrationError::Unavailable)
    }
}
#[async_trait]
impl GrantGateway for UnavailableIntegrations {
    async fn create_use_grant(&self, _: OwnerConfirmation) -> IntegrationResult<UseGrant> {
        Err(IntegrationError::Unavailable)
    }
    async fn revoke_use_grant(
        &self,
        _: &VerifiedOwner,
        _: UseGrantId,
        _: GrantRevision,
    ) -> IntegrationResult<()> {
        Err(IntegrationError::Unavailable)
    }
}
#[async_trait]
impl ExecutionGateway for UnavailableIntegrations {
    async fn discover_mcp(&self, _: OwnerDiscoveryContext) -> IntegrationResult<DiscoveredCatalog> {
        Err(IntegrationError::Unavailable)
    }
    async fn execute_http(
        &self,
        _: AuthorizedInvocation,
        _: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        Err(IntegrationError::Unavailable)
    }
    async fn call_mcp(
        &self,
        _: AuthorizedInvocation,
        _: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        Err(IntegrationError::Unavailable)
    }
}
