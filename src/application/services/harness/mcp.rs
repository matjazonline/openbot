//! A run-resolved MCP capability, distinct from the closed native/built-in catalogue.
//! The HTTP adapter owns protocol types; the application host owns current grant/revision checks.
use super::ToolInvocation;
use crate::{app_error::AppResult, entities::mcp::McpToolRef};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct McpToolDeclaration {
    pub identity: McpToolRef,
    pub description: String,
    pub input_schema: Value,
    pub definition_revision: i64,
    pub credential_revision: i64,
    pub selection_revision: i64,
}

/// Construct only after resolving this agent's company selections, bounded discovery and
/// fingerprint checks. Disabled/ungranted connections must perform no network work.
#[async_trait]
pub trait HarnessMcpToolHost: Send + Sync {
    fn available(&self) -> &[McpToolDeclaration];

    /// Recheck current tenant, selection, grant, schema, credentials and lease before dispatch.
    /// A possibly committed remote effect must return an error, never be automatically retried.
    /// Implementation owns sessions and cancellation and must await their shutdown.
    async fn invoke(
        &self,
        declaration: &McpToolDeclaration,
        correlation_id: &str,
        args: Value,
    ) -> AppResult<ToolInvocation>;
}

/// Fixed 52-character ASCII name: stable across labels/slug edits and distinct across connections.
pub fn mcp_model_id(
    identity: &crate::entities::mcp::McpToolRef,
) -> crate::entities::value_objects::ToolId {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(identity.connection_id.as_bytes());
    hash.update(identity.name.as_str().as_bytes());
    let hash = format!("{:x}", hash.finalize());
    crate::entities::value_objects::ToolId::from(format!("mcp_{}", &hash[..48]))
}
