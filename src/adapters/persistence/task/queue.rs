//! The task lifecycle: claiming, attempt bookkeeping, failure, stop and resume, and the enqueue
//! that lands a task with its channel fan-out.
//!
//! The row-local transition attribution lives here too, beside the status-changing statements that
//! are required to write it.

use std::collections::HashSet;

use sqlx::Postgres;
use uuid::Uuid;

use super::*;
use crate::{
    adapters::persistence::channel_gate::{
        ChannelGateAccess, acquire_channel_gate_on, task_gate_key_on,
    },
    adapters::persistence::thread_handoff::{HandoffRunEnd, fail_handoff_run_on},
    app_error::{AppError, AppResult},
    entities::{
        message::CanonicalMessageId,
        task::{
            BackgroundTask, NewTask, ResumeActor, StopActor, TaskFailure, TaskFailureOutcome,
            TaskSource, TaskStopReason, TaskTarget, TaskTransitionReason, TransitionActor,
        },
        transport::PrincipalId,
    },
};

/// Taking a task's lease. Only a task that is still pending and already due can be claimed, so two
/// callers racing for the same row leave exactly one of them holding it.
pub(crate) const CLAIM_TASK_SQL: &str = r#"UPDATE background_tasks
   SET status = 'processing', worker_id = $2, execution_generation = gen_random_uuid(),
       locked_at = CURRENT_TIMESTAMP,
       lock_expires_at = $3, updated_at = CURRENT_TIMESTAMP,
       transition_reason = 'claimed', transition_actor_kind = 'worker',
       transition_actor_id = $2, transition_approval_id = NULL, transition_outreach_id = NULL
   WHERE id = $1 AND status = 'pending' AND run_at <= CURRENT_TIMESTAMP
     AND owner_principal_kind = 'agent'
     AND EXISTS (
         SELECT 1 FROM principals AS owner
         JOIN channel_agents AS assignment
           ON assignment.company_id = background_tasks.company_id
          AND assignment.channel_id = background_tasks.channel_id
          AND assignment.agent_id = owner.agent_id
         WHERE owner.company_id = background_tasks.company_id
           AND owner.id = background_tasks.owner_principal_id
           AND owner.kind = 'agent'
     )"#;
/// Open the ledger row for one attempt.
///
/// The conflict is not an error and not a duplicate: a task whose lease lapsed is re-claimed with
/// its `retry_count` untouched — `mark_task_failed` never ran — so the new run carries the same
/// attempt number as the run that vanished. That earlier run reported nothing, so its half-written
/// row is reset here rather than left to be read as a finished attempt that took forever.
pub(crate) const BEGIN_ATTEMPT_SQL: &str = r#"INSERT INTO task_attempts
       (id, task_id, attempt_number, execution_generation, status, started_at,
        worker_id, machine_id, machine_region)
   VALUES ($1, $2, $3, $4, 'processing', CURRENT_TIMESTAMP, $5, $6, $7)
   ON CONFLICT (task_id, attempt_number) DO UPDATE
      SET status = 'processing', started_at = CURRENT_TIMESTAMP, finished_at = NULL,
          error = NULL, stop_reason = NULL, prompt_tokens = NULL, completion_tokens = NULL,
          execution_generation = EXCLUDED.execution_generation,
          worker_id = EXCLUDED.worker_id, machine_id = EXCLUDED.machine_id,
          machine_region = EXCLUDED.machine_region"#;

/// Close the ledger row, but only while it is still the open one. If another worker took the task
/// over and reopened the row, this run is no longer the run of record and must not overwrite it.
pub(crate) const FINISH_ATTEMPT_SQL: &str = r#"UPDATE task_attempts
   SET status = $4, error = $5, prompt_tokens = $6, completion_tokens = $7,
       stop_reason = $8,
       finished_at = CURRENT_TIMESTAMP
   WHERE task_id = $1 AND attempt_number = $2 AND execution_generation = $3
     AND status = 'processing'"#;

