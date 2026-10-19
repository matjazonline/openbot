//! Authority → local workflow fence → checkpoint; no legacy receipt projection.
use super::super::harness_checkpoint::{decode, state_name};
use super::{
    persistence_bound::{Attempt, End, Evidence, WriteBound},
    *,
};
use crate::{
    application::workflow::{actions::ActionScope, checkpoints::*, lease::*},
    services::harness::runs::{
        ExecutionReservation, MAX_CHECKPOINT_BYTES, Mutation, RunCheckpoint, RunState,
        SavedModelTurn, WriteOutcome,
    },
};
use serde_json::Value;

type Outcome = WriteOutcome<RunCheckpoint>;

#[derive(sqlx::FromRow)]
struct Saved {
    checkpoint: Value,
    workflow_prepared: Value,
    model_blocked_decision: Option<Uuid>,
    model_request_id: Option<Uuid>,
    model_generation: Option<Uuid>,
    model_worker_id: Option<Uuid>,
    model_attempt: Option<i32>,
}

#[async_trait]
impl WorkflowCheckpointStore for PostgresPersistence {
    async fn update_workflow_checkpoint(
        &self,
        write: WorkflowCheckpointWrite,
        update: WorkflowCheckpointUpdate,
    ) -> AppResult<Outcome> {
        let bound = WriteBound::start("checkpoint.update", write.lease_policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = updates::update_on(&mut tx, write, update, &bound).await;
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(result) => Ok(result),
            Attempt::Failed(error) => Err(error),
            // Exact replay can resolve a saved mutation on the next live read;
            // an ambiguous acknowledgment never authorizes the next phase.
            Attempt::Ambiguous => Err(bound.unresolved()),
        }
    }

    async fn open_workflow_checkpoint(
        &self,
        fence: WorkflowFence,
        spec: &WorkflowCheckpointSpec,
        policy: LeasePolicy,
    ) -> AppResult<Outcome> {
        validate_spec(fence, spec)?;
        let proposed = RunCheckpoint::new(
            spec.identity.clone(),
            spec.initial_prompt.clone(),
            spec.policy,
        )?;
        let bound = WriteBound::start("checkpoint.open", policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = open_on(&mut tx, fence, spec, &proposed, &bound).await;
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(value) => Ok(value),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(async {
                        let mut conn = bound.acquire(&self.pool).await?;
                        let mut tx = bound.begin(&mut conn).await?;
                        let result = async {
                            if !lock_owner(&mut tx, fence, &bound).await? {
                                return Err(bound.unresolved());
                            }
                            let Some(saved) = load_on(&mut tx, fence).await? else {
                                return Ok(Evidence::NotApplied);
                            };
                            let run = matching(saved, spec)?;
                            Ok(Evidence::Applied(WriteOutcome::AlreadyApplied(run)))
                        }
                        .await;
                        bound.roll_back(tx, result).await
                    }))
                    .await
            }
        }
    }

    async fn load_workflow_checkpoint(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
    ) -> AppResult<Outcome> {
        let bound = WriteBound::read_only("checkpoint.load", policy);
        bound
            .read(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = async {
                    if !lock_owner(&mut tx, fence, &bound).await? {
                        return Ok(WriteOutcome::OwnershipLost);
                    }
                    let run = load_on(&mut tx, fence).await?.ok_or_else(missing)?;
                    let run = decode(run.checkpoint)?;
                    live(&mut tx, fence, &bound).await?;
                    Ok(WriteOutcome::AlreadyApplied(run))
                }
                .await;
                bound.roll_back(tx, result).await
            }))
            .await
    }

    async fn reserve_workflow_model(
        &self,
        write: WorkflowCheckpointWrite,
        request: WorkflowModelRequest,
        notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
    ) -> AppResult<WorkflowModelAdmission> {
        let bound = WriteBound::start("checkpoint.model.reserve", write.lease_policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = models::reserve_on(&mut tx, write, &request, notices, &bound).await;
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(result) => Ok(result),
            Attempt::Failed(error) => Err(error),
            // Even a later successful lookup cannot grant provider entry.
            Attempt::Ambiguous => Err(bound.unresolved()),
        }
    }

    async fn settle_workflow_model(
        &self,
        write: WorkflowCheckpointWrite,
        turn: SavedModelTurn,
        notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
    ) -> AppResult<WorkflowModelSettlement> {
        let bound = WriteBound::start("checkpoint.model.settle", write.lease_policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = models::settle_on(&mut tx, write, &turn, notices, &bound).await;
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(result) => Ok(result),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => Err(bound.unresolved()),
        }
    }

    async fn reserve_workflow_execution(
        &self,
        write: WorkflowCheckpointWrite,
        allowance_ms: u64,
    ) -> AppResult<Outcome> {
        let mutation = Mutation::Execution(ExecutionReservation {
            generation: write.fence.generation.0,
            allowance_ms,
            started_at: chrono::Utc::now(),
            replayed_invocations: 0,
        });
        let bound = WriteBound::start("checkpoint.execution", write.lease_policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = execution_on(&mut tx, write, mutation, &bound).await;
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(value) => Ok(value),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(async {
                        let mut conn = bound.acquire(&self.pool).await?;
                        let mut tx = bound.begin(&mut conn).await?;
                        let evidence =
                            execution_evidence(&mut tx, write, allowance_ms, &bound).await;
                        bound.roll_back(tx, evidence).await
                    }))
                    .await
            }
        }
    }
}

