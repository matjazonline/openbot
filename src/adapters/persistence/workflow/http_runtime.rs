use super::*;
use crate::application::workflow::{lease::ClaimedWorkflow, polling::WorkflowStepKind};
use std::collections::BTreeMap;

struct RunFacts {
    actor: WorkflowActor,
    bundle: std::sync::Arc<PublishedBundle>,
    resources: BTreeMap<crate::domain::workflow::ResourceName, RuntimeResourceId>,
}
async fn facts(tx: &mut Transaction<'_, Postgres>, scope: ActionScope) -> AppResult<RunFacts> {
    let actor = action_authority::prelock_on(tx, scope)
        .await?
        .ok_or_else(missing)?;
    let (actor_id, bundle, resources): (Uuid, Vec<u8>, Value) = sqlx::query_as("SELECT actor_id,bundle,resources FROM workflow_runs WHERE company_id=$1 AND id=$2 AND state IN ('queued','running') AND deadline>clock_timestamp() FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    if actor_id != actor.user_id() {
        return Err(conflict());
    }
    Ok(RunFacts {
        actor,
        bundle: publication::restore_bundle(
            &bundle,
            &crate::adapters::workflow_source::WorkflowSourceDecoder,
        )?,
        resources: serde_json::from_value(resources).map_err(|_| invalid())?,
    })
}
fn request(
    scope: ActionScope,
    facts: &RunFacts,
    id: Uuid,
    tool: publication::ToolSnapshot,
    arguments: Value,
) -> AppResult<ActionRequest> {
    HttpRequest::parse(&arguments)?;
    let target = if tool.contract.connection.is_some() {
        ActionTarget::Connection {
            resource: id,
            resource_kind: TypeName::parse("http").map_err(|_| invalid())?,
        }
    } else {
        ActionTarget::Local {
            resource: id,
            resource_kind: TypeName::parse("http").map_err(|_| invalid())?,
        }
    };
    Ok(ActionRequest {
        scope,
        operation_key: tool.contract.name.clone(),
        target,
        contract: tool,
        arguments,
        context: ActionPolicyContext {
            actor: facts.actor.user_id(),
            capability_ceiling: facts
                .bundle
                .snapshots()
                .tools
                .iter()
                .map(|tool| tool.policy.capability.clone())
                .collect(),
            approval_required: false,
        },
    })
}
#[async_trait]
impl HttpActionPreparation for PostgresPersistence {
    async fn http_step_request(
        &self,
        claim: &ClaimedWorkflow,
        kind: &WorkflowStepKind,
    ) -> AppResult<ActionRequest> {
        let scope = ActionScope {
            company: claim.fence.scope.company,
            run: claim.fence.scope.run,
            execution: claim.fence.scope.execution,
        };
        let mut tx = begin_http(self).await?;
        let facts = facts(&mut tx, scope).await?;
        let (step, inputs): (String, Value) = sqlx::query_as("SELECT step_id,frozen_inputs FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
        if step != claim.activation.step.as_str() || inputs != claim.activation.inputs {
            return Err(missing());
        }
        if facts
            .bundle
            .compiled()
            .graph()
            .definition()
            .steps
            .get(&claim.activation.step)
            .is_none_or(|step| step.step_type.as_str() != kind.0)
        {
            return Err(missing());
        }
        let (name, slot, arguments) = match kind.0.as_str() {
            "http.request" => {
                let slot = inputs
                    .get("connection")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?;
                let mut arguments = inputs.clone();
                arguments
                    .as_object_mut()
                    .ok_or_else(invalid)?
                    .remove("connection");
                ("http.request", Some(slot), arguments)
            }
            "tool.call" => (
                inputs
                    .get("tool")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?,
                None,
                inputs.get("arguments").cloned().ok_or_else(invalid)?,
            ),
            _ => return Err(missing()),
        };
        let tool = facts
            .bundle
            .snapshots()
            .tools
            .iter()
            .find(|tool| {
                tool.contract.name.as_str() == name
                    && tool.contract.connection.as_ref().map(|s| s.as_str()) == slot
            })
            .cloned()
            .ok_or_else(missing)?;
        let id = match &tool.contract.connection {
            Some(slot) => facts.resources.get(slot).ok_or_else(missing)?.as_uuid(),
            None => registered_on(&mut tx, scope.company, &tool).await?,
        };
        policy_on(&mut tx, scope.company, id, &tool).await?;
        let result = request(scope, &facts, id, tool, arguments)?;
        tx.commit().await?;
        Ok(result)
    }
    async fn http_tool_request(
        &self,
        scope: ActionScope,
        call: &HttpToolCall,
    ) -> AppResult<ActionRequest> {
        let mut tx = begin_http(self).await?;
        let facts = facts(&mut tx, scope).await?;
        let tool = facts
            .bundle
            .snapshots()
            .tools
            .iter()
            .find(|tool| tool.contract.name == call.operation && tool.contract.connection.is_none())
            .cloned()
            .ok_or_else(missing)?;
        if registered_on(&mut tx, scope.company, &tool).await? != call.connection {
            return Err(missing());
        }
        let result = request(scope, &facts, call.connection, tool, call.arguments.clone())?;
        tx.commit().await?;
        Ok(result)
    }
}
