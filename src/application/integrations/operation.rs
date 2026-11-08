//! Portable operation projection, independently checked against persisted host authority.
use crate::{domain::integrations::*, entities::mcp::McpToolName};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationTarget {
    Http,
    Mcp {
        tool: McpToolName,
        input_schema: Value,
        output_schema: Option<Value>,
    },
}
impl OperationTarget {
    pub fn validate(&self) -> IntegrationResult<()> {
        if let Self::Mcp {
            input_schema,
            output_schema,
            ..
        } = self
        {
            for schema in [Some(input_schema), output_schema.as_ref()]
                .into_iter()
                .flatten()
            {
                if !schema.is_object() {
                    return Err(IntegrationError::Invalid);
                }
                if serde_json::to_vec(schema)
                    .map_err(|_| IntegrationError::Invalid)?
                    .len()
                    > crate::entities::mcp::MAX_MCP_SCHEMA_BYTES
                {
                    return Err(IntegrationError::Limit);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreDispatchFailure {
    Denied,
    Invalid,
    Retry,
    ReconnectRequired,
    Configuration,
    Cancelled,
}