/// What a reaped run is recorded as having failed with. The reaper writes it to both the task row
/// and the attempt ledger, which must agree on why the run vanished.
pub(crate) const LEASE_EXPIRED_ERROR: &str =
    "Task lease expired without the run reporting a result";
/// What a status change says about itself: why it happened and who caused it.
///
/// Written into the task row by the same statement that changes the status, which is what the
/// ledger trigger reads. Constructed only from the typed cause enums, so the shape the database
/// CHECK constraint enforces -- an actor kind together with exactly the id that kind requires --
/// is not expressible any other way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransitionAttribution {
    pub(crate) reason: TaskTransitionReason,
    pub(crate) actor: TransitionActor,
}

impl TransitionAttribution {
    pub(crate) fn new(reason: TaskTransitionReason, actor: TransitionActor) -> Self {
        Self { reason, actor }
    }

    pub(crate) fn stopped(actor: StopActor) -> Self {
        Self::new(actor.reason(), actor.transition_actor())
    }

    pub(crate) fn resumed(actor: ResumeActor) -> Self {
        Self::new(actor.reason(), actor.transition_actor())
    }

    /// The `SET` fragment naming all five columns.
    ///
    /// Rendered rather than bound: every part is a `&'static str` from an enum or a `Uuid`, whose
    /// `Display` can only emit hex and dashes, so there is nothing here a placeholder would
    /// protect. Rendering keeps the fragment self-contained -- a statement splices it in without
    /// renumbering its own parameters, which is what makes writing it everywhere cheap enough to
    /// be unconditional.
    pub(crate) fn set_clause(self) -> String {
        format!(
            "transition_reason = '{reason}', transition_actor_kind = '{kind}', \
             transition_actor_id = {actor_id}, transition_approval_id = {approval_id}, \
             transition_outreach_id = {outreach_id}",
            reason = self.reason.as_str(),
            kind = self.actor.kind().as_str(),
            actor_id = sql_uuid_literal(self.actor.actor_id()),
            approval_id = sql_uuid_literal(self.actor.approval_id()),
            outreach_id = sql_uuid_literal(self.actor.outreach_id()),
        )
    }
}

