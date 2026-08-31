//! The task lifecycle: claiming, attempt bookkeeping, failure, stop and resume, and the enqueue
//! that lands a task with its channel fan-out.
//!
//! The row-local transition attribution lives here too, beside the status-changing statements that
//! are required to write it.

use serde_json::Value;
use sqlx::Postgres;
use uuid::Uuid;

use super::*;
use crate::{
    app_error::{AppError, AppResult},
    entities::task::{
        BackgroundTask, NewTask, ResumeActor, StopActor, TaskFailure, TaskStopReason,
        TaskTransitionReason, TransitionActor,
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
   WHERE id = $1 AND status = 'pending' AND run_at <= CURRENT_TIMESTAMP"#;
/// Open the ledger row for one attempt.
///
/// The conflict is not an error and not a duplicate: a task whose lease lapsed is re-claimed with
/// its `retry_count` untouched — `mark_task_failed` never ran — so the new run carries the same
/// attempt number as the run that vanished. That earlier run reported nothing, so its half-written
/// row is reset here rather than left to be read as a finished attempt that took forever.
pub(crate) const BEGIN_ATTEMPT_SQL: &str = r#"INSERT INTO task_attempts
       (id, task_id, attempt_number, execution_generation, status, started_at)
   VALUES ($1, $2, $3, $4, 'processing', CURRENT_TIMESTAMP)
   ON CONFLICT (task_id, attempt_number) DO UPDATE
      SET status = 'processing', started_at = CURRENT_TIMESTAMP, finished_at = NULL,
          error = NULL, stop_reason = NULL, prompt_tokens = NULL, completion_tokens = NULL,
          execution_generation = EXCLUDED.execution_generation"#;

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
    };
    // The lease names the run that failed, so the failure cannot be attributed to anyone else.
    let attribution =
        TransitionAttribution::new(reason, TransitionActor::Worker(failure.lease.worker_id));
    let result = sqlx::query(&format!(
        r#"UPDATE background_tasks
           SET status = $1, retry_count = retry_count + 1, last_error = $2,
               run_at = $3, worker_id = NULL, execution_generation = NULL, locked_at = NULL,
               lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP, {attribution}
           WHERE id = $4 AND status = 'processing' AND worker_id = $5
             AND execution_generation = $6
             AND lock_expires_at > CURRENT_TIMESTAMP"#,
        attribution = attribution.set_clause(),
    ))
    .bind(failure.outcome.status().as_str())
    .bind(failure.error)
    .bind(failure.next_run_at)
    .bind(failure.lease.task_id)
    .bind(failure.lease.worker_id)
    .bind(failure.lease.execution_generation)
    .execute(pool)
    .await
    .map_err(AppError::from)?;
    Ok(result.rows_affected() == 1)
}

