use crate::application::integrations::*;
use crate::domain::integrations::IntegrationResult;
use async_trait::async_trait;
pub(super) struct NoExecution;
#[async_trait]
impl ExecutionGateway for NoExecution {
    async fn execute_http(
        &self,
        _: AuthorizedInvocation,
        _: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        panic!("invalid projection reached credential execution")
    }
    async fn call_mcp(
        &self,
        _: AuthorizedInvocation,
        _: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        panic!("invalid projection reached credential execution")
    }
    async fn discover_mcp(&self, _: OwnerDiscoveryContext) -> IntegrationResult<DiscoveredCatalog> {
        panic!("invalid projection reached credential discovery")
    }
}
