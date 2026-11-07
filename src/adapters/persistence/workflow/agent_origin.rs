//! An agent call must agree with the real execution, immutable manifest and saved model call.
use super::*;
use crate::application::agent::{AgentModelToolName, AgentToolOrigin, ToolCallOrigin};
use crate::application::workflow::{
    actions::{
        ActionOriginAuthority, ActionPolicyContext, ActionRequest, ActionScope, ModelToolCallId,
    },
    agent_contract::{agent_action_snapshot, agent_action_target, validate_agent_action},
};
use serde_json::Value;

pub(super) async fn authority_on(
    db: &mut PgConnection,
    request: &ActionRequest,
    actor: WorkflowActor,
    bundle: &PublishedBundle,
) -> AppResult<ActionOriginAuthority> {
    match request.context.origin {
        ToolCallOrigin::Workflow => {
            require_workflow_origin(db, request.scope, bundle).await?;
            Ok(ActionOriginAuthority::Workflow)
        }
        ToolCallOrigin::Agent(origin) => {
            let selection = selected_on(db, request.scope, actor, bundle, origin).await?;
            validate_agent_action(request, &selection)?;
            Ok(ActionOriginAuthority::Agent(Box::new(selection)))
        }
        ToolCallOrigin::ChannelFinal(_) => Err(missing()),
    }
}

/// Prevent a missing agent origin from silently falling through to STEP/global authority.
pub(super) async fn require_workflow_origin(
    db: &mut PgConnection,
    scope: ActionScope,
    bundle: &PublishedBundle,
) -> AppResult<()> {
    let step: String = sqlx::query_scalar(
        "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_optional(db)
    .await?
    .ok_or_else(missing)?;
    let step = StepId::parse(step).map_err(|_| invalid())?;
    let definition = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&step)
        .ok_or_else(missing)?;
    if crate::application::workflow::registry::ModelStep::of(definition.step_type.as_str())
        .is_some()
    {
        return Err(missing());
    }
    Ok(())
}

async fn selected_on(
    db: &mut PgConnection,
    scope: ActionScope,
    actor: WorkflowActor,
    bundle: &PublishedBundle,
    origin: AgentToolOrigin,
) -> AppResult<ResolvedAgent> {
    let selection = agent_manifest::execution_selection_on(db, scope, actor, bundle).await?;
    if selection.mode() != AgentUseMode::Tools || selection.agent().agent_id != origin.agent {
        return Err(missing());
    }
    if selection.manifest_id() != origin.manifest || selection.revision().id != origin.revision {
        return Err(missing());
    }
    Ok(selection)
}