fn validate_spec(fence: WorkflowFence, spec: &WorkflowCheckpointSpec) -> AppResult<()> {
    if spec.identity.company_id != fence.scope.company.as_uuid()
        || spec.identity.task_id != fence.scope.job.0
        || spec.identity.agent_id.is_nil()
        || !spec.prepared.is_object()
        || serde_json::to_vec(&spec.prepared)
            .map_err(|_| invalid())?
            .len()
            > MAX_CHECKPOINT_BYTES
    {
        return Err(AppError::BadRequest(
            "Invalid workflow checkpoint preparation".into(),
        ));
    }
    Ok(())
}

async fn lock_owner(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    bound: &WriteBound,
) -> AppResult<bool> {
    let scope = ActionScope {
        company: fence.scope.company,
        run: fence.scope.run,
        execution: fence.scope.execution,
    };
    let Some(actor) = action_authority::prelock_on(tx, scope).await? else {
        return Ok(false);
    };
    if !lease::lock_fence(tx, fence, bound.server_timeout()).await? {
        return Ok(false);
    }
    let current: Uuid =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(&mut **tx)
            .await?;
    Ok(current == actor.user_id()
        && lease::live_window(tx, fence, bound.started())
            .await?
            .is_some())
}

async fn live(tx: &mut PgConnection, fence: WorkflowFence, bound: &WriteBound) -> AppResult<()> {
    if lease::live_window(tx, fence, bound.started())
        .await?
        .is_none()
    {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        ));
    }
    Ok(())
}

async fn load_on(tx: &mut PgConnection, fence: WorkflowFence) -> AppResult<Option<Saved>> {
    sqlx::query_as(
        "SELECT checkpoint,workflow_prepared,model_blocked_decision,model_request_id,model_generation,model_worker_id,model_attempt FROM task_harness_runs
        WHERE owner_kind='workflow' AND company_id=$1 AND workflow_run_id=$2
          AND workflow_execution_id=$3 AND task_id=$4 FOR UPDATE",
    )
    .bind(fence.scope.company.as_uuid())
    .bind(fence.scope.run.as_uuid())
    .bind(fence.scope.execution.as_uuid())
    .bind(fence.scope.job.0)
    .fetch_optional(tx)
    .await
    .map_err(Into::into)
}

