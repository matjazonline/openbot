//! Company MCP tools as workflow actions. The company MCP catalog owns connections (fixed
//! endpoint), encrypted credentials, reviewed discovery and grants; this module adds the
//! trusted workflow policy for a granted tool and the step/direct-tool runtime over the
//! shared ActionService. Credentials never enter actions, bundles or receipts.
use super::{
    authorization::WorkflowActor,
    compiler::{Schema, SourceSpan},
    publication::{ActionRecovery, ToolSnapshot},
};
use crate::{
    app_error::{AppError, AppResult},
    domain::workflow::{CompanyId, TypeName},
    entities::mcp::{McpDiscoveredTool, McpToolName},
};
use async_trait::async_trait;
use serde_json::{Value, json};
use uuid::Uuid;
#[path = "mcp_runtime.rs"]
mod runtime;
pub use runtime::*;

pub const MAX_MCP_OPERATION_BYTES: usize = 65_536;
pub const MAX_MCP_OPERATIONS: usize = 256;

/// The frozen output contract of a tool that declares no `outputSchema`: MCP structured
/// content is always a JSON object.
pub fn undeclared_output_schema() -> Value {
    json!({"type":"object"})
}

/// The (input, output) schemas a reviewed or live discovery entry offers.
fn offered(tool: &McpDiscoveredTool) -> (&Value, Value) {
    (
        &tool.input_schema,
        tool.output_schema
            .clone()
            .unwrap_or_else(undeclared_output_schema),
    )
}

/// True only when discovery still offers exactly the frozen tool: same name, identical input
/// and output schemas. Descriptions and annotations are ignored; they grant nothing.
pub fn discovery_matches(frozen: &ToolSnapshot, discovered: &[McpDiscoveredTool]) -> bool {
    discovered.iter().any(|tool| {
        let (input, output) = offered(tool);
        tool.name.as_str() == frozen.contract.name.as_str()
            && *input == frozen.contract.input_schema
            && output == frozen.contract.output_schema
    })
}

/// A committed `mcp.call` envelope reporting a tool-level error.
pub fn mcp_result_is_error(result: &Value) -> bool {
    result.get("isError") == Some(&Value::Bool(true))
}

/// Trusted workflow policy for one granted MCP tool, installed by a company administrator.
/// MCP annotations, discovery metadata and session ids never supply any part of it.
#[derive(Clone)]
pub struct McpOperation {
    pub tool: ToolSnapshot,
    pub approval_required: bool,
}
impl McpOperation {
    /// Validates the policy against the connection's current reviewed catalog entry.
    /// Stored snapshots are re-validated with this at every use.
    pub fn validate(
        &self,
        company: CompanyId,
        reviewed: Option<&McpDiscoveredTool>,
        granted: bool,
    ) -> AppResult<()> {
        let tool = &self.tool;
        let name = tool.contract.name.as_str();
        if tool.company_id != company
            || tool.contract.connection.is_some()
            || tool.policy.recovery != ActionRecovery::Reconcile
            || tool.policy.policy_revision == 0
            || McpToolName::try_from(name.to_owned()).is_err()
            || serde_json::to_vec(tool).map_err(|_| invalid())?.len() > MAX_MCP_OPERATION_BYTES
            || !granted
            || !reviewed
                .is_some_and(|reviewed| discovery_matches(tool, std::slice::from_ref(reviewed)))
        {
            return Err(invalid());
        }
        let span = SourceSpan {
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        };
        for schema in [&tool.contract.input_schema, &tool.contract.output_schema] {
            Schema::compile(schema.clone(), "/mcp-operation", span).map_err(|_| invalid())?;
        }
        Ok(())
    }
}

/// `expected_version`: None registers a new approval only; Some(v) replaces exactly the
/// approval at version v. Versions are random per write and never reissued, so a stale
/// writer cannot match an approval recreated after a revoke or cascade (no ABA).
pub struct McpOperationWrite {
    pub connection: Uuid,
    /// The reviewed catalog state (discovery and grants) the administrator approved against.
    pub catalog_revision: i64,
    pub expected_version: Option<Uuid>,
    pub operation: McpOperation,
}
pub struct McpOperationRevoke {
    pub connection: Uuid,
    pub catalog_revision: i64,
    pub name: TypeName,
    pub version: Uuid,
}