pub(crate) fn sql_uuid_literal(id: Option<Uuid>) -> String {
    id.map_or_else(|| "NULL".to_owned(), |id| format!("'{id}'::uuid"))
}
pub(crate) async fn mark_task_failed_on(
    pool: &sqlx::PgPool,
    failure: TaskFailure<'_>,
) -> AppResult<bool> {
    let reason = match failure.reason {
        TaskStopReason::RetryableFailure => TaskTransitionReason::RetryableFailure,
        TaskStopReason::TerminalFailure => TaskTransitionReason::TerminalFailure,
        TaskStopReason::TimedOut => TaskTransitionReason::TimedOut,
        TaskStopReason::Shutdown => TaskTransitionReason::Shutdown,
        TaskStopReason::LeaseLost => TaskTransitionReason::LeaseLost,
        TaskStopReason::Completed => TaskTransitionReason::Completed,
        TaskStopReason::OwnershipTransferred => TaskTransitionReason::OwnershipTransferred,
        TaskStopReason::AgentInstruction => TaskTransitionReason::AgentInstruction,
        TaskStopReason::DelegationCancelled => TaskTransitionReason::DelegationCancelled,
        TaskStopReason::OperatorStopped => TaskTransitionReason::OperatorStopped,
        TaskStopReason::ApprovalRejected => TaskTransitionReason::ApprovalRejected,
        TaskStopReason::ChannelAgentRemoved => TaskTransitionReason::ChannelAgentRemoved,
    };
    // The lease names the run that failed, so the failure cannot be attributed to anyone else.
    let attribution =
        TransitionAttribution::new(reason, TransitionActor::Worker(failure.lease.worker_id));
    // A transaction rather than a bare statement because a terminal failure also hands any
    // drafting run's reply back to the team, and the two must not be able to disagree.
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    let company_id = sqlx::query_scalar::<_, Uuid>(&format!(
        r#"UPDATE background_tasks
           SET status = $1, retry_count = retry_count + 1, last_error = $2,
               run_at = $3, worker_id = NULL, execution_generation = NULL, locked_at = NULL,
               lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP, {attribution}
           WHERE id = $4 AND status = 'processing' AND worker_id = $5
             AND execution_generation = $6
             AND owner_principal_id = $7 AND ownership_version = $8
             AND lock_expires_at > CURRENT_TIMESTAMP
           RETURNING company_id"#,
        attribution = attribution.set_clause(),
    ))
    .bind(failure.outcome.status().as_str())
    .bind(failure.error)
    .bind(failure.next_run_at)
    .bind(failure.lease.task_id)
    .bind(failure.lease.worker_id)
    .bind(failure.lease.execution_generation)
    .bind(
        failure
            .lease
            .claimed_owner
            .agent_principal_id()
            .map(PrincipalId::as_uuid),
    )
    .bind(
        i64::try_from(failure.lease.ownership_version)
            .map_err(|_| AppError::Conflict("Ownership version exhausted.".into()))?,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?;
    let Some(company_id) = company_id else {
        tx.commit().await.map_err(AppError::from)?;
        return Ok(false);
    };
    // A retry leaves the handoff `drafting`: the run has not ended, it is going to be attempted
    // again. Only a task that has run out of road gives the reply back to the team.
    if failure.outcome == TaskFailureOutcome::DeadLetter {
        fail_handoff_run_on(
            &mut tx,
            company_id,
            failure.lease.task_id,
            HandoffRunEnd::TaskFailed,
            failure.error,
        )
        .await?;
    }
    tx.commit().await.map_err(AppError::from)?;
    Ok(true)
}

/// Which states each stop cause may act on.
///
/// An operator may stop anything still in flight or recoverable, and so may removing the owning
/// agent from the task's channel. A rejected approval may only stop the task that approval parked
/// -- a rejection is an answer to one question, not a licence to end unrelated work that has since
/// moved on.
pub(crate) fn stoppable_statuses(actor: StopActor) -> &'static str {
    match actor {
        StopActor::Operator(_) | StopActor::ChannelAgentRemoved(_) => {
            "'pending', 'processing', 'pending_approval', \
             'waiting_for_third_party_reply', 'failed', 'dead_letter'"
        }
        StopActor::Approval(_) => "'pending_approval'",
    }
}

pub(crate) async fn stop_task_on(
    pool: &sqlx::PgPool,
    id: Uuid,
    actor: StopActor,
) -> AppResult<BackgroundTask> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    let company_id: Uuid =
        sqlx::query_scalar("SELECT company_id FROM background_tasks WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::from)?;
    lock_task_outreaches_on(&mut tx, &[id]).await?;
    if stop_tasks_on(&mut tx, company_id, &[id], actor)
        .await?
        .is_empty()
    {
        // The same answer the single-row `UPDATE ... RETURNING` this replaced gave a task that
        // was not stoppable by this actor, so callers keep telling the two apart as before.
        return Err(AppError::from(sqlx::Error::RowNotFound));
    }
    withdraw_pending_work_on(&mut tx, company_id, &[id], actor.delivery_cancellation()).await?;
    let db = sqlx::query_as::<_, BackgroundTaskDb>(
        r#"SELECT id, company_id, channel_id, thread_id, correlation_id, task_type, status,
                  payload, retry_count, max_retries, last_error, business_priority,
                  business_due_at, attention_version, owner_principal_id,
                  owner_principal_kind, ownership_version, worker_id,
                  execution_generation, locked_at, lock_expires_at, run_at, created_at,
                  updated_at
             FROM background_tasks WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::from)?;
    tx.commit().await.map_err(AppError::from)?;
    db.try_into()
}

