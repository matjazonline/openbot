//! Test-only exact HTTPS metadata mapping to an owned counted protocol peer.
use super::*;
use crate::application::services::mcp_client::{
    McpPreparation, McpPreparationFailure, McpPreparationResult,
};

pub(crate) struct PinnedFixture {
    pub native: Arc<HttpMcpClient>,
    pub metadata: McpEndpoint,
    pub actual: McpEndpoint,
}
#[async_trait]
impl McpClient for PinnedFixture {
    fn validate_endpoint(&self, endpoint: &McpEndpoint) -> AppResult<()> {
        if endpoint == &self.metadata {
            Ok(())
        } else {
            Err(AppError::Forbidden("Unregistered fixture endpoint".into()))
        }
    }
    async fn connect(
        &self,
        _: &McpEndpoint,
        _: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>> {
        Err(AppError::Forbidden(
            "Fixture requires genuine guarded preparation".into(),
        ))
    }
    async fn connect_prepared(
        &self,
        endpoint: &McpEndpoint,
        token: SecretString,
        work: McpPreparation,
    ) -> McpPreparationResult<Box<dyn McpSession>> {
        self.validate_endpoint(endpoint)
            .map_err(|_| McpPreparationFailure::Denied)?;
        self.native
            .connect_prepared(&self.actual, token, work)
            .await
    }
    async fn shutdown(&self) -> AppResult<()> {
        self.native.shutdown().await
    }
}
