//! Local ownership and the root accounting lock enclose every model-ledger write.
use super::*;
use crate::application::workflow::budget::{
    BudgetDisposition, BudgetReservation, BudgetReservationKey, BudgetReservationResult,
};
use crate::domain::workflow::{BudgetCharge, BudgetResource};
use crate::services::harness::runs::ModelReservation;

type Admission = WorkflowModelAdmission;

pub(super) async fn reserve_on(
    tx: &mut Transaction<'_, Postgres>,
    write: WorkflowCheckpointWrite,
    request: &WorkflowModelRequest,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
    bound: &WriteBound,
) -> AppResult<End<Admission>> {
    if !token_budget::prelock_reviewers(tx, write.fence.scope).await? {
        return Ok(End::Rollback(Admission::OwnershipLost));
    }
    // Revocation linearizes against admission: checked under authority locks.
    authorize_on(tx, write.fence).await?;
    if !lock_owner(tx, write.fence, bound).await? {
        return Ok(End::Rollback(Admission::OwnershipLost));
    }
    let saved = load_on(tx, write.fence).await?.ok_or_else(missing)?;
    let current = decode(saved.checkpoint.clone())?;
    live(tx, write.fence, bound).await?;
    if let Some(result) = admission_replay(&saved, &current, write, &request.reservation)? {
        return Ok(End::Rollback(result));
    }
    if let Some(decision) = saved.model_blocked_decision {
        decisions::tokens::approved_request(tx, write.fence.scope, decision, request).await?;
    }
    let (next, _) = current.apply(Mutation::Reserve(request.reservation.clone()))?;
    let root = token_budget::lock_on(tx, write.fence).await?;
    if root.refusal.is_some() {
        return Ok(End::Rollback(Admission::TokenRefused));
    }
    if root.model_exhausted {
        charge(tx, write, request, bound).await?;
        return Ok(End::Commit(Admission::BudgetExhausted));
    }
    if root.original.is_some() && notices.is_none() {
        return Err(AppError::BadRequest(
            "Token review notifications are unavailable".into(),
        ));
    }
    let additional = request
        .reservation
        .input_tokens
        .checked_add(request.reservation.output_tokens)
        .ok_or_else(invalid)?;
    if root.requires_approval(additional)? {
        let paused = decisions::tokens::park(
            tx,
            decisions::tokens::TokenPause {
                fence: write.fence,
                checkpoint: &current,
                root: &root,
                subject: decisions::tokens::TokenSubject::Request(request),
            },
            notices.ok_or_else(invalid)?,
        )
        .await?;
        return Ok(End::Commit(Admission::Parked(paused.wait)));
    }
    if !charge(tx, write, request, bound).await? {
        return Ok(End::Commit(Admission::BudgetExhausted));
    }
    super::super::super::model_usage::persist_checkpoint_usage_on(tx, &current, &next).await?;
    persist_on(
        tx,
        write.fence,
        &next,
        CheckpointOwnership::Model(Some(request.reservation.request_id.0)),
    )
    .await?;
    live(tx, write.fence, bound).await?;
    Ok(End::Commit(Admission::New(NewModelAdmission::committed(
        next,
    ))))
}

async fn charge(
    tx: &mut PgConnection,
    write: WorkflowCheckpointWrite,
    request: &WorkflowModelRequest,
    bound: &WriteBound,
) -> AppResult<bool> {
    let receipt = BudgetReservation::new(
        write.fence,
        BudgetReservationKey::parse(format!("model.{}", request.reservation.request_id.0))?,
        BudgetCharge::new(BudgetResource::ModelCall, 1).map_err(|_| invalid())?,
    )?;
    match budget::reserve_on(tx, &receipt, bound.server_timeout()).await? {
        BudgetReservationResult::Recorded(BudgetDisposition::Granted) => Ok(true),
        BudgetReservationResult::Recorded(BudgetDisposition::Exhausted) => Ok(false),
        // A receipt without its atomic checkpoint never grants provider entry.
        _ => Err(invalid()),
    }
}

fn admission_replay(
    saved: &Saved,
    current: &RunCheckpoint,
    write: WorkflowCheckpointWrite,
    reservation: &ModelReservation,
) -> AppResult<Option<Admission>> {
    if current.run_id != write.checkpoint {
        return Ok(Some(Admission::OwnershipLost));
    }
    if let Some(previous) = current
        .reservations
        .iter()
        .find(|r| r.request_id == reservation.request_id)
    {
        if previous != reservation {
            return Err(AppError::Conflict("Model request identity changed".into()));
        }
        return Ok(Some(Admission::PreviouslyAdmitted(Box::new(
            current.clone(),
        ))));
    }
    if current.revision != write.expected_revision {
        return Ok(Some(Admission::RevisionConflict));
    }
    if saved.model_request_id.is_some() {
        return Ok(Some(Admission::ActiveRequest));
    }
    if current.state != RunState::Active
        || !current
            .executions
            .iter()
            .any(|r| r.generation == write.fence.generation.0)
    {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::CheckpointDenied,
        ));
    }
    Ok(None)
}

