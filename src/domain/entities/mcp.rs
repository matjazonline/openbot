//! Company-owned MCP configuration. Agents carry references, never endpoint overrides or secrets.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_MCP_CONNECTIONS: usize = 64;
pub const MAX_MCP_SELECTIONS: usize = 8;
pub const MAX_EFFECTIVE_MCP_TOOLS: usize = 32;
pub const MAX_MCP_DISCOVERED_TOOLS: usize = 100;
pub const MAX_MCP_SCHEMA_BYTES: usize = 65_536;
pub const MAX_MCP_DISCOVERY_BYTES: usize = 1_048_576;
pub const MAX_MCP_SECRET_BYTES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct McpEndpoint(String);
impl McpEndpoint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for McpEndpoint {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        let url = url::Url::parse(&value).map_err(|_| "Invalid MCP endpoint URL")?;
        if value.len() > 2048
            || !matches!(url.scheme(), "https" | "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
        {
            return Err("MCP endpoint must be HTTP(S), at most 2048 bytes, without userinfo, query or fragment".into());
        }
        Ok(Self(url.to_string()))
    }
}
impl From<McpEndpoint> for String {
    fn from(value: McpEndpoint) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct McpToolName(String);
impl McpToolName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for McpToolName {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err("MCP tool name must contain 1–256 bytes without control characters".into());
        }
        Ok(Self(value))
    }
}
impl From<McpToolName> for String {
    fn from(value: McpToolName) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpAuth {
    None,
    Bearer,
}
impl McpAuth {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bearer => "bearer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpDiscoveredTool {
    pub name: McpToolName,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolRef {
    pub connection_id: Uuid,
    pub name: McpToolName,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyMcpConnection {
    pub id: Uuid,
    pub company_id: Uuid,
    pub slug: crate::entities::value_objects::McpConnectionSlug,
    pub endpoint: McpEndpoint,
    pub enabled: bool,
    pub auth: McpAuth,
    pub secret_set: bool,
    pub revision: i64,
    pub credential_revision: i64,
    pub discovered_tools: Vec<McpDiscoveredTool>,
    pub tool_grants: Vec<McpToolName>,
}

/// A revision belongs to the set, including an empty set; deletion cannot reset it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMcpSelection {
    pub company_id: Uuid,
    pub agent_id: Uuid,
    pub revision: i64,
    pub connection_ids: Vec<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_and_selection_schemas_reject_inline_authority() {
        for value in [
            "https://secret@example.com/mcp",
            "https://example.com/mcp?token=secret",
            "https://example.com/#fragment",
        ] {
            assert!(McpEndpoint::try_from(value.to_string()).is_err());
        }
        assert!(
            McpEndpoint::try_from(format!("https://example.com/{}", "x".repeat(2048))).is_err()
        );
        let value = serde_json::json!({"company_id":Uuid::nil(),"agent_id":Uuid::nil(),"revision":1,"connection_ids":[],"endpoint":"https://example.com"});
        assert!(serde_json::from_value::<AgentMcpSelection>(value).is_err());
    }
}
