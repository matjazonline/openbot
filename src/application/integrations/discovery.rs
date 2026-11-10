//! Owner-authorized configuration has no business dispatch capability.
use super::VerifiedOwner;
use crate::{
    domain::integrations::{authority::AuthorizationSnapshot, *},
    entities::mcp::*,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct OwnerDiscoveryContext {
    pub(crate) owner: VerifiedOwner,
    pub(crate) authorization: AuthorizationSnapshot,
    pub(crate) authority: Arc<dyn ConfigurationAuthority>,
    pub(crate) deadline: DateTime<Utc>,
    pub(crate) cancellation: CancellationToken,
}
pub use crate::domain::integrations::authority::ConfigurationAuthorityFacts;
#[async_trait]
pub trait ConfigurationAuthority: Send + Sync {
    async fn check(
        &self,
        context: &OwnerDiscoveryContext,
        gate: &dyn ConfigurationCredentialGate,
    ) -> IntegrationResult<()>;
}
#[async_trait]
pub trait ConfigurationCredentialGate: Send + Sync {
    async fn check(
        &self,
        context: &OwnerDiscoveryContext,
        facts: ConfigurationAuthorityFacts,
    ) -> IntegrationResult<()>;
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct DiscoveredCatalog(Vec<McpDiscoveredTool>);
impl DiscoveredCatalog {
    pub fn from_redacted(tools: Vec<McpDiscoveredTool>) -> IntegrationResult<Self> {
        if tools.len() > MAX_MCP_DISCOVERED_TOOLS
            || serde_json::to_vec(&tools)
                .map_err(|_| IntegrationError::Invalid)?
                .len()
                > MAX_MCP_DISCOVERY_BYTES
        {
            return Err(IntegrationError::Limit);
        }
        let mut names = std::collections::BTreeSet::new();
        for tool in &tools {
            if !names.insert(tool.name.as_str()) {
                return Err(IntegrationError::Invalid);
            }
            if tool.description.len() > 4096 {
                return Err(IntegrationError::Limit);
            }
            for schema in [Some(&tool.input_schema), tool.output_schema.as_ref()]
                .into_iter()
                .flatten()
            {
                if !schema.is_object() {
                    return Err(IntegrationError::Invalid);
                }
                crate::application::services::tool_schema::compile(schema)
                    .map_err(|_| IntegrationError::Invalid)?;
            }
        }
        Ok(Self(tools))
    }
    pub fn into_tools(self) -> Vec<McpDiscoveredTool> {
        self.0
    }
}

#[async_trait]
pub(crate) trait PreparedCatalog: Send + Sync {
    fn tools(&self) -> &[McpDiscoveredTool];
    async fn close(&self) -> IntegrationResult<()>;
}

impl<'de> serde::Deserialize<'de> for DiscoveredCatalog {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_redacted(Vec::<McpDiscoveredTool>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}