fn matching(saved: Saved, spec: &WorkflowCheckpointSpec) -> AppResult<RunCheckpoint> {
    let run = decode(saved.checkpoint)?;
    if run.identity != spec.identity
        || run.policy != spec.policy
        || saved.workflow_prepared != spec.prepared
        || run.messages.first()
            != Some(&crate::services::harness::runs::ConversationMessage::User {
                text: spec.initial_prompt.clone(),
            })
    {
        return Err(AppError::Conflict(
            "Workflow checkpoint preparation changed".into(),
        ));
    }
    Ok(run)
}

async fn open_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    spec: &WorkflowCheckpointSpec,
    proposed: &RunCheckpoint,
    bound: &WriteBound,
) -> AppResult<End<Outcome>> {
    if !lock_owner(tx, fence, bound).await? {
        return Ok(End::Rollback(WriteOutcome::OwnershipLost));
    }
    let owned: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM agents WHERE company_id=$1 AND id=$2)")
            .bind(fence.scope.company.as_uuid())
            .bind(spec.identity.agent_id)
            .fetch_one(&mut **tx)
            .await?;
    if !owned {
        return Ok(End::Rollback(WriteOutcome::OwnershipLost));
    }
    if let Some(saved) = load_on(tx, fence).await? {
        let run = matching(saved, spec)?;
        live(tx, fence, bound).await?;
        return Ok(End::Rollback(WriteOutcome::AlreadyApplied(run)));
    }
    sqlx::query("INSERT INTO task_harness_runs
        (id,company_id,task_id,agent_id,owner_kind,workflow_run_id,workflow_execution_id,
         workflow_generation,workflow_worker_id,workflow_attempt,workflow_prepared,schema_version,revision,state,checkpoint)
        VALUES ($1,$2,$3,$4,'workflow',$5,$6,$7,$8,$9,$10,1,0,'active',$11)")
        .bind(proposed.run_id.0).bind(fence.scope.company.as_uuid()).bind(fence.scope.job.0).bind(spec.identity.agent_id)
        .bind(fence.scope.run.as_uuid()).bind(fence.scope.execution.as_uuid()).bind(fence.generation.0)
        .bind(fence.worker.0).bind(fence.attempt.0).bind(&spec.prepared).bind(serde_json::to_value(proposed).map_err(|_| invalid())?)
        .execute(&mut **tx).await?;
    live(tx, fence, bound).await?;
    Ok(End::Commit(WriteOutcome::Applied(proposed.clone())))
}

async fn execution_on(
    tx: &mut Transaction<'_, Postgres>,
    write: WorkflowCheckpointWrite,
    mutation: Mutation,
    bound: &WriteBound,
) -> AppResult<End<Outcome>> {
    if !lock_owner(tx, write.fence, bound).await? {
        return Ok(End::Rollback(WriteOutcome::OwnershipLost));
    }
    let saved = load_on(tx, write.fence).await?.ok_or_else(missing)?;
    let current = decode(saved.checkpoint)?;
    live(tx, write.fence, bound).await?;
    if current.run_id != write.checkpoint {
        return Ok(End::Rollback(WriteOutcome::OwnershipLost));
    }
    if current.state != RunState::Active {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::CheckpointDenied,
        ));
    }
    if current.revision != write.expected_revision {
        return Ok(End::Rollback(match current.apply(mutation) {
            Ok((_, false)) => WriteOutcome::AlreadyApplied(current),
            _ => WriteOutcome::RevisionConflict,
        }));
    }
    let (next, changed) = current.apply(mutation)?;
    if !changed {
        return Ok(End::Rollback(WriteOutcome::AlreadyApplied(next)));
    }
    persist_on(tx, write.fence, &next, CheckpointOwnership::Execution).await?;
    live(tx, write.fence, bound).await?;
    Ok(End::Commit(WriteOutcome::Applied(next)))
}