/// Lock these tasks' outreaches, in id order.
///
/// Delegation commands and final dispatches lock outreach before task. Every stop takes the same
/// order, so an operator action or a channel edit cannot deadlock a command racing on the same task.
pub(crate) async fn lock_task_outreaches_on(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    task_ids: &[Uuid],
) -> AppResult<()> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM task_outreaches WHERE task_id = ANY($1) ORDER BY id FOR UPDATE",
    )
    .bind(task_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// A task a stop has just moved to `stopped`, with what it was doing beforehand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub(crate) struct StoppedTask {
    pub(crate) id: Uuid,
    pub(crate) ownership_version: i64,
    /// The execution the stop interrupted, when the task was running. Exactly that attempt is
    /// closed; older generations are history and stay as they are.
    pub(crate) interrupted_generation: Option<Uuid>,
}

/// Stop every one of these tasks that `actor` may stop, and end the work each was doing.
///
/// The caller holds the outreach locks ([`lock_task_outreaches_on`]); the task rows are locked here
/// in id order. A task already outside the actor's stoppable states is left exactly as it is, so a
/// repeated stop writes no second ledger event. Queued publications are a separate step,
/// [`withdraw_pending_work_on`], because a completed task has those and nothing to stop.
pub(crate) async fn stop_tasks_on(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    company_id: Uuid,
    task_ids: &[Uuid],
    actor: StopActor,
) -> AppResult<Vec<StoppedTask>> {
    if task_ids.is_empty() {
        return Ok(Vec::new());
    }
    // The pre-stop generation comes from the locking read, because `RETURNING` sees the row after
    // the lease columns were cleared.
    let stopped = sqlx::query_as::<_, StoppedTask>(&format!(
        r#"WITH interrupted AS (
               SELECT id, execution_generation
                 FROM background_tasks
                WHERE company_id = $1 AND id = ANY($2) AND status IN ({statuses})
                ORDER BY id
                  FOR UPDATE
           )
           UPDATE background_tasks AS task
              SET status = 'stopped', worker_id = NULL, execution_generation = NULL,
                  locked_at = NULL, lock_expires_at = NULL, wait_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP, {attribution}
             FROM interrupted
            WHERE task.id = interrupted.id
        RETURNING task.id, task.ownership_version,
                  interrupted.execution_generation AS interrupted_generation"#,
        attribution = TransitionAttribution::stopped(actor).set_clause(),
        statuses = stoppable_statuses(actor),
    ))
    .bind(company_id)
    .bind(task_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if stopped.is_empty() {
        return Ok(stopped);
    }
    let ids: Vec<Uuid> = stopped.iter().map(|task| task.id).collect();

    close_interrupted_attempts_on(tx, &stopped, actor).await?;
    sqlx::query(
        r#"UPDATE task_outreaches
           SET status = 'cancelled', version = version + 1, updated_at = CURRENT_TIMESTAMP
           WHERE task_id = ANY($1) AND status IN (
               'waiting', 'threshold_met', 'timeout_pending_approval', 'proceed_partial'
           )"#,
    )
    .bind(&ids)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE task_outreach_targets AS target SET status = 'cancelled'
           FROM task_outreaches AS outreach
           WHERE outreach.task_id = ANY($1) AND target.outreach_id = outreach.id
             AND target.status = 'active'"#,
    )
    .bind(&ids)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let versions: Vec<(Uuid, i64)> = stopped
        .iter()
        .map(|task| (task.id, task.ownership_version))
        .collect();
    super::supersede_harness_runs_for_tasks_on(tx, company_id, &versions).await?;
    sqlx::query(
        r#"UPDATE task_approval_waits SET state = 'expired'
            WHERE company_id = $1 AND task_id = ANY($2) AND state = 'waiting'"#,
    )
    .bind(company_id)
    .bind(&ids)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    // A stopped task is terminal for anything waiting on it, so a drafting run stops too and its
    // reply goes back to the team rather than waiting for an agent that will never run again.
    for task_id in handoff_runs_in_state_on(tx, company_id, &ids, "running").await? {
        fail_handoff_run_on(
            tx,
            company_id,
            task_id,
            HandoffRunEnd::TaskFailed,
            "the task drafting this reply was stopped",
        )
        .await?;
    }
    Ok(stopped)
}

