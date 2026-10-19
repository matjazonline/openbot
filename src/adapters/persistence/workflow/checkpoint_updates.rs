use super::*;
use crate::application::workflow::activation::ActivationRequest;

pub(super) async fn update_on(
    tx: &mut Transaction<'_, Postgres>,
    write: WorkflowCheckpointWrite,
    update: WorkflowCheckpointUpdate,
    bound: &WriteBound,
) -> AppResult<End<Outcome>> {
    if !lock_owner(tx, write.fence, bound).await? {
        return Ok(End::Rollback(WriteOutcome::OwnershipLost));
    }
    let saved = load_on(tx, write.fence).await?.ok_or_else(missing)?;
    let current = decode(saved.checkpoint.clone())?;
    live(tx, write.fence, bound).await?;
    if current.run_id != write.checkpoint {
        return Ok(End::Rollback(WriteOutcome::OwnershipLost));
    }
    let receives_truth = matches!(&update, WorkflowCheckpointUpdate::InvocationResult { .. });
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
    persist_on(tx, write.fence, &next, CheckpointOwnership::Progression).await?;
    live(tx, write.fence, bound).await?;
    Ok(End::Commit(WriteOutcome::Applied(next)))
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