/// Which states each stop cause may act on.
///
/// An operator may stop anything still in flight or recoverable. A rejected approval may only stop
/// the task that approval parked -- a rejection is an answer to one question, not a licence to end
/// unrelated work that has since moved on.
pub(crate) fn stoppable_statuses(actor: StopActor) -> &'static str {
    match actor {
        StopActor::Operator(_) => {
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
    let db = sqlx::query_as::<_, BackgroundTaskDb>(&format!(
        r#"UPDATE background_tasks
           SET status = 'stopped', worker_id = NULL, execution_generation = NULL, locked_at = NULL,
               lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP, {attribution}
           WHERE id = $1
             AND status IN ({statuses})
           RETURNING id, company_id, channel_id, thread_id, correlation_id, task_type, status,
                     payload, retry_count, max_retries, last_error, worker_id,
                     execution_generation, locked_at, lock_expires_at, run_at, created_at,
                     updated_at"#,
        attribution = TransitionAttribution::stopped(actor).set_clause(),
        statuses = stoppable_statuses(actor),
    ))
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE task_outreaches SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP
           WHERE task_id = $1 AND status IN ('waiting', 'timeout_pending_approval')"#,
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE email_outbox SET status = 'failed', last_error = 'Task stopped',
               updated_at = CURRENT_TIMESTAMP
           WHERE task_id = $1 AND status = 'pending'"#,
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;
    tx.commit().await.map_err(AppError::from)?;
    db.try_into()
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

pub(crate) async fn resume_task_on(
    pool: &sqlx::PgPool,
    id: Uuid,
    actor: ResumeActor,
) -> AppResult<BackgroundTask> {
    let db = sqlx::query_as::<_, BackgroundTaskDb>(&format!(
        r#"UPDATE background_tasks
           SET status = 'pending', run_at = CURRENT_TIMESTAMP, worker_id = NULL,
               execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL,
               updated_at = CURRENT_TIMESTAMP, {attribution}{retry_budget}
           WHERE id = $1
             AND status IN ({statuses})
           RETURNING id, company_id, channel_id, thread_id, correlation_id, task_type, status,
                     payload, retry_count, max_retries, last_error, worker_id,
                     execution_generation, locked_at, lock_expires_at, run_at, created_at,
                     updated_at"#,
        attribution = TransitionAttribution::resumed(actor).set_clause(),
        retry_budget = retry_budget_clause(actor),
        statuses = resumable_statuses(actor),
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(AppError::from)?;
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
        correlation_id,
    } = new_task;
    let id = Uuid::new_v4();
    let source_message_id = payload
        .pointer("/inbound_message/message_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            payload
                .get("schedule_run_id")
                .and_then(Value::as_str)
                .map(|id| format!("schedule-run:{id}"))
        });
    let targets = task_targets(&payload, company_id, channel_id, thread_id)?;
    let db = sqlx::query_as::<_, BackgroundTaskDb>(
        r#"INSERT INTO background_tasks (
                id, company_id, channel_id, thread_id, source_message_id,
                correlation_id, task_type, status, payload
           )
           VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8)
           ON CONFLICT (company_id, source_message_id)
           -- Deliberately does not touch `correlation_id`: a redelivered message joins the chain
           -- its first delivery started rather than overwriting it with a fresher one.
           DO UPDATE SET source_message_id = EXCLUDED.source_message_id
           RETURNING id, company_id, channel_id, thread_id, correlation_id, task_type, status, payload,
                      retry_count, max_retries, last_error, worker_id, execution_generation, locked_at, lock_expires_at,
                      run_at, created_at, updated_at"#,
    )
    .bind(id)
    .bind(company_id)
    .bind(channel_id)
    .bind(thread_id)
    .bind(source_message_id)
    .bind(correlation_id.as_uuid())
    .bind(&task_type)
    .bind(payload)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;

    for (position, target) in targets.into_iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO task_channel_targets (
                    task_id, company_id, channel_id, thread_id, recipient_role, position
               ) VALUES ($1, $2, $3, $4, $5, $6)
               ON CONFLICT (task_id, channel_id) DO NOTHING"#,
        )
        .bind(db.id)
        .bind(company_id)
        .bind(target.channel_id)
        .bind(target.thread_id)
        .bind(target.recipient_role)
        .bind(position as i32)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    }

    db.try_into()
}

pub(crate) struct TaskTarget {
    pub(crate) channel_id: Uuid,
    pub(crate) thread_id: Uuid,
    pub(crate) recipient_role: String,
}

pub(crate) fn task_targets(
    payload: &Value,
    company_id: Uuid,
    primary_channel_id: Uuid,
    primary_thread_id: Option<Uuid>,
) -> AppResult<Vec<TaskTarget>> {
    let Some(matches) = payload.get("channel_matches").and_then(Value::as_array) else {
        return Ok(primary_thread_id
            .map(|thread_id| {
                vec![TaskTarget {
                    channel_id: primary_channel_id,
                    thread_id,
                    recipient_role: "to".to_string(),
                }]
            })
            .unwrap_or_default());
    };

    if matches.is_empty() {
        return Ok(primary_thread_id
            .map(|thread_id| {
                vec![TaskTarget {
                    channel_id: primary_channel_id,
                    thread_id,
                    recipient_role: "to".to_string(),
                }]
            })
            .unwrap_or_default());
    }

    matches
        .iter()
        .map(|entry| {
            let target_company = json_uuid(entry, "/company/id")?;
            if target_company != company_id {
                return Err(AppError::Internal(
                    "A task cannot target channels from multiple companies".into(),
                ));
            }

            Ok(TaskTarget {
                channel_id: json_uuid(entry, "/channel/id")?,
                thread_id: json_uuid(entry, "/thread/id")?,
                recipient_role: entry
                    .get("recipient_role")
                    .and_then(Value::as_str)
                    .unwrap_or("to")
                    .to_string(),
            })
        })
        .collect()
}

pub(crate) fn json_uuid(value: &Value, pointer: &str) -> AppResult<Uuid> {
    let raw = value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal(format!("Missing task target field {pointer}")))?;
    Uuid::parse_str(raw)
        .map_err(|_| AppError::Internal(format!("Invalid task target UUID at {pointer}")))
}