/// Close exactly the attempt each stop interrupted, with the stop's typed reason.
///
/// Fenced on the captured generation and on `processing`: a worker that already recorded its own
/// ending keeps it, and when the interrupted run later tries to finish, its write matches nothing.
async fn close_interrupted_attempts_on(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    stopped: &[StoppedTask],
    actor: StopActor,
) -> AppResult<()> {
    let (task_ids, generations): (Vec<Uuid>, Vec<Uuid>) = stopped
        .iter()
        .filter_map(|task| {
            task.interrupted_generation
                .map(|generation| (task.id, generation))
        })
        .unzip();
    if task_ids.is_empty() {
        return Ok(());
    }
    let error = match actor {
        StopActor::Operator(_) => "An operator stopped the task during this attempt",
        StopActor::Approval(_) => "A rejected approval stopped the task during this attempt",
        StopActor::ChannelAgentRemoved(_) => {
            "The owning agent was removed from the task's channel during this attempt"
        }
    };
    sqlx::query(
        r#"UPDATE task_attempts AS attempt
              SET status = 'failed', stop_reason = $3, error = $4,
                  finished_at = CURRENT_TIMESTAMP
             FROM unnest($1::uuid[], $2::uuid[]) AS interrupted(task_id, execution_generation)
            WHERE attempt.task_id = interrupted.task_id
              AND attempt.execution_generation = interrupted.execution_generation
              AND attempt.status = 'processing'"#,
    )
    .bind(&task_ids)
    .bind(&generations)
    .bind(actor.attempt_stop_reason().as_str())
    .bind(error)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// What withdrawing a set of tasks' pending work took back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WithdrawnWork {
    pub(crate) deliveries:
        crate::adapters::persistence::delivery::cancellation::CancelledDeliveries,
    pub(crate) superseded_reviews: u64,
}

/// Withdraw everything these tasks could still publish or be released by: queued and in-flight
/// deliveries, pending review drafts, and pending human approvals.
///
/// Applies to stopped and completed tasks alike. A final dispatch can commit a reply to the outbox
/// the instant before its task is stopped, and a review draft outlives the run that wrote it -- so
/// "the task is no longer running" is not the same fact as "nothing of it can still go out".
pub(crate) async fn withdraw_pending_work_on(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    company_id: Uuid,
    task_ids: &[Uuid],
    reason: crate::entities::transport::DeliveryCancellationReason,
) -> AppResult<WithdrawnWork> {
    if task_ids.is_empty() {
        return Ok(WithdrawnWork::default());
    }
    let deliveries =
        crate::adapters::persistence::delivery::cancellation::cancel_task_deliveries_on(
            tx, company_id, task_ids, reason,
        )
        .await?;

    // A superseded draft can no longer be approved: the review command requires `pending_review`
    // under the draft's row lock, so an approval racing this either commits first -- and its
    // delivery is cancelled above, being task-linked -- or finds the draft gone.
    let (superseded_reviews, drafting_tasks): (i64, Vec<Uuid>) = sqlx::query_as(
        r#"WITH superseded AS (
               UPDATE response_drafts AS draft
                  SET status = 'superseded', updated_at = CURRENT_TIMESTAMP
                WHERE draft.company_id = $1 AND draft.status = 'pending_review'
                  AND draft.task_id = ANY($2)
            RETURNING draft.company_id, draft.id, draft.version, draft.task_id
           ),
           reviews AS (
               UPDATE response_reviews AS review
                  SET status = 'superseded', updated_at = CURRENT_TIMESTAMP
                 FROM superseded
                WHERE (review.company_id, review.draft_id, review.draft_version)
                      = (superseded.company_id, superseded.id, superseded.version)
                  AND review.status = 'pending'
            RETURNING review.draft_id
           )
           SELECT (SELECT count(*) FROM reviews),
                  ARRAY(SELECT DISTINCT task_id FROM superseded WHERE task_id IS NOT NULL)"#,
    )
    .bind(company_id)
    .bind(task_ids)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;
    for task_id in drafting_tasks {
        fail_handoff_run_on(
            tx,
            company_id,
            task_id,
            HandoffRunEnd::DraftWithdrawn,
            "the work that drafted this reply was stopped",
        )
        .await?;
    }

    sqlx::query(
        r#"UPDATE human_approvals SET status = 'expired', updated_at = CURRENT_TIMESTAMP
            WHERE company_id = $1 AND task_id = ANY($2) AND status = 'pending'"#,
    )
    .bind(company_id)
    .bind(task_ids)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;

    Ok(WithdrawnWork {
        deliveries,
        superseded_reviews: u64::try_from(superseded_reviews).unwrap_or_default(),
    })
}