pub(super) enum CheckpointOwnership {
    Execution,
    Model(Option<Uuid>),
    TokenPause(Uuid),
    Progression,
}

pub(super) async fn persist_on(
    tx: &mut PgConnection,
    fence: WorkflowFence,
    run: &RunCheckpoint,
    ownership: CheckpointOwnership,
) -> AppResult<()> {
    struct Fields {
        mode: &'static str,
        active: Option<Uuid>,
        decision: Option<Uuid>,
    }
    let fields = match ownership {
        CheckpointOwnership::Progression => Fields {
            mode: "progression",
            active: None,
            decision: None,
        },
        CheckpointOwnership::Execution => Fields {
            mode: "execution",
            active: None,
            decision: None,
        },
        CheckpointOwnership::Model(request) => Fields {
            mode: "model",
            active: request,
            decision: None,
        },
        CheckpointOwnership::TokenPause(decision) => Fields {
            mode: "token",
            active: None,
            decision: Some(decision),
        },
    };
    let changed = sqlx::query("UPDATE task_harness_runs SET revision=$5,state=$6,checkpoint=$7,
        workflow_generation=$8,workflow_worker_id=$9,workflow_attempt=$10,updated_at=clock_timestamp(),
        model_request_id=CASE WHEN $11='execution' AND model_generation=$8 THEN model_request_id ELSE $12 END,
        model_worker_id=CASE WHEN $11='execution' AND model_generation=$8 THEN model_worker_id WHEN $12::uuid IS NOT NULL THEN $9 ELSE NULL END,
        model_attempt=CASE WHEN $11='execution' AND model_generation=$8 THEN model_attempt WHEN $12::uuid IS NOT NULL THEN $10 ELSE NULL END,
        model_generation=CASE WHEN $11='execution' AND model_generation=$8 THEN model_generation WHEN $12::uuid IS NOT NULL THEN $8 ELSE NULL END,
        model_blocked_decision=CASE WHEN $11='execution' THEN model_blocked_decision ELSE $13 END
        WHERE owner_kind='workflow' AND company_id=$1 AND workflow_run_id=$2 AND workflow_execution_id=$3 AND id=$4")
        .bind(fence.scope.company.as_uuid()).bind(fence.scope.run.as_uuid()).bind(fence.scope.execution.as_uuid())
        .bind(run.run_id.0).bind(revision(run.revision.0)?).bind(state_name(&run.state))
        .bind(serde_json::to_value(run).map_err(|_| invalid())?).bind(fence.generation.0).bind(fence.worker.0).bind(fence.attempt.0)
        .bind(fields.mode).bind(fields.active).bind(fields.decision).execute(tx).await?.rows_affected();
    if changed != 1 {
        return Err(missing());
    }
    Ok(())
}

async fn execution_evidence(
    tx: &mut Transaction<'_, Postgres>,
    write: WorkflowCheckpointWrite,
    allowance_ms: u64,
    bound: &WriteBound,
) -> AppResult<Evidence<Outcome>> {
    if !lock_owner(tx, write.fence, bound).await? {
        return Err(bound.unresolved());
    }
    let saved = load_on(tx, write.fence)
        .await?
        .ok_or_else(|| bound.unresolved())?;
    let run = decode(saved.checkpoint)?;
    if run.run_id != write.checkpoint {
        return Err(bound.unresolved());
    }
    live(tx, write.fence, bound).await?;
    match run
        .executions
        .iter()
        .find(|execution| execution.generation == write.fence.generation.0)
    {
        Some(execution) if execution.allowance_ms == allowance_ms => {
            Ok(Evidence::Applied(WriteOutcome::AlreadyApplied(run)))
        }
        Some(_) => Err(bound.unresolved()),
        None => Ok(Evidence::NotApplied),
    }
}

#[path = "checkpoint_models.rs"]
mod models;

#[path = "checkpoint_updates.rs"]
mod updates;
