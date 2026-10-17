//! Production composition of `mcp.call` steps and direct agent MCP tools: the shared
//! ActionService over the SQL dispatch writer with the company MCP authority, and the
//! guarded MCP client. Worker startup and agent tool exposure are wired in 06/08.
use crate::{
    adapters::{
        persistence::{
            PostgresPersistence,
            workflow::{action_dispatch::PostgresActionDispatch, mcp::McpActionAuthority},
        },
        workflow_http::NoLocal,
    },
    app_error::AppResult,
    application::{
        services::mcp_client::McpClient,
        workflow::{
            actions::RemoteDispatchObservation,
            lease::{ClaimedWorkflow, LeasePolicy, WorkflowFence, WorkflowHandlerOutcome},
            mcp::{McpActions, McpToolCall},
            polling::WorkflowStepKind,
            worker::WorkflowHandler,
        },
    },
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct McpRuntime {
    inner: McpActions<PostgresPersistence, PostgresActionDispatch<McpActionAuthority, NoLocal>>,
}
impl McpRuntime {
    /// `client` is the process's shared guarded MCP client (also used by legacy agent
    /// runs), so session capacity and shutdown supervision stay in one owner.
    pub fn new(
        persistence: PostgresPersistence,
        client: Arc<dyn McpClient>,
        lease: LeasePolicy,
    ) -> Self {
        Self {
            inner: McpActions::new(
                persistence.clone(),
                PostgresActionDispatch::new(persistence, McpActionAuthority, NoLocal),
                client,
                lease,
            ),
        }
    }
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: McpToolCall,
        cancel: &CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        // Bound the provider/SQL composition at the external seam.
        Box::pin(self.inner.call_tool(fence, call, cancel)).await
    }
    /// The step's completed result; MCP steps never suspend.
    pub async fn run(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> crate::application::workflow::lease::WorkflowHandlerResult {
        Box::pin(self.inner.run(kind, claim)).await
    }
}
#[async_trait]
impl WorkflowHandler for McpRuntime {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        self.inner.supports(kind)
    }
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        // Bound the external provider/SQL composition at the adapter seam.
        Box::pin(self.inner.execute(kind, claim)).await
    }
}