/// The tasks among these that have a drafting run in `state`, so a per-run ending is written only
/// where one exists rather than attempted once per task.
async fn handoff_runs_in_state_on(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    company_id: Uuid,
    task_ids: &[Uuid],
    state: &str,
) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        r#"SELECT DISTINCT task_id FROM thread_handoff_runs
            WHERE company_id = $1 AND task_id = ANY($2) AND state = $3
            ORDER BY task_id"#,
    )
    .bind(company_id)
    .bind(task_ids)
    .bind(state)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)
}

/// Which states each resume cause may act on.
///
/// The two causes reach the same status from opposite directions and must not share a predicate.
/// An operator picks up work that has stopped or run out of road. An approval releases the one
/// task it parked -- letting it act on `failed` or `dead_letter` would have a stale link resurrect
/// abandoned work, and letting an operator act on `pending_approval` or
/// `waiting_for_third_party_reply` would walk a task straight through the gate it is parked on.
/// Either mismatch matches no row, which is the existing not-found error and no ledger event.
pub(crate) fn resumable_statuses(actor: ResumeActor) -> &'static str {
    match actor {
        ResumeActor::Operator(_) => "'stopped', 'failed', 'dead_letter'",
        ResumeActor::Approval(_) => "'pending_approval'",
    }
}

/// The `retry_count` assignment a resume contributes, written to follow the rest of the `SET`
/// list -- hence the leading comma, and the empty string when there is nothing to assign.
///
/// An operator pressing Resume on a dead-lettered task means "try this again", and a task whose
/// budget is already spent re-dead-letters on the first failure of the very run it was resumed
/// for. Without this the button moves the row to `pending` and changes nothing that outlives one
/// attempt. The `stopped` arm covers the same task after an operator stopped it first, which is
/// the shape the Tasks page actually offers.
///
/// A continuation is not a retry: an approval releasing a parked task is the same attempt
/// carrying on, so it leaves the budget where the attempts left it.
///
/// Postgres evaluates the right-hand side against the pre-update row, so one statement decides
/// this from the status it is replacing without first reading the task.
pub(crate) fn retry_budget_clause(actor: ResumeActor) -> &'static str {
    match actor {
        ResumeActor::Operator(_) => {
            ", retry_count = CASE \
                 WHEN status IN ('failed', 'dead_letter') THEN 0 \
                 WHEN status = 'stopped' AND retry_count >= max_retries THEN 0 \
                 ELSE retry_count END"
        }
        ResumeActor::Approval(_) => "",
    }
}

/// Whether a task's owner may still run it: a person, nobody, or an agent still assigned to the
/// task's primary channel. The same assignment join the claim requires, so a resume can never make
/// runnable a task no worker would be allowed to take.
const OWNER_STILL_ASSIGNED_SQL: &str = r#"(
    background_tasks.owner_principal_kind IS DISTINCT FROM 'agent'
    OR EXISTS (
        SELECT 1 FROM principals AS owner
        JOIN channel_agents AS assignment
          ON assignment.company_id = owner.company_id
         AND assignment.agent_id = owner.agent_id
        WHERE owner.company_id = background_tasks.company_id
          AND owner.id = background_tasks.owner_principal_id
          AND owner.kind = 'agent'
          AND assignment.channel_id = background_tasks.channel_id
    )
)"#;

