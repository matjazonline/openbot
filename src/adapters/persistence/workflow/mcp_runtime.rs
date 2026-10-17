use super::*;
use crate::adapters::persistence::workflow::http::runtime::{
    RunFacts, context, facts, frozen_inputs,
};
use crate::application::workflow::{lease::ClaimedWorkflow, polling::WorkflowStepKind};
use crate::domain::workflow::ResourceName;

/// One request shape for steps and direct tools: the bundle's frozen snapshot for exactly
/// (tool, slot), the slot resolved only through the run's saved binding.
fn request(
    scope: ActionScope,
    facts: &RunFacts,
    slot: &ResourceName,
    tool: &str,
    arguments: Value,
) -> AppResult<ActionRequest> {
    let contract = facts
        .bundle
        .snapshots()
        .tools
        .iter()
        .find(|snapshot| {
            snapshot.contract.name.as_str() == tool
                && snapshot.contract.connection.as_ref() == Some(slot)
        })
        .cloned()
        .ok_or_else(missing)?;
    let id = facts.resources.get(slot).ok_or_else(missing)?.as_uuid();
    if !arguments.is_object() {
        return Err(crate::application::workflow::mcp::invalid());
    }
    Ok(ActionRequest {
        scope,
        operation_key: contract.contract.name.clone(),
        target: ActionTarget::Connection {
            resource: id,
            resource_kind: TypeName::parse("mcp").map_err(|_| invalid())?,
        },
        contract,
        arguments,
        context: context(facts),
    })
}

#[async_trait]
impl McpActionPreparation for PostgresPersistence {
    async fn mcp_step_request(
        &self,
        claim: &ClaimedWorkflow,
        kind: &WorkflowStepKind,
    ) -> AppResult<ActionRequest> {
        if kind.0 != "mcp.call" {
            return Err(missing());
        }
        let scope = ActionScope {
            company: claim.fence.scope.company,
            run: claim.fence.scope.run,
            execution: claim.fence.scope.execution,
        };
        let mut tx = begin_http(self).await?;
        let facts = facts(&mut tx, scope).await?;
        let inputs = frozen_inputs(&mut tx, &facts, claim, kind).await?;
        let field = |name: &str| {
            inputs
                .get(name)
                .ok_or_else(crate::application::workflow::mcp::invalid)
        };
        let slot = ResourceName::parse(field("connection")?.as_str().unwrap_or_default())
            .map_err(|_| crate::application::workflow::mcp::invalid())?;
        let tool = field("tool")?
            .as_str()
            .ok_or_else(crate::application::workflow::mcp::invalid)?;
        let result = request(scope, &facts, &slot, tool, field("arguments")?.clone())?;
        current_on(&mut tx, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
    async fn mcp_tool_request(
        &self,
        scope: ActionScope,
        call: &McpToolCall,
    ) -> AppResult<ActionRequest> {
        let mut tx = begin_http(self).await?;
        let facts = facts(&mut tx, scope).await?;
        let result = request(
            scope,
            &facts,
            &call.slot,
            call.tool.as_str(),
            call.arguments.clone(),
        )?;
        current_on(&mut tx, &result).await?;
        tx.commit().await?;
        Ok(result)
    }
}