pub(super) async fn settle_on(
    tx: &mut Transaction<'_, Postgres>,
    write: WorkflowCheckpointWrite,
    turn: &SavedModelTurn,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
    bound: &WriteBound,
) -> AppResult<End<WorkflowModelSettlement>> {
    let reviewers_available = token_budget::prelock_reviewers(tx, write.fence.scope).await?;
    if !lock_owner(tx, write.fence, bound).await? {
        return Ok(End::Rollback(WorkflowModelSettlement::saved(
            WriteOutcome::OwnershipLost,
        )));
    }
    let saved = load_on(tx, write.fence).await?.ok_or_else(missing)?;
    let current = decode(saved.checkpoint.clone())?;
    live(tx, write.fence, bound).await?;
    if let Some(result) = settlement_replay(&saved, &current, write, turn)? {
        let result = replay_policy(tx, write.fence, result).await?;
        live(tx, write.fence, bound).await?;
        return Ok(End::Rollback(result));
    }
    let (next, _) = current.apply(Mutation::Model(turn.clone()))?;
    token_budget::lock_on(tx, write.fence).await?;
    super::super::super::model_usage::persist_checkpoint_usage_on(tx, &current, &next).await?;
    persist_on(tx, write.fence, &next, CheckpointOwnership::Model(None)).await?;
    let root = token_budget::lock_on(tx, write.fence).await?;
    live(tx, write.fence, bound).await?;
    if root.refusal.is_some() {
        return Ok(End::Commit(WorkflowModelSettlement::Refused(Box::new(
            next,
        ))));
    }
    if next.state == RunState::Active && root.requires_approval(0)? {
        let Some(notices) = notices.filter(|_| reviewers_available) else {
            return Ok(End::Commit(WorkflowModelSettlement::ReviewUnavailable(
                Box::new(next),
            )));
        };
        let paused = decisions::tokens::park(
            tx,
            decisions::tokens::TokenPause {
                fence: write.fence,
                checkpoint: &next,
                root: &root,
                subject: decisions::tokens::TokenSubject::Settlement(turn.request_id),
            },
            notices,
        )
        .await?;
        return Ok(End::Commit(WorkflowModelSettlement::Parked {
            checkpoint: Box::new(paused.checkpoint),
            wait: paused.wait,
        }));
    }
    Ok(End::Commit(WorkflowModelSettlement::saved(
        WriteOutcome::Applied(next),
    )))
}

async fn replay_policy(
    db: &mut PgConnection,
    fence: crate::application::workflow::lease::WorkflowFence,
    outcome: Outcome,
) -> AppResult<WorkflowModelSettlement> {
    let WriteOutcome::AlreadyApplied(checkpoint) = outcome else {
        return Ok(WorkflowModelSettlement::saved(outcome));
    };
    let root = token_budget::lock_on(db, fence).await?;
    if root.refusal.is_some() {
        return Ok(WorkflowModelSettlement::Refused(Box::new(checkpoint)));
    }
    if checkpoint.state == RunState::Active && root.requires_approval(0)? {
        // A previous settlement may have retained truth without a usable reviewer
        // or notification composer. Replaying it cannot grant progression.
        return Ok(WorkflowModelSettlement::ReviewUnavailable(Box::new(
            checkpoint,
        )));
    }
    Ok(WorkflowModelSettlement::saved(
        WriteOutcome::AlreadyApplied(checkpoint),
    ))
}

fn settlement_replay(
    saved: &Saved,
    current: &RunCheckpoint,
    write: WorkflowCheckpointWrite,
    turn: &SavedModelTurn,
) -> AppResult<Option<Outcome>> {
    if current.run_id != write.checkpoint {
        return Ok(Some(WriteOutcome::OwnershipLost));
    }
    if let Some(previous) = current
        .turns
        .iter()
        .find(|t| t.request_id == turn.request_id)
    {
        if previous != turn {
            return Err(AppError::Conflict("Saved model turn changed".into()));
        }
        return Ok(Some(WriteOutcome::AlreadyApplied(current.clone())));
    }
    if current.revision != write.expected_revision {
        return Ok(Some(WriteOutcome::RevisionConflict));
    }
    if saved.model_request_id != Some(turn.request_id.0)
        || saved.model_generation != Some(write.fence.generation.0)
        || saved.model_worker_id != Some(write.fence.worker.0)
        || saved.model_attempt != Some(write.fence.attempt.0)
    {
        return Ok(Some(WriteOutcome::OwnershipLost));
    }
    Ok(None)
}