pub(super) async fn request_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActionScope,
    facts: &http::runtime::RunFacts,
    origin: AgentToolOrigin,
    call: &ModelToolCallId,
    arguments: Value,
) -> AppResult<ActionRequest> {
    let selection = selected_on(tx, scope, facts.actor, &facts.bundle, origin).await?;
    agent_authority::check_execution_on(tx, scope, selection.agent(), AgentUseMode::Tools).await?;
    let tool = selection
        .tools()
        .iter()
        .find(|tool| tool.binding.id == origin.binding)
        .ok_or_else(missing)?;
    let declared = AgentModelToolName::for_alias(&tool.binding.alias)?;
    saved_call_on(tx, scope, origin, &declared, call, &arguments).await?;
    tracing::debug!(company=%scope.company.as_uuid(), run=%scope.run.as_uuid(),
        agent=%origin.agent.as_uuid(), revision=%origin.revision.as_uuid(),
        binding=%origin.binding.as_uuid(), call=%call.as_str(), "Resolved saved agent action");
    let contract = agent_action_snapshot(scope.company, tool)?;
    let request = ActionRequest {
        scope,
        operation_key: contract.contract.name.clone(),
        target: agent_action_target(scope.company, tool)?,
        contract,
        arguments,
        context: ActionPolicyContext {
            origin: ToolCallOrigin::Agent(origin),
            actor: facts.actor.user_id(),
            capability_ceiling: selection
                .tools()
                .iter()
                .map(|tool| tool.policy.capability.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            approval_required: tool.approval_required,
        },
    };
    validate_agent_action(&request, &selection)?;
    Ok(request)
}

#[derive(sqlx::FromRow)]
struct SavedCall {
    id: Uuid,
    checkpoint: Value,
    workflow_prepared: Value,
}

async fn saved_call_on(
    db: &mut PgConnection,
    scope: ActionScope,
    origin: AgentToolOrigin,
    declared: &AgentModelToolName,
    call: &ModelToolCallId,
    arguments: &Value,
) -> AppResult<()> {
    let invocation = Uuid::parse_str(call.as_str()).map_err(|_| missing())?;
    let rows = sqlx::query_as::<_, SavedCall>(
        "SELECT harness.id,harness.checkpoint,harness.workflow_prepared FROM task_harness_runs AS harness \
         WHERE harness.company_id=$1 AND harness.workflow_run_id=$2 AND harness.workflow_execution_id=$3 AND harness.agent_id=$4 \
         AND EXISTS(SELECT 1 FROM jsonb_array_elements(harness.checkpoint->'invocations') AS invocation WHERE invocation->'call'->>'invocation_id'=$5) LIMIT 2"
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(origin.agent.as_uuid())
        .bind(invocation.to_string()).fetch_all(db).await?;
    let [row] = rows.as_slice() else {
        return Err(missing());
    };
    let tools: Vec<crate::application::workflow::agent::PreparedTool> =
        serde_json::from_value(row.workflow_prepared["tools"].clone()).map_err(|_| invalid())?;
    if !tools
        .iter()
        .any(|tool| tool.origin == origin && &tool.declared == declared)
    {
        return Err(missing());
    }
    let checkpoint =
        crate::adapters::persistence::harness_checkpoint::decode(row.id, row.checkpoint.clone())?;
    let saved = checkpoint
        .invocations
        .iter()
        .find(|saved| saved.call.invocation_id.0 == invocation)
        .ok_or_else(missing)?;
    if saved.call.tool_id.as_str() != declared.as_str() || &saved.call.arguments != arguments {
        return Err(missing());
    }
    use crate::services::harness::runs::InvocationState;
    match saved.state {
        InvocationState::Completed if saved.result.is_some() => Ok(()),
        InvocationState::Prepared | InvocationState::Ready | InvocationState::Indeterminate
            if checkpoint
                .pending_call()
                .is_some_and(|pending| pending.invocation_id.0 == invocation) =>
        {
            Ok(())
        }
        _ => Err(missing()),
    }
}

#[derive(sqlx::FromRow)]
struct OriginRun {
    actor_id: Uuid,
    bundle: Vec<u8>,
}

/// Called inside an existing company-authorized transaction at freeze and provider use.
pub(super) async fn current_on(
    db: &mut PgConnection,
    request: &ActionRequest,
) -> AppResult<ActionOriginAuthority> {
    let run = sqlx::query_as::<_, OriginRun>(
        "SELECT run.actor_id,stored.bundle FROM workflow_runs AS run \
         JOIN workflow_bundles AS stored ON stored.company_id=run.company_id AND stored.digest=run.bundle_digest \
         WHERE run.company_id=$1 AND run.id=$2 AND octet_length(stored.bundle)<=$3"
    ).bind(request.scope.company.as_uuid()).bind(request.scope.run.as_uuid()).bind(MAX_BUNDLE_BYTES as i32)
        .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    if run.actor_id != request.context.actor {
        return Err(missing());
    }
    let actor = WorkflowActor::authenticated(run.actor_id)?;
    let bundle = restore_bundle(
        &run.bundle,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    let authority = authority_on(db, request, actor, &bundle).await?;
    if let ActionOriginAuthority::Agent(selection) = &authority {
        agent_authority::check_execution_on(
            db,
            request.scope,
            selection.agent(),
            AgentUseMode::Tools,
        )
        .await?;
    }
    Ok(authority)
}

pub(super) async fn freeze_on(
    db: &mut PgConnection,
    request: &ActionRequest,
    call: Option<&ModelToolCallId>,
) -> AppResult<()> {
    let authority = current_on(db, request).await?;
    if let ActionOriginAuthority::Agent(selection) = authority {
        let ToolCallOrigin::Agent(origin) = request.context.origin else {
            return Err(missing());
        };
        let tool = selection
            .tools()
            .iter()
            .find(|tool| tool.binding.id == origin.binding)
            .ok_or_else(missing)?;
        let declared = AgentModelToolName::for_alias(&tool.binding.alias)?;
        saved_call_on(
            db,
            request.scope,
            origin,
            &declared,
            call.ok_or_else(missing)?,
            &request.arguments,
        )
        .await?;
    }
    Ok(())
}

/// Workflow children use the same immutable call provenance, without an ActionService receipt.
pub(super) async fn workflow_tool_on(
    db: &mut PgConnection,
    scope: ActionScope,
    call: &crate::application::workflow::tools::WorkflowToolCall,
    actor: WorkflowActor,
    bundle: &PublishedBundle,
) -> AppResult<ActionOriginAuthority> {
    match call.origin {
        ToolCallOrigin::Workflow => {
            require_workflow_origin(db, scope, bundle).await?;
            if !bundle.snapshots().tools.iter().any(|tool| {
                tool.contract.name == call.name && tool.workflow.as_ref() == Some(&call.pin)
            }) {
                return Err(missing());
            }
            Ok(ActionOriginAuthority::Workflow)
        }
        ToolCallOrigin::Agent(origin) => {
            let selection = selected_on(db, scope, actor, bundle, origin).await?;
            let tool = selection
                .tools()
                .iter()
                .find(|tool| tool.binding.id == origin.binding)
                .ok_or_else(missing)?;
            let snapshot = agent_action_snapshot(scope.company, tool)?;
            if snapshot.contract.name != call.name || snapshot.workflow.as_ref() != Some(&call.pin)
            {
                return Err(missing());
            }
            let declared = AgentModelToolName::for_alias(&tool.binding.alias)?;
            saved_call_on(
                db,
                scope,
                origin,
                &declared,
                &ModelToolCallId::parse(call.call.as_uuid().to_string()).map_err(|_| invalid())?,
                &call.arguments,
            )
            .await?;
            Ok(ActionOriginAuthority::Agent(Box::new(selection)))
        }
        ToolCallOrigin::ChannelFinal(_) => Err(missing()),
    }
}