#[async_trait]
pub trait McpOperations: Send + Sync {
    /// Returns the new approval version.
    async fn register_mcp_operation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        write: McpOperationWrite,
    ) -> AppResult<Uuid>;
    async fn revoke_mcp_operation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        revoke: McpOperationRevoke,
    ) -> AppResult<()>;
}

pub fn invalid() -> AppError {
    AppError::BadRequest("Invalid MCP action configuration or input".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::workflow::{
        publication::{ActionEffect, ApprovedActionPolicy},
        registry::ToolContract,
    };

    fn snapshot(output: Value) -> ToolSnapshot {
        ToolSnapshot {
            workflow: None,
            company_id: CompanyId::new(Uuid::nil()),
            contract: ToolContract {
                name: TypeName::parse("lookup").unwrap(),
                connection: None,
                input_schema: json!({"type":"object"}),
                output_schema: output,
            },
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("mcp.lookup").unwrap(),
                policy_revision: 1,
                effect: ActionEffect::Read,
                recovery: ActionRecovery::Reconcile,
            },
        }
    }
    fn tool(input: Value, output: Option<Value>) -> McpDiscoveredTool {
        McpDiscoveredTool {
            name: "lookup".to_string().try_into().unwrap(),
            description: "any".into(),
            input_schema: input,
            output_schema: output,
        }
    }

    #[test]
    fn workflow_mcp_discovery_must_offer_exactly_the_frozen_schemas() {
        let frozen = snapshot(undeclared_output_schema());
        assert!(discovery_matches(
            &frozen,
            &[tool(json!({"type":"object"}), None)]
        ));
        assert!(!discovery_matches(&frozen, &[]));
        assert!(!discovery_matches(
            &frozen,
            &[tool(json!({"type":"object","required":["a"]}), None)]
        ));
        assert!(!discovery_matches(
            &frozen,
            &[tool(
                json!({"type":"object"}),
                Some(json!({"type":"object","required":["x"]}))
            )]
        ));
        let declared = snapshot(json!({"type":"object","properties":{"x":{"type":"integer"}}}));
        assert!(discovery_matches(
            &declared,
            &[tool(
                json!({"type":"object"}),
                Some(declared.contract.output_schema.clone())
            )]
        ));
        assert!(!discovery_matches(
            &declared,
            &[tool(json!({"type":"object"}), None)]
        ));
    }

    #[test]
    fn workflow_mcp_operation_requires_grant_review_and_reconcile_recovery() {
        let company = CompanyId::new(Uuid::nil());
        let reviewed = tool(json!({"type":"object"}), None);
        let operation = McpOperation {
            tool: snapshot(undeclared_output_schema()),
            approval_required: false,
        };
        assert!(operation.validate(company, Some(&reviewed), true).is_ok());
        assert!(operation.validate(company, Some(&reviewed), false).is_err());
        assert!(operation.validate(company, None, true).is_err());
        assert!(
            operation
                .validate(CompanyId::new(Uuid::from_u128(9)), Some(&reviewed), true)
                .is_err()
        );
        let mut replay = operation.clone();
        replay.tool.policy.recovery = ActionRecovery::SafeRepeat;
        assert!(replay.validate(company, Some(&reviewed), true).is_err());
        let mut slotted = operation.clone();
        slotted.tool.contract.connection =
            Some(crate::domain::workflow::ResourceName::parse("svc").unwrap());
        assert!(slotted.validate(company, Some(&reviewed), true).is_err());
        let mut revision = operation;
        revision.tool.policy.policy_revision = 0;
        assert!(revision.validate(company, Some(&reviewed), true).is_err());
    }

    #[test]
    fn workflow_mcp_error_envelope_classification() {
        assert!(mcp_result_is_error(&json!({"content":[],"isError":true})));
        assert!(!mcp_result_is_error(&json!({"content":[],"isError":false})));
        assert!(!mcp_result_is_error(&json!({"content":[]})));
    }
}