pub(crate) async fn resume_task_on(
    pool: &sqlx::PgPool,
    id: Uuid,
    actor: ResumeActor,
) -> AppResult<BackgroundTask> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    // A resume re-admits an agent's work through its channel assignment. The gate is held before
    // the `UPDATE` starts, so the assignment that statement reads is the one a concurrent removal
    // left behind rather than the one it replaced.
    let Some(key) = task_gate_key_on(&mut tx, id).await? else {
        return Err(AppError::from(sqlx::Error::RowNotFound));
    };
    acquire_channel_gate_on(&mut tx, key, ChannelGateAccess::Admit).await?;
    let db = sqlx::query_as::<_, BackgroundTaskDb>(&format!(
        r#"UPDATE background_tasks
           SET status = 'pending', run_at = CURRENT_TIMESTAMP, worker_id = NULL,
               execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL,
               updated_at = CURRENT_TIMESTAMP, {attribution}{retry_budget}
           WHERE id = $1
             AND status IN ({statuses})
             AND {OWNER_STILL_ASSIGNED_SQL}
           RETURNING id, company_id, channel_id, thread_id, correlation_id, task_type, status,
                     payload, retry_count, max_retries, last_error, business_priority,
                     business_due_at, attention_version, owner_principal_id,
                     owner_principal_kind, ownership_version, worker_id,
                     execution_generation, locked_at, lock_expires_at, run_at, created_at,
                     updated_at"#,
        attribution = TransitionAttribution::resumed(actor).set_clause(),
        retry_budget = retry_budget_clause(actor),
        statuses = resumable_statuses(actor),
    ))
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?;
    let Some(db) = db else {
        // Tell "its agent was removed from the channel" apart from "not resumable by this actor",
        // which keeps the not-found answer it always had.
        let unassigned: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS (SELECT 1 FROM background_tasks WHERE id = $1 \
             AND status IN ({statuses}) AND NOT {OWNER_STILL_ASSIGNED_SQL})",
            statuses = resumable_statuses(actor),
        ))
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
        return Err(if unassigned {
            AppError::Conflict(
                "The agent that owns this task is no longer assigned to its channel. \
                 Transfer the task to an assigned agent or a teammate before resuming it."
                    .into(),
            )
        } else {
            AppError::from(sqlx::Error::RowNotFound)
        });
    };
    tx.commit().await.map_err(AppError::from)?;
    db.try_into()
}
/// The SQL half of enqueueing: the task row and its channel-target fan-out, which have to commit
/// together. The `ON CONFLICT` clause makes a redelivered source message return the task it already
/// has rather than starting a second run of it.
pub(crate) async fn insert_task(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    new_task: NewTask,
) -> AppResult<BackgroundTask> {
    let NewTask {
        company_id,
        channel_id,
        thread_id,
        task_type,
        payload,
        targets,
        source,
        correlation_id,
    } = new_task;
    let id = Uuid::new_v4();
    let targets = stated_or_own_channel(targets, channel_id, thread_id);
    // Two `ON CONFLICT` targets cannot be named in one statement, and only one of the two source
    // columns is ever set, so the conflict target follows the source the caller stated. An
    // unattributed task has nothing to collide on and always inserts.
    //
    // Neither branch touches `correlation_id`: a redelivered cause joins the chain its first
    // delivery started rather than overwriting it with a fresher one.
    const RETURNING: &str = "RETURNING id, company_id, channel_id, thread_id, correlation_id, \
         task_type, status, payload, retry_count, max_retries, last_error, business_priority, \
         business_due_at, attention_version, owner_principal_id, \
         owner_principal_kind, ownership_version, worker_id, \
         execution_generation, locked_at, lock_expires_at, run_at, created_at, updated_at";
    let conflict = match source {
        TaskSource::Message(_) => {
            "ON CONFLICT (company_id, source_message_uuid, channel_id)              DO UPDATE SET source_message_uuid = EXCLUDED.source_message_uuid"
        }
        TaskSource::ScheduleRun(_) => {
            "ON CONFLICT (company_id, source_schedule_run_id)              DO UPDATE SET source_schedule_run_id = EXCLUDED.source_schedule_run_id"
        }
        TaskSource::Unattributed => "",
    };
    let db = sqlx::query_as::<_, BackgroundTaskDb>(&format!(
        r#"INSERT INTO background_tasks (
                id, company_id, channel_id, thread_id, source_message_uuid,
                source_schedule_run_id, correlation_id, task_type, status, payload
           )
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending', $9)
           {conflict}
           {RETURNING}"#
    ))
    .bind(id)
    .bind(company_id)
    .bind(channel_id)
    .bind(thread_id)
    .bind(source.message_id().map(CanonicalMessageId::as_uuid))
    .bind(source.schedule_run_id())
    .bind(correlation_id.as_uuid())
    .bind(&task_type)
    .bind(payload)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;

    insert_channel_targets(tx, db.id, company_id, targets).await?;
    db.try_into()
}

