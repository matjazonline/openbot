use super::*;
use crate::application::workflow::activation::ActivationRequest;

pub(super) async fn update_on(
    tx: &mut Transaction<'_, Postgres>,
    write: WorkflowCheckpointWrite,
    update: WorkflowCheckpointUpdate,
    bound: &WriteBound,
) -> AppResult<End<Outcome>> {
    // A saved call is dispatched, or re-dispatched after approval, only under
    // current agent/model authority; replay of the preparation rechecks it too.
    if matches!(update, WorkflowCheckpointUpdate::PrepareInvocation(_))
        && authorize_on(tx, write.fence).await?.is_none()
    {
        return Ok(End::Rollback(WriteOutcome::OwnershipLost));
    }
    if !lock_owner(tx, write.fence, bound).await? {
        return Ok(End::Rollback(WriteOutcome::OwnershipLost));
    }
    let saved = load_on(tx, write.fence).await?.ok_or_else(missing)?;
    let current = decode(saved.id, saved.checkpoint.clone())?;
    live(tx, write.fence, bound).await?;
    if current.run_id != write.checkpoint {
        return Ok(End::Rollback(WriteOutcome::OwnershipLost));
    }
    let receives_truth = matches!(&update, WorkflowCheckpointUpdate::InvocationResult { .. });
    // The funnel for every tool result: direct calls, approved or parked continuations and
    // refusals. A result from an unaccounted target lowers the run's exactness (phase 05 E3).
    let lowers_exactness = match &update {
        WorkflowCheckpointUpdate::InvocationResult { invocation, .. } => {
            !result_exact(&saved.workflow_prepared, &current, *invocation)?
        }
        _ => false,
    };
    let (next, changed) = match current.apply(mutation(update)) {
        Ok(result) => result,
        Err(_) if current.revision != write.expected_revision => {
            return Ok(End::Rollback(WriteOutcome::RevisionConflict));
        }
        Err(error) => return Err(error),
    };
    if !changed {
        return Ok(End::Rollback(WriteOutcome::AlreadyApplied(current)));
    }
    if current.revision != write.expected_revision {
        return Ok(End::Rollback(WriteOutcome::RevisionConflict));
    }
    if saved.model_request_id.is_some() {
        return Err(AppError::Conflict("Model request is still active".into()));
    }
    if !current
        .executions
        .iter()
        .any(|execution| execution.generation == write.fence.generation.0)
    {
        return Err(AppError::Conflict(
            "Current execution time has not been reserved".into(),
        ));
    }
    if let Some(decision) = saved.model_blocked_decision {
        approved_settlement(tx, write.fence.scope, decision).await?;
    }
    // A received result is durable truth, not permission for another effect.
    // The next prepare/final/model boundary rechecks the shared root gate.
    if !receives_truth || saved.model_blocked_decision.is_some() {
        token_budget::require_progression(tx, write.fence.scope).await?;
    }
    if lowers_exactness {
        super::super::knowledge::lower_content_exact_on(
            tx,
            write.fence.scope.company,
            write.fence.scope.run,
        )
        .await?;
    }
    persist_on(tx, write.fence, &next, CheckpointOwnership::Progression).await?;
    agent_history::accepted_on(tx, write.fence, &saved.workflow_prepared, &next).await?;
    live(tx, write.fence, bound).await?;
    Ok(End::Commit(WriteOutcome::Applied(next)))
}

/// Whether the saved call's prepared target keeps the run exact. A call naming no prepared tool
/// (the host's unknown-tool refusal) is treated as unaccounted.
fn result_exact(
    prepared: &Value,
    checkpoint: &RunCheckpoint,
    invocation: crate::services::harness::runs::InvocationId,
) -> AppResult<bool> {
    let saved = checkpoint
        .invocations
        .iter()
        .find(|saved| saved.call.invocation_id == invocation)
        .ok_or_else(missing)?;
    let tools: Vec<crate::application::workflow::agent::PreparedTool> =
        serde_json::from_value(prepared["tools"].clone()).map_err(|_| invalid())?;
    let target = tools
        .iter()
        .find(|tool| tool.declared.as_str() == saved.call.tool_id.as_str())
        .map(|tool| &tool.target);
    Ok(crate::application::workflow::exactness::tool_result_exact(
        target,
    ))
}

fn mutation(update: WorkflowCheckpointUpdate) -> Mutation {
    match update {
        WorkflowCheckpointUpdate::BindTools(fingerprint) => Mutation::BindTools(fingerprint),
        WorkflowCheckpointUpdate::PrepareInvocation(invocation) => Mutation::Prepare(invocation),
        WorkflowCheckpointUpdate::InvocationResult { invocation, result } => {
            Mutation::Result(invocation, result)
        }
        WorkflowCheckpointUpdate::FinalOutput(output) => Mutation::Final(*output),
        WorkflowCheckpointUpdate::InvalidOutput => Mutation::InvalidOutput,
    }
}

async fn approved_settlement(
    db: &mut PgConnection,
    scope: ActivationRequest,
    decision: Uuid,
) -> AppResult<()> {
    let approved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_decisions AS decision
        JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id
        WHERE decision.company_id=$1 AND decision.run_id=$2 AND decision.execution_id=$3 AND decision.id=$4
        AND decision.subject_kind='token_allowance' AND decision.token_stage='settlement'
        AND decision.choice='approve' AND wait.state='completed')")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(decision)
        .fetch_one(db).await?;
    if !approved {
        return Err(AppError::Conflict(
            "Checkpoint token review has not authorized progression".into(),
        ));
    }
    Ok(())
}
