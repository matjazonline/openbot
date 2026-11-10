//! Display stored reviews without implying that stale policy is dispatch authority.
use super::*;

pub(super) const SQL: &str = "SELECT name, snapshot, approval_required, version \
    FROM workflow_mcp_operations \
    WHERE company_id=$1 AND connection_id=$2 AND revoked_at IS NULL \
      AND ($3::text IS NULL OR name=$3) AND ($4::text IS NULL OR name>$4) \
    ORDER BY name LIMIT $5";

#[derive(sqlx::FromRow)]
pub(super) struct Row {
    pub(super) name: String,
    snapshot: Value,
    approval_required: bool,
    version: Uuid,
}

impl Row {
    pub(super) fn review(self, connection: &CompanyMcpConnection) -> AppResult<McpOperationReview> {
        let corrupt = || AppError::Database("Stored MCP operation snapshot is invalid".into());
        let tool: publication::ToolSnapshot =
            serde_json::from_value(self.snapshot).map_err(|_| corrupt())?;
        if tool.contract.name.as_str() != self.name {
            return Err(corrupt());
        }
        let operation = McpOperation {
            tool,
            approval_required: self.approval_required,
        };
        operation
            .validate_snapshot(CompanyId::new(connection.company_id))
            .map_err(|_| corrupt())?;
        Ok(McpOperationReview {
            version: self.version,
            schema_current: discovery_matches(&operation.tool, &connection.discovered_tools),
            granted: connection
                .tool_grants
                .iter()
                .any(|name| name.as_str() == self.name),
            operation,
        })
    }
}