/// The run's channel fan-out, as one statement however many channels the producer stated.
///
/// A channel stated twice keeps its first position and role, and every later channel keeps the
/// position it was stated at -- what the row-at-a-time insert this replaced wrote, since its
/// `ON CONFLICT` skipped the repeat without renumbering. The repeat is dropped before binding
/// rather than left to that clause: within one statement, SQL promises nothing about which of two
/// conflicting rows is inserted first.
///
/// The clause stays for the redelivery `insert_task` absorbs: a source message delivered again
/// returns the task it already has, and that task's targets already exist.
async fn insert_channel_targets(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    task_id: Uuid,
    company_id: Uuid,
    targets: Vec<TaskTarget>,
) -> AppResult<()> {
    let mut stated = HashSet::with_capacity(targets.len());
    let targets: Vec<(i32, TaskTarget)> = targets
        .into_iter()
        .enumerate()
        .filter(|(_, target)| stated.insert(target.channel_id))
        .map(|(position, target)| (position as i32, target))
        .collect();
    if targets.is_empty() {
        return Ok(());
    }
    // Projections of one list, so the arrays `unnest` zips together cannot differ in length -- a
    // short array would be padded with NULLs rather than rejected.
    let channel_ids: Vec<Uuid> = targets
        .iter()
        .map(|(_, target)| target.channel_id)
        .collect();
    let thread_ids: Vec<Uuid> = targets.iter().map(|(_, target)| target.thread_id).collect();
    let roles: Vec<&str> = targets
        .iter()
        .map(|(_, target)| target.recipient_role.as_str())
        .collect();
    let positions: Vec<i32> = targets.iter().map(|(position, _)| *position).collect();
    sqlx::query(
        r#"INSERT INTO task_channel_targets (
                task_id, company_id, channel_id, thread_id, recipient_role, position
           )
           SELECT $1, $2, target.channel_id, target.thread_id, target.recipient_role,
                  target.position
           FROM unnest($3::uuid[], $4::uuid[], $5::text[], $6::int4[])
                AS target (channel_id, thread_id, recipient_role, position)
           ON CONFLICT (task_id, channel_id) DO NOTHING"#,
    )
    .bind(task_id)
    .bind(company_id)
    .bind(&channel_ids)
    .bind(&thread_ids)
    .bind(&roles)
    .bind(&positions)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// The channels this run drives: the ones the producer stated, or its own if it stated none.
///
/// A schedule and an approval resume answer in exactly one place and say nothing; an inbound
/// message states every channel its ingest authorized. Reading them out of the payload JSON -- what
/// this replaces -- made the queue depend on one producer's payload shape and silently enqueued a
/// single-channel run for every other.
fn stated_or_own_channel(
    stated: Vec<TaskTarget>,
    channel_id: Uuid,
    thread_id: Option<Uuid>,
) -> Vec<TaskTarget> {
    if !stated.is_empty() {
        return stated;
    }
    thread_id
        .map(|thread_id| {
            vec![TaskTarget {
                channel_id,
                thread_id,
                recipient_role: RecipientRole::To,
            }]
        })
        .unwrap_or_default()
}
