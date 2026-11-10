//! Test-only observation of the real guarded client; no scripted transport or result substitution.
use super::*;
use crate::app_error::AppResult;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Counts {
    calls: AtomicUsize,
    discoveries: AtomicUsize,
    sessions: AtomicUsize,
}
pub(super) struct Observation {
    pub(super) tool_calls: usize,
    pub(super) catalog_discoveries: usize,
}
pub(super) struct ObservedClient {
    inner: Arc<HttpMcpClient>,
    counts: Arc<Counts>,
}
impl ObservedClient {
    pub(super) fn new(inner: Arc<HttpMcpClient>) -> Self {
        Self {
            inner,
            counts: Arc::default(),
        }
    }
    pub(super) fn observation(&self) -> Observation {
        Observation {
            tool_calls: self.counts.calls.load(Ordering::SeqCst),
            catalog_discoveries: self.counts.discoveries.load(Ordering::SeqCst),
        }
    }
    fn observe(&self, inner: Box<dyn McpSession>) -> Box<dyn McpSession> {
        self.counts.sessions.fetch_add(1, Ordering::SeqCst);
        Box::new(ObservedSession {
            inner,
            counts: self.counts.clone(),
        })
    }
}
#[async_trait]
impl McpClient for ObservedClient {
    async fn connect_prepared(
        &self,
        endpoint: &McpEndpoint,
        token: SecretString,
        work: crate::services::mcp_client::McpPreparation,
    ) -> crate::services::mcp_client::McpPreparationResult<Box<dyn McpSession>> {
        let inner = Box::pin(self.inner.connect_prepared(endpoint, token, work)).await?;
        Ok(self.observe(inner))
    }
    async fn shutdown(&self) -> AppResult<()> {
        Box::pin(self.inner.shutdown()).await?;
        assert_eq!(
            self.counts.sessions.load(Ordering::SeqCst),
            0,
            "all observed sessions closed"
        );
        Ok(())
    }
    fn validate_endpoint(&self, endpoint: &McpEndpoint) -> AppResult<()> {
        self.inner.validate_endpoint(endpoint)
    }
    async fn connect(
        &self,
        endpoint: &McpEndpoint,
        token: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>> {
        // Keep the observation seam's future small when entering the real guarded transport.
        let inner = Box::pin(self.inner.connect(endpoint, token)).await?;
        Ok(self.observe(inner))
    }
}
struct ObservedSession {
    inner: Box<dyn McpSession>,
    counts: Arc<Counts>,
}
#[async_trait]
impl McpSession for ObservedSession {
    async fn discover(&self) -> AppResult<Vec<crate::entities::mcp::McpDiscoveredTool>> {
        self.counts.discoveries.fetch_add(1, Ordering::SeqCst);
        Box::pin(self.inner.discover()).await
    }
    async fn call(
        &self,
        name: &crate::entities::mcp::McpToolName,
        arguments: Value,
    ) -> AppResult<McpCallResult> {
        self.counts.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(self.inner.call(name, arguments)).await
    }
    async fn close(self: Box<Self>) -> AppResult<()> {
        let Self { inner, counts } = *self;
        let result = Box::pin(inner.close()).await;
        counts.sessions.fetch_sub(1, Ordering::SeqCst);
        result
    }
}
