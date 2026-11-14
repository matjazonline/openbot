//! Local ownership and the root accounting lock enclose every model-ledger write.
use super::*;
use crate::application::workflow::budget::{
    BudgetDisposition, BudgetReservation, BudgetReservationKey, BudgetReservationResult,
};
use crate::domain::workflow::{BudgetCharge, BudgetResource};
use crate::services::harness::runs::ModelReservation;

type Admission = WorkflowModelAdmission;

#[path = "checkpoint_model_accounts.rs"]
mod accounts;

const MAX_HINT_ATTEMPTS: usize = 2;

pub(super) async fn reserve(
    conn: &mut sqlx::pool::PoolConnection<Postgres>,
    write: WorkflowCheckpointWrite,
    request: &WorkflowModelRequest,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
    bound: &WriteBound,
) -> AppResult<Admission> {
    for _ in 0..MAX_HINT_ATTEMPTS {
        let mut tx = bound.begin(conn).await?;
        match accounts::prepare_on(&mut tx, write, accounts::Purpose::Reserve(request), notices)
            .await?
        {
            accounts::Preparation::Stale => {
                bound.roll_back(tx, Ok(())).await?;
            }
            accounts::Preparation::Current(authority) => {
                let result = reserve_on(&mut tx, write, request, notices, bound, authority).await;
                return bound.end(tx, result).await;
            }
        }
    }
    Err(accounts::changed())
}

pub(super) async fn settle(
    conn: &mut sqlx::pool::PoolConnection<Postgres>,
    write: WorkflowCheckpointWrite,
    turn: &SavedModelTurn,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
    bound: &WriteBound,
) -> AppResult<WorkflowModelSettlement> {
    for _ in 0..MAX_HINT_ATTEMPTS {
        let mut tx = bound.begin(conn).await?;
        match accounts::prepare_on(&mut tx, write, accounts::Purpose::Settle(turn), notices).await?
        {
            accounts::Preparation::Stale => {
                bound.roll_back(tx, Ok(())).await?;
            }
            accounts::Preparation::Current(authority) => {
                let result = settle_on(&mut tx, write, turn, notices, bound, authority).await;
                return bound.end(tx, result).await;
            }
        }
    }
    Err(accounts::changed())
}

async fn reserve_on(
    tx: &mut Transaction<'_, Postgres>,
    write: WorkflowCheckpointWrite,
    request: &WorkflowModelRequest,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
    bound: &WriteBound,
    authority: accounts::ModelAuthority,
) -> AppResult<End<Admission>> {
    // Revocation linearizes against admission: checked under authority locks.
    if authorize_prelocked_on(tx, write.fence, authority.actor)
        .await?
        .is_none()
    {
        return Ok(End::Rollback(Admission::OwnershipLost));
    }
    if !lock_prelocked_owner(tx, write.fence, bound, authority.actor).await? {
        return Ok(End::Rollback(Admission::OwnershipLost));
    }
    agent_history::audience_on(
        tx,
        ActionScope {
            company: write.fence.scope.company,
            run: write.fence.scope.run,
            execution: write.fence.scope.execution,
        },
    )
    .await?;
    let saved = load_on(tx, write.fence).await?.ok_or_else(missing)?;
    let current = decode(saved.id, saved.checkpoint.clone())?;
    live(tx, write.fence, bound).await?;
    if let Some(result) = admission_replay(&saved, &current, write, &request.reservation)? {
        return Ok(End::Rollback(result));
    }
    agent_history::require_active_on(tx, write.fence, &saved.workflow_prepared, current.run_id.0)
        .await?;
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
    let additional = review_request_tokens(&root, request, notices)?;
    if root.requires_approval(additional)? {
        if !authority.reviewers_available {
            return Ok(End::Rollback(Admission::OwnershipLost));
        }
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

fn review_request_tokens(
    root: &token_budget::RootTokens,
    request: &WorkflowModelRequest,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
) -> AppResult<u64> {
    if root.original.is_some() && notices.is_none() {
        return Err(AppError::BadRequest(
            "Token review notifications are unavailable".into(),
        ));
    }
    request
        .reservation
        .input_tokens
        .checked_add(request.reservation.output_tokens)
        .ok_or_else(invalid)
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

async fn settle_on(
    tx: &mut Transaction<'_, Postgres>,
    write: WorkflowCheckpointWrite,
    turn: &SavedModelTurn,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
    bound: &WriteBound,
    authority: accounts::ModelAuthority,
) -> AppResult<End<WorkflowModelSettlement>> {
    if !lock_prelocked_owner(tx, write.fence, bound, authority.actor).await? {
        return Ok(End::Rollback(WorkflowModelSettlement::saved(
            WriteOutcome::OwnershipLost,
        )));
    }
    let saved = load_on(tx, write.fence).await?.ok_or_else(missing)?;
    let current = decode(saved.id, saved.checkpoint.clone())?;
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
    agent_history::accepted_on(tx, write.fence, &saved.workflow_prepared, &next).await?;
    let root = token_budget::lock_on(tx, write.fence).await?;
    live(tx, write.fence, bound).await?;
    if root.refusal.is_some() {
        return Ok(End::Commit(WorkflowModelSettlement::Refused(Box::new(
            next,
        ))));
    }
    if next.state == RunState::Active && root.requires_approval(0)? {
        let Some(notices) = notices.filter(|_| authority.reviewers_available) else {
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
