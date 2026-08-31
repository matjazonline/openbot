use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::types::PgInterval;
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashMap;
use std::future::Future;
use std::str::FromStr;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        message::Message,
        outbox::{OutboxEntry, OutboxStatus},
        outreach::{
            CreateOutreachRequest, DueOutreach, OutreachProgress, OutreachReplyMatch,
            OutreachStatus,
        },
        stuck_work::{StuckWorkCensus, StuckWorkThresholds},
        task::{
            BackgroundTask, ChainStage, NewTask, TaskApprovalContext, TaskAttemptOutcome,
            TaskAttemptRecord, TaskAttemptRecordStatus, TaskAttemptRef, TaskAttemptStatus,
            TaskBoardFilter, TaskChainBoard, TaskChainCard, TaskChainCounts, TaskChainDetail,
            TaskChainTaskDetail, TaskLeaseRef, TaskOutreachContext, TaskStatus, TaskStatusEvent,
            TaskStatusEventCursor, TaskStopReason, TaskTransitionActorKind, TaskTransitionReason,
            ThreadActivity, TokenUsage,
        },
        value_objects::MessageId,
    },
};

pub const TASK_LEASE_SECONDS: i64 = 15 * 60;

/// Taking a task's lease. Only a task that is still pending and already due can be claimed, so two
/// callers racing for the same row leave exactly one of them holding it.
const CLAIM_TASK_SQL: &str = r#"UPDATE background_tasks
   SET status = 'processing', worker_id = $2, execution_generation = gen_random_uuid(),
       locked_at = CURRENT_TIMESTAMP,
       lock_expires_at = $3, updated_at = CURRENT_TIMESTAMP
   WHERE id = $1 AND status = 'pending' AND run_at <= CURRENT_TIMESTAMP"#;

/// Open the ledger row for one attempt.
///
/// The conflict is not an error and not a duplicate: a task whose lease lapsed is re-claimed with
/// its `retry_count` untouched — `mark_task_failed` never ran — so the new run carries the same
/// attempt number as the run that vanished. That earlier run reported nothing, so its half-written
/// row is reset here rather than left to be read as a finished attempt that took forever.
const BEGIN_ATTEMPT_SQL: &str = r#"INSERT INTO task_attempts
       (id, task_id, attempt_number, execution_generation, status, started_at)
   VALUES ($1, $2, $3, $4, 'processing', CURRENT_TIMESTAMP)
   ON CONFLICT (task_id, attempt_number) DO UPDATE
      SET status = 'processing', started_at = CURRENT_TIMESTAMP, finished_at = NULL,
          error = NULL, stop_reason = NULL, prompt_tokens = NULL, completion_tokens = NULL,
          execution_generation = EXCLUDED.execution_generation"#;

/// Close the ledger row, but only while it is still the open one. If another worker took the task
/// over and reopened the row, this run is no longer the run of record and must not overwrite it.
const FINISH_ATTEMPT_SQL: &str = r#"UPDATE task_attempts
   SET status = $4, error = $5, prompt_tokens = $6, completion_tokens = $7,
       stop_reason = $8,
       finished_at = CURRENT_TIMESTAMP
   WHERE task_id = $1 AND attempt_number = $2 AND execution_generation = $3
     AND status = 'processing'"#;

/// What a reaped run is recorded as having failed with. The reaper writes it to both the task row
/// and the attempt ledger, which must agree on why the run vanished.
const LEASE_EXPIRED_ERROR: &str = "Task lease expired without the run reporting a result";

pub(crate) async fn set_task_transition_context(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    reason: TaskTransitionReason,
    actor_kind: TaskTransitionActorKind,
    actor_id: Option<Uuid>,
) -> AppResult<()> {
    sqlx::query(
        r#"SELECT set_config('mail_agents.task_transition_reason', $1, TRUE),
                  set_config('mail_agents.task_transition_actor_kind', $2, TRUE),
                  set_config('mail_agents.task_transition_actor_id', COALESCE($3::text, ''), TRUE)"#,
    )
    .bind(reason.as_str())
    .bind(actor_kind.as_str())
    .bind(actor_id)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

pub(crate) async fn set_task_transition_source(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    approval_id: Option<Uuid>,
    outreach_id: Option<Uuid>,
) -> AppResult<()> {
    sqlx::query(
        r#"SELECT set_config(
                      'mail_agents.task_transition_approval_id', COALESCE($1::text, ''), TRUE
                  ),
                  set_config(
                      'mail_agents.task_transition_outreach_id', COALESCE($2::text, ''), TRUE
                  )"#,
    )
    .bind(approval_id)
    .bind(outreach_id)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Log when a lease-guarded state change was ignored because the lease or status moved on.
///
/// Both queues need this, and so does the inline dispatch path: a status write that quietly does
/// nothing leaves its row claimed, and a claimed row that never reports a result is redelivered —
/// or re-executed — on every lease expiry after that.
pub fn report_outcome(subject: &str, id: Uuid, change: &str, outcome: AppResult<bool>) {
    match outcome {
        Ok(true) => {}
        Ok(false) => {
            warn!("{subject} {id} {change} ignored because its lease or status changed")
        }
        Err(err) => error!("Failed to record {subject} {id} {change}: {err}"),
    }
}

/// A claim on a task: who holds it, and for how long at a time. Holding the *duration* rather than
/// a computed deadline is what lets a lease extend itself — a precomputed `expires_at` is only ever
/// right at the instant it was made, and every renewal after that has to guess the interval again.
#[derive(Clone, Copy, Debug)]
pub struct TaskLease {
    /// Which task, held by which worker, for which run.
    pub reference: TaskLeaseRef,
    ttl: chrono::Duration,
}

impl TaskLease {
    /// A worker's lease on a queued task: long, because the worker heartbeats it.
    pub fn worker(reference: TaskLeaseRef) -> Self {
        Self::new(reference, TASK_LEASE_SECONDS)
    }

    fn new(reference: TaskLeaseRef, ttl_seconds: i64) -> Self {
        Self {
            reference,
            ttl: chrono::Duration::seconds(ttl_seconds),
        }
    }

    /// The deadline a claim or renewal made *now* should carry.
    pub fn expires_at(&self) -> DateTime<Utc> {
        Utc::now() + self.ttl
    }

    /// How often to renew: a third of the term, so two beats can be missed before it lapses.
    pub fn heartbeat(&self) -> Duration {
        Duration::from_secs((self.ttl.num_seconds() / 3).max(1) as u64)
    }
}

/// What became of work run under a lease.
pub enum Leased<T> {
    Finished(T),
    /// The lease could not be renewed — the task was stopped, another worker took it over, or the
    /// database is unreachable. Whatever this run was doing, it is no longer the run of record and
    /// must not report a result.
    Lost,
}

/// Run `work` while keeping `lease` alive, renewing on every [`TaskLease::heartbeat`].
///
/// The worker holds a lease across a task it polled for, and cannot assume that task finishes
/// inside a single lease term — a lapsed lease means a second run of the same agent.
pub async fn while_leased<F: Future>(
    persistence: &dyn TaskPersistence,
    lease: &TaskLease,
    work: F,
) -> Leased<F::Output> {
    let task_id = lease.reference.task_id;
    tokio::pin!(work);
    let mut heartbeat = tokio::time::interval(lease.heartbeat());
    // The first tick completes immediately; spend it here rather than renewing a lease just taken.
    heartbeat.tick().await;

    loop {
        tokio::select! {
            output = &mut work => return Leased::Finished(output),
            _ = heartbeat.tick() => {
                match persistence
                    .renew_task_lease(lease.reference, lease.expires_at())
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => return Leased::Lost,
                    Err(error) => {
                        error!("Failed to renew the lease on task {task_id}: {error}");
                        return Leased::Lost;
                    }
                }
            }
        }
    }
}

#[derive(sqlx::FromRow, Debug)]
pub struct BackgroundTaskDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub correlation_id: Uuid,
    pub task_type: String,
    pub status: String,
    pub payload: Value,
    pub retry_count: i32,
    pub max_retries: i32,
    pub last_error: Option<String>,
    pub worker_id: Option<Uuid>,
    pub execution_generation: Option<Uuid>,
    pub locked_at: Option<DateTime<Utc>>,
    pub lock_expires_at: Option<DateTime<Utc>>,
    pub run_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Debug)]
struct TaskAttemptRecordDb {
    attempt_number: i32,
    status: String,
    error: Option<String>,
    stop_reason: Option<String>,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    result: Option<Value>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    execution_generation: Uuid,
}

/// One attempt as read by a chain-wide batch, carrying the task it belongs to.
///
/// The flattened row is the same shape the per-task read uses, so the two cannot drift into
/// selecting different columns.
#[derive(sqlx::FromRow, Debug)]
struct ChainAttemptDb {
    task_id: Uuid,
    #[sqlx(flatten)]
    record: TaskAttemptRecordDb,
}

#[derive(sqlx::FromRow, Debug)]
struct TaskStatusEventDb {
    id: Uuid,
    company_id: Uuid,
    task_id: Uuid,
    correlation_id: Uuid,
    sequence: i32,
    from_status: Option<String>,
    to_status: String,
    reason: String,
    actor_kind: String,
    actor_id: Option<Uuid>,
    related_approval_id: Option<Uuid>,
    related_outreach_id: Option<Uuid>,
    retry_count: i32,
    run_at: DateTime<Utc>,
    execution_generation: Option<Uuid>,
    transitioned_at: DateTime<Utc>,
}

impl TryFrom<TaskStatusEventDb> for TaskStatusEvent {
    type Error = AppError;

    fn try_from(db: TaskStatusEventDb) -> AppResult<Self> {
        let row = format!("task_status_events row {}", db.id);
        let parse_status = |value: &str| {
            TaskStatus::from_str(value)
                .map_err(|error| AppError::Internal(format!("{row}: {error}")))
        };
        Ok(Self {
            id: db.id,
            company_id: db.company_id,
            task_id: db.task_id,
            correlation_id: db.correlation_id.into(),
            sequence: db.sequence,
            from_status: db.from_status.as_deref().map(parse_status).transpose()?,
            to_status: parse_status(&db.to_status)?,
            reason: TaskTransitionReason::from_str(&db.reason)
                .map_err(|error| AppError::Internal(format!("{row}: {error}")))?,
            actor_kind: TaskTransitionActorKind::from_str(&db.actor_kind)
                .map_err(|error| AppError::Internal(format!("{row}: {error}")))?,
            actor_id: db.actor_id,
            related_approval_id: db.related_approval_id,
            related_outreach_id: db.related_outreach_id,
            retry_count: db.retry_count,
            run_at: db.run_at,
            execution_generation: db.execution_generation,
            transitioned_at: db.transitioned_at,
        })
    }
}

#[derive(sqlx::FromRow, Debug)]
struct TaskChainCardDb {
    correlation_id: Uuid,
    stage: String,
    title: String,
    channel_names: Vec<String>,
    agent_names: Vec<String>,
    total_tasks: i64,
    pending: i64,
    processing: i64,
    expired_processing: i64,
    pending_approval: i64,
    waiting_reply: i64,
    completed: i64,
    failed: i64,
    dead_letter: i64,
    stopped: i64,
    total_deliveries: i64,
    delivery_pending: i64,
    delivery_sending: i64,
    delivery_sent: i64,
    delivery_failed: i64,
    created_at: DateTime<Utc>,
    last_activity_at: DateTime<Utc>,
    next_action_at: Option<DateTime<Utc>>,
    retry_count: i64,
    failure_summary: Option<String>,
    stage_total: i64,
}

impl TryFrom<TaskChainCardDb> for (TaskChainCard, i64) {
    type Error = AppError;

    fn try_from(db: TaskChainCardDb) -> AppResult<Self> {
        let stage = ChainStage::from_str(&db.stage).map_err(|error| {
            AppError::Internal(format!(
                "task chain {} has invalid stage: {error}",
                db.correlation_id
            ))
        })?;
        let counts = TaskChainCounts {
            total_tasks: db.total_tasks,
            pending: db.pending,
            processing: db.processing,
            expired_processing: db.expired_processing,
            pending_approval: db.pending_approval,
            waiting_reply: db.waiting_reply,
            completed: db.completed,
            failed: db.failed,
            dead_letter: db.dead_letter,
            stopped: db.stopped,
            total_deliveries: db.total_deliveries,
            delivery_pending: db.delivery_pending,
            delivery_sending: db.delivery_sending,
            delivery_sent: db.delivery_sent,
            delivery_failed: db.delivery_failed,
        };
        debug_assert_eq!(stage, ChainStage::derive(&counts));
        Ok((
            TaskChainCard {
                correlation_id: db.correlation_id.into(),
                stage,
                title: db.title,
                channel_names: db.channel_names,
                agent_names: db.agent_names,
                counts,
                created_at: db.created_at,
                last_activity_at: db.last_activity_at,
                next_action_at: db.next_action_at,
                retry_count: db.retry_count,
                failure_summary: db.failure_summary,
            },
            db.stage_total,
        ))
    }
}

/// Which chains the board considers, selected one row at a time.
///
/// Every predicate in `staged` is an aggregate over `GROUP BY correlation_id`, and Postgres cannot
/// push an aggregate predicate below its own grouping — so on its own the window filter runs
/// *after* the scan it was meant to bound, over every task the company has ever run. These are the
/// same three conditions written at row level, where an index can serve them: unfinished work,
/// unresolved work, or anything touched since the cutoff. The outbox arm also closes a real gap —
/// a chain whose only recent activity is a delivery was previously selected only incidentally.
const BOARD_ELIGIBLE_RECENT: &str = r#"
                   SELECT correlation_id FROM (
                       SELECT correlation_id
                       FROM background_tasks
                       WHERE company_id = $1
                         AND (status IN ('pending', 'processing', 'pending_approval',
                                         'waiting_for_third_party_reply', 'failed', 'dead_letter')
                              OR updated_at >= $3)
                       UNION
                       SELECT correlation_id
                       FROM email_outbox
                       WHERE company_id = $1
                         AND (status IN ('pending', 'sending', 'failed')
                              OR updated_at >= $3)
                   ) AS recent
                   WHERE $2::uuid IS NULL OR EXISTS (
                       SELECT 1
                       FROM background_tasks AS filtered_task
                       WHERE filtered_task.company_id = $1
                         AND filtered_task.correlation_id = recent.correlation_id
                         AND filtered_task.channel_id = $2
                   )"#;

/// The pre-pushdown selection: every chain in the company, filtered only by channel.
///
/// Kept as the control the equivalence test runs the same projection over. It is the definition of
/// "correct" that [`BOARD_ELIGIBLE_RECENT`] has to keep matching, and the only thing standing
/// between a faster query and a quietly different one.
#[cfg(test)]
const BOARD_ELIGIBLE_EVERY_CHAIN: &str = r#"
                   SELECT DISTINCT task.correlation_id
                   FROM background_tasks AS task
                   WHERE task.company_id = $1
                     AND ($2::uuid IS NULL OR EXISTS (
                         SELECT 1
                         FROM background_tasks AS filtered_task
                         WHERE filtered_task.company_id = task.company_id
                           AND filtered_task.correlation_id = task.correlation_id
                           AND filtered_task.channel_id = $2
                     ))"#;

/// The board projection over whichever chain selection it is given.
///
/// The selection is a parameter so the equivalence test can drive the identical projection from
/// [`BOARD_ELIGIBLE_EVERY_CHAIN`]; production always assembles it once from
/// [`BOARD_ELIGIBLE_RECENT`], in [`BOARD_QUERY`].
fn board_query_sql(eligible: &str) -> String {
    format!(
        r#"WITH eligible AS ({eligible}
               ),
               task_rollup AS (
                   SELECT task.correlation_id,
                          (array_agg(
                              COALESCE(NULLIF(thread.subject, ''), task.task_type)
                              ORDER BY task.created_at, task.id
                          ))[1] AS title,
                          COUNT(*)::bigint AS total_tasks,
                          COUNT(*) FILTER (WHERE task.status = 'pending')::bigint AS pending,
                          COUNT(*) FILTER (WHERE task.status = 'processing')::bigint AS processing,
                          COUNT(*) FILTER (
                              WHERE task.status = 'processing'
                                AND task.lock_expires_at <= CURRENT_TIMESTAMP
                          )::bigint AS expired_processing,
                          COUNT(*) FILTER (WHERE task.status = 'pending_approval')::bigint
                              AS pending_approval,
                          COUNT(*) FILTER (
                              WHERE task.status = 'waiting_for_third_party_reply'
                          )::bigint AS waiting_reply,
                          COUNT(*) FILTER (WHERE task.status = 'completed')::bigint AS completed,
                          COUNT(*) FILTER (WHERE task.status = 'failed')::bigint AS failed,
                          COUNT(*) FILTER (WHERE task.status = 'dead_letter')::bigint
                              AS dead_letter,
                          COUNT(*) FILTER (WHERE task.status = 'stopped')::bigint AS stopped,
                          SUM(task.retry_count)::bigint AS retry_count,
                          MIN(task.created_at) AS created_at,
                          MAX(task.updated_at) AS task_last_activity,
                          LEAST(
                              MIN(task.run_at) FILTER (WHERE task.status = 'pending'),
                              MIN(task.wait_expires_at) FILTER (
                                  WHERE task.status = 'waiting_for_third_party_reply'
                              )
                          ) AS task_next_action,
                          CASE
                              WHEN COUNT(*) FILTER (
                                  WHERE task.status IN ('failed', 'dead_letter')
                              ) > 0 THEN 'One or more tasks failed'
                              WHEN COUNT(*) FILTER (WHERE task.status = 'stopped') > 0
                                  THEN 'Stopped by an operator'
                              ELSE NULL
                          END AS failure_summary
                   FROM background_tasks AS task
                   JOIN eligible ON eligible.correlation_id = task.correlation_id
                   LEFT JOIN threads AS thread
                     ON thread.company_id = task.company_id
                    AND thread.channel_id = task.channel_id
                    AND thread.id = task.thread_id
                   WHERE task.company_id = $1
                   GROUP BY task.correlation_id
               ),
               participant_rollup AS (
                   SELECT task.correlation_id,
                          array_agg(DISTINCT channel.name ORDER BY channel.name) AS channel_names,
                          COALESCE(
                              array_agg(DISTINCT agent.name ORDER BY agent.name)
                                  FILTER (WHERE agent.id IS NOT NULL),
                              ARRAY[]::text[]
                          ) AS agent_names
                   FROM background_tasks AS task
                   JOIN eligible ON eligible.correlation_id = task.correlation_id
                   JOIN channels AS channel
                     ON channel.company_id = task.company_id AND channel.id = task.channel_id
                   LEFT JOIN channel_agents AS assignment
                     ON assignment.company_id = channel.company_id
                    AND assignment.channel_id = channel.id
                   LEFT JOIN agents AS agent ON agent.id = assignment.agent_id
                   WHERE task.company_id = $1
                   GROUP BY task.correlation_id
               ),
               delivery_rollup AS (
                   SELECT outbox.correlation_id,
                          COUNT(*)::bigint AS total_deliveries,
                          COUNT(*) FILTER (WHERE outbox.status = 'pending')::bigint
                              AS delivery_pending,
                          COUNT(*) FILTER (WHERE outbox.status = 'sending')::bigint
                              AS delivery_sending,
                          COUNT(*) FILTER (WHERE outbox.status = 'sent')::bigint AS delivery_sent,
                          COUNT(*) FILTER (WHERE outbox.status = 'failed')::bigint
                              AS delivery_failed,
                          MAX(outbox.updated_at) AS delivery_last_activity,
                          MIN(outbox.available_at) FILTER (WHERE outbox.status = 'pending')
                              AS delivery_next_action
                   FROM email_outbox AS outbox
                   JOIN eligible ON eligible.correlation_id = outbox.correlation_id
                   WHERE outbox.company_id = $1
                   GROUP BY outbox.correlation_id
               ),
               combined AS (
                   SELECT task_rollup.correlation_id, task_rollup.title,
                          participant_rollup.channel_names, participant_rollup.agent_names,
                          task_rollup.total_tasks, task_rollup.pending, task_rollup.processing,
                          task_rollup.expired_processing, task_rollup.pending_approval,
                          task_rollup.waiting_reply, task_rollup.completed, task_rollup.failed,
                          task_rollup.dead_letter, task_rollup.stopped,
                          COALESCE(delivery_rollup.total_deliveries, 0) AS total_deliveries,
                          COALESCE(delivery_rollup.delivery_pending, 0) AS delivery_pending,
                          COALESCE(delivery_rollup.delivery_sending, 0) AS delivery_sending,
                          COALESCE(delivery_rollup.delivery_sent, 0) AS delivery_sent,
                          COALESCE(delivery_rollup.delivery_failed, 0) AS delivery_failed,
                          task_rollup.created_at,
                          GREATEST(
                              task_rollup.task_last_activity,
                              COALESCE(delivery_rollup.delivery_last_activity, '-infinity')
                          ) AS last_activity_at,
                          LEAST(task_rollup.task_next_action, delivery_rollup.delivery_next_action)
                              AS next_action_at,
                          task_rollup.retry_count, task_rollup.failure_summary,
                          (task_rollup.pending + task_rollup.processing
                              + task_rollup.pending_approval + task_rollup.waiting_reply
                              + COALESCE(delivery_rollup.delivery_pending, 0)
                              + COALESCE(delivery_rollup.delivery_sending, 0)) > 0 AS is_active,
                          (task_rollup.failed + task_rollup.dead_letter
                              + task_rollup.expired_processing
                              + COALESCE(delivery_rollup.delivery_failed, 0)) > 0 AS is_unresolved
                   FROM task_rollup
                   JOIN participant_rollup
                     ON participant_rollup.correlation_id = task_rollup.correlation_id
                   LEFT JOIN delivery_rollup
                     ON delivery_rollup.correlation_id = task_rollup.correlation_id
               ),
               staged AS (
                   SELECT combined.*,
                          CASE
                              WHEN failed > 0 OR dead_letter > 0 OR stopped > 0
                                   OR expired_processing > 0 OR delivery_failed > 0
                                  THEN 'needs_attention'
                              WHEN pending_approval > 0 THEN 'waiting_approval'
                              WHEN processing > 0 OR delivery_sending > 0 THEN 'running'
                              WHEN waiting_reply > 0 THEN 'waiting_reply'
                              WHEN pending > 0 OR delivery_pending > 0 THEN 'queued'
                              WHEN total_tasks > 0 AND completed = total_tasks
                                   AND delivery_sent = total_deliveries THEN 'completed'
                              ELSE 'needs_attention'
                          END AS stage
                   FROM combined
                   -- Redundant with the row-level pushdown in `eligible`, and deliberately so:
                   -- the two select the same set, which is what makes the pushdown testable and
                   -- keeps this aggregate as the readable specification of what the board shows.
                   WHERE is_active OR is_unresolved OR last_activity_at >= $3
               ),
               ranked AS (
                   SELECT staged.*,
                          COUNT(*) OVER (PARTITION BY stage)::bigint AS stage_total,
                          ROW_NUMBER() OVER (
                              PARTITION BY stage
                              ORDER BY last_activity_at DESC, correlation_id
                          ) AS stage_rank
                   FROM staged
               )
               SELECT correlation_id, stage, title, channel_names, agent_names,
                      total_tasks, pending, processing, expired_processing, pending_approval,
                      waiting_reply, completed, failed, dead_letter, stopped,
                      total_deliveries, delivery_pending, delivery_sending, delivery_sent,
                      delivery_failed, created_at, last_activity_at, next_action_at, retry_count,
                      failure_summary, stage_total
               FROM ranked
               WHERE stage_rank <= $4
               ORDER BY CASE stage
                            WHEN 'queued' THEN 1
                            WHEN 'running' THEN 2
                            WHEN 'waiting_approval' THEN 3
                            WHEN 'waiting_reply' THEN 4
                            WHEN 'completed' THEN 5
                            ELSE 6
                        END,
                        last_activity_at DESC, correlation_id"#
    )
}

/// The board query as production runs it, assembled once rather than per render.
static BOARD_QUERY: LazyLock<String> = LazyLock::new(|| board_query_sql(BOARD_ELIGIBLE_RECENT));

/// The most status events one page of the chain timeline may carry. Public and documented on
/// [`crate::use_cases::thread::TaskPersistence::list_task_status_events`]; the detail loader keeps
/// its own limit rather than widening this one.
const MAX_STATUS_EVENT_PAGE: usize = 200;

/// A chain detail pane is an operational view, not an export: past these the pane truncates and
/// says so rather than projecting an unbounded ledger into one HTML response.
const CHAIN_DETAIL_MAX_TASKS: i64 = 200;
const CHAIN_DETAIL_MAX_ATTEMPTS: i64 = 1_000;
const CHAIN_DETAIL_MAX_DELIVERIES: i64 = 1_000;
const CHAIN_DETAIL_MAX_EVENTS: i64 = 200;
const CHAIN_DETAIL_MAX_APPROVALS: i64 = 200;
const CHAIN_DETAIL_MAX_OUTREACHES: i64 = 200;

/// What a bounded read actually asks the database for: one row past the limit.
///
/// That sentinel is the whole difference between "there are exactly `limit` of these" and "there
/// are more than `limit` of these" — a result that merely happens to be `limit` long is not
/// truncation, and must not be reported as such.
fn probe_limit(limit: i64) -> i64 {
    limit + 1
}

/// Drop the sentinel row [`probe_limit`] fetched, answering whether it was there.
fn trim_to_limit<T>(rows: &mut Vec<T>, limit: i64) -> bool {
    let limit = limit as usize;
    let truncated = rows.len() > limit;
    rows.truncate(limit);
    truncated
}

/// Redistribute one batched result set back onto the tasks its rows belong to.
///
/// Synchronous on purpose: the chain-detail path is already several `await`s deep, and grouping
/// rows in memory never waits on anything, so an `async fn` here would be a future frame bought
/// for nothing.
fn group_by_task<T>(rows: impl IntoIterator<Item = (Uuid, T)>) -> HashMap<Uuid, Vec<T>> {
    let mut grouped: HashMap<Uuid, Vec<T>> = HashMap::new();
    for (task_id, item) in rows {
        grouped.entry(task_id).or_default().push(item);
    }
    grouped
}

/// The company-scoped chain timeline, built in one place so the public page and the detail pane
/// cannot disagree about what a page of it contains.
///
/// `limit` is passed through unclamped: the port clamps to [`MAX_STATUS_EVENT_PAGE`] before
/// calling, and the detail loader deliberately asks for one row more than its own limit.
fn chain_status_events_query<'a>(
    company_id: Uuid,
    correlation_id: CorrelationId,
    cursor: Option<TaskStatusEventCursor>,
    limit: i64,
) -> QueryBuilder<'a, Postgres> {
    let mut query = QueryBuilder::<Postgres>::new(
        r#"SELECT id, company_id, task_id, correlation_id, sequence, from_status, to_status,
                  reason, actor_kind, actor_id, related_approval_id, related_outreach_id,
                  retry_count, run_at, execution_generation, transitioned_at
           FROM task_status_events
           WHERE company_id = "#,
    );
    query
        .push_bind(company_id)
        .push(" AND correlation_id = ")
        .push_bind(correlation_id.as_uuid());
    if let Some(cursor) = cursor {
        query
            .push(" AND (transitioned_at, task_id, sequence, id) > (")
            .push_bind(cursor.transitioned_at)
            .push(", ")
            .push_bind(cursor.task_id)
            .push(", ")
            .push_bind(cursor.sequence)
            .push(", ")
            .push_bind(cursor.id)
            .push(")");
    }
    query
        .push(" ORDER BY transitioned_at, task_id, sequence, id LIMIT ")
        .push_bind(limit);
    query
}

/// Past this, the per-render board projection is worth looking at. Not an SLO — a tripwire for the
/// question `plan/kanban_denormalized_rollup_table_optimization.md` defers, so that the decision to
/// build a denormalized rollup table rests on a measurement rather than on a hunch.
const BOARD_QUERY_WARN_THRESHOLD: Duration = Duration::from_millis(500);

/// Log the board projection when it runs long, with the number that actually decides whether the
/// query needs rethinking.
///
/// `returned_cards` is capped by the per-column display limit, so it plateaus and says nothing
/// about cost. Every row of a non-empty stage carries that stage's shared total, so summing one
/// total per stage recovers the working set the query really aggregated over — the value to watch
/// grow.
fn warn_if_board_projection_is_slow(company_id: Uuid, elapsed: Duration, rows: &[TaskChainCardDb]) {
    if elapsed <= BOARD_QUERY_WARN_THRESHOLD {
        return;
    }
    let eligible_chains: i64 = rows
        .iter()
        .map(|row| (row.stage.as_str(), row.stage_total))
        .collect::<HashMap<_, _>>()
        .into_values()
        .sum();
    warn!(
        ?elapsed,
        %company_id,
        returned_cards = rows.len(),
        eligible_chains,
        "Task board projection is slow"
    );
}

impl TryFrom<TaskAttemptRecordDb> for TaskAttemptRecord {
    type Error = AppError;

    fn try_from(db: TaskAttemptRecordDb) -> AppResult<Self> {
        Ok(Self {
            attempt_number: db.attempt_number,
            status: TaskAttemptRecordStatus::from_str(&db.status).map_err(AppError::Internal)?,
            error: db.error,
            stop_reason: db
                .stop_reason
                .map(|reason| TaskStopReason::from_str(&reason).map_err(AppError::Internal))
                .transpose()?,
            prompt_tokens: db.prompt_tokens,
            completion_tokens: db.completion_tokens,
            result: db.result,
            started_at: db.started_at,
            finished_at: db.finished_at,
            execution_generation: db.execution_generation,
        })
    }
}

#[derive(sqlx::FromRow, Debug)]
pub struct OutboxEmail {
    pub id: Uuid,
    pub payload: Value,
    /// The stable key this send was queued under. The delivered Message-ID is derived from it, so
    /// every attempt at the same logical send goes out under the same Message-ID — and so a caller
    /// that queued the row can predict that Message-ID without waiting for delivery.
    pub idempotency_key: String,
}

#[derive(sqlx::FromRow, Debug)]
struct OutreachDb {
    id: Uuid,
    task_id: Uuid,
    status: String,
    required_threshold_percent: f64,
    expires_at: DateTime<Utc>,
}

impl TryFrom<BackgroundTaskDb> for BackgroundTask {
    type Error = AppError;

    fn try_from(db: BackgroundTaskDb) -> AppResult<Self> {
        let status =
            TaskStatus::from_str(&db.status).map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(BackgroundTask {
            id: db.id,
            company_id: db.company_id,
            channel_id: db.channel_id,
            thread_id: db.thread_id,
            correlation_id: db.correlation_id.into(),
            task_type: db.task_type,
            status,
            payload: db.payload,
            retry_count: db.retry_count,
            max_retries: db.max_retries,
            last_error: db.last_error,
            worker_id: db.worker_id,
            execution_generation: db.execution_generation,
            locked_at: db.locked_at,
            lock_expires_at: db.lock_expires_at,
            run_at: db.run_at,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

/// One `email_outbox` row as stored, before its `TEXT` status is parsed.
#[derive(sqlx::FromRow, Debug)]
pub struct OutboxEntryDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub status: String,
    pub idempotency_key: String,
    pub payload: Value,
    pub retry_count: i32,
    pub last_error: Option<String>,
    pub provider_message_id: Option<String>,
    pub available_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<OutboxEntryDb> for OutboxEntry {
    type Error = AppError;

    fn try_from(db: OutboxEntryDb) -> AppResult<Self> {
        let status =
            OutboxStatus::from_str(&db.status).map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(OutboxEntry {
            id: db.id,
            company_id: db.company_id,
            channel_id: db.channel_id,
            task_id: db.task_id,
            status,
            idempotency_key: db.idempotency_key,
            payload: db.payload,
            retry_count: db.retry_count,
            last_error: db.last_error,
            provider_message_id: db.provider_message_id,
            available_at: db.available_at,
            sent_at: db.sent_at,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

/// The columns [`OutboxEntryDb`] reads, named once so the list and the single-row read cannot
/// select different shapes.
const OUTBOX_COLUMNS: &str = r#"id, company_id, channel_id, task_id, status, idempotency_key, payload,
              retry_count, last_error, provider_message_id, available_at, sent_at,
              created_at, updated_at"#;

/// The most delivery attempts one outbox row gets before it is dead-lettered.
const OUTBOX_MAX_ATTEMPTS: i32 = 5;

/// The `SET` clause that ends one delivery attempt: count it, back off, and dead-letter once the
/// attempts run out. `error_sql` is however that statement names the error — a bind parameter or a
/// literal.
///
/// Shared so an attempt that failed outright and one that stranded its lease age at the same rate.
/// A row that only ever expires must still reach `failed`; while expiry was uncounted, the poller
/// redelivered such a row every lease period forever.
fn outbox_attempt_failed_set(error_sql: &str) -> String {
    format!(
        r#"SET status = CASE WHEN retry_count + 1 >= {OUTBOX_MAX_ATTEMPTS} THEN 'failed' ELSE 'pending' END,
                   retry_count = retry_count + 1, last_error = {error_sql},
                   available_at = CURRENT_TIMESTAMP
                       + make_interval(secs => power(2, LEAST(retry_count + 1, 8))::double precision),
                   worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP"#
    )
}

/// One email handed to the transport for delivery.
///
/// A struct rather than four positional parameters, two of which are `Uuid`.
pub struct OutboundSend {
    pub company_id: Uuid,
    /// The channel this email goes out as. Unlike `task_id` it is not a lifecycle join — it is the
    /// dimension the outbox is filtered by.
    pub channel_id: Uuid,
    /// The task whose work produced this email. Carries no lifecycle meaning — it is the join the
    /// task view uses to show delivery state, nothing writes back through it.
    pub task_id: Option<Uuid>,
    /// The chain this email belongs to, inherited from the task that produced it. Unlike
    /// `task_id` it is never cleared, so a delivered email stays attached to its trail even after
    /// the task row is gone.
    pub correlation_id: CorrelationId,
    /// Stable across every retry of the same logical send — that is what makes it a lock, and what
    /// the delivered Message-ID is derived from.
    pub idempotency_key: String,
    /// The `OutboundEmail` for the poller to deliver.
    pub payload: serde_json::Value,
}

/// Everything one agent dispatch makes durable, so it can land as a single transaction.
///
/// The reply message in each answered thread, the outbox row that delivers it, and the audit
/// payload on the task are one result. Committed separately, a crash or a lost lease between
/// them leaves the thread showing an answer that was never sent, or an email going out for a
/// task whose payload says it never ran -- and the retry then has to reconcile the difference.
pub struct AgentDispatchCommit<'a> {
    /// Proof this run still owns the task. The whole transaction is fenced on it.
    pub lease: TaskLeaseRef,
    /// The reply, stored once per thread it answered.
    pub messages: &'a [Message],
    /// The email to hand to the outbox, or `None` for a simulated run that sends nothing.
    pub outbound: Option<OutboundSend>,
    /// The run's audit payload, written back onto the task.
    pub payload: Value,
    /// Whether this dispatch also closes the task's outreach.
    pub complete_outreach: bool,
}

/// What [`TaskPersistence::commit_agent_dispatch`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchCommit {
    /// Everything landed. `outbox_id` is `None` when an equivalent send was already queued, which
    /// is the idempotency key doing its job rather than a failure.
    Committed { outbox_id: Option<Uuid> },
    /// This run no longer owns the task, so nothing was written at all.
    LeaseLost,
}

#[async_trait]
pub trait TaskPersistence: Send + Sync {
    /// What each of these threads is currently doing, for the mailbox's activity indicators.
    ///
    /// Batched deliberately: the thread column renders up to a full page of rows at once, and one
    /// query per row would be a page-load's worth of round trips. Threads with nothing in flight
    /// are simply absent from the map.
    async fn list_thread_activity(
        &self,
        _thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadActivity>> {
        Ok(HashMap::new())
    }

    async fn create_outreach_and_pause(
        &self,
        _request: CreateOutreachRequest,
    ) -> AppResult<OutreachProgress> {
        Err(AppError::Internal(
            "Outreach persistence is not configured".into(),
        ))
    }

    async fn find_correlated_outreach_reply(
        &self,
        _company_id: Uuid,
        _channel_id: Uuid,
        _thread_id: Uuid,
        _sender: &str,
        _references: &[MessageId],
    ) -> AppResult<Option<OutreachReplyMatch>> {
        Ok(None)
    }

    async fn record_outreach_reply(
        &self,
        _matched: &OutreachReplyMatch,
        _response_message_id: Uuid,
    ) -> AppResult<OutreachProgress> {
        Err(AppError::Internal(
            "Outreach persistence is not configured".into(),
        ))
    }

    async fn list_due_outreaches(
        &self,
        _due_at: DateTime<Utc>,
        _limit: i64,
    ) -> AppResult<Vec<DueOutreach>> {
        Ok(Vec::new())
    }

    async fn mark_outreach_timeout_pending(&self, _outreach_id: Uuid) -> AppResult<bool> {
        Ok(false)
    }

    async fn restore_outreach_waiting(&self, _outreach_id: Uuid) -> AppResult<()> {
        Ok(())
    }

    async fn get_outreach_context(&self, _task_id: Uuid) -> AppResult<Option<String>> {
        Ok(None)
    }

    async fn complete_outreach(&self, _task_id: Uuid) -> AppResult<()> {
        Ok(())
    }

    async fn claim_outbox_emails(
        &self,
        _worker_id: Uuid,
        _lock_expires_at: DateTime<Utc>,
        _limit: i64,
    ) -> AppResult<Vec<OutboxEmail>> {
        Ok(Vec::new())
    }

    async fn mark_outbox_email_sent(
        &self,
        _id: Uuid,
        _worker_id: Uuid,
        _provider_message_id: &str,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn mark_outbox_email_failed(
        &self,
        _id: Uuid,
        _worker_id: Uuid,
        _error: &str,
    ) -> AppResult<bool> {
        Ok(true)
    }

    /// Dead-letter one claimed delivery outright, without spending its remaining attempts.
    ///
    /// For a failure that cannot come out differently next time — a payload that will not
    /// deserialize, say. Retrying those only delays the same verdict by five backoffs.
    async fn mark_outbox_email_dead(
        &self,
        _id: Uuid,
        _worker_id: Uuid,
        _error: &str,
    ) -> AppResult<bool> {
        Ok(true)
    }

    /// End every delivery whose lease has run out, counting each as a spent attempt.
    ///
    /// A claimed row whose worker died — or whose status write never landed — is otherwise stuck
    /// in `sending` with its attempt count untouched, so it is redelivered every lease period and
    /// never reaches the dead-letter cap. Returns how many rows were reaped.
    async fn reap_expired_outbox_leases(&self) -> AppResult<u64> {
        Ok(0)
    }

    /// Give back every task whose lease lapsed without the run reporting anything, charging each
    /// one an attempt.
    ///
    /// Claims used to steal an expired `processing` row directly, which re-ran it with
    /// `retry_count` untouched, left its open attempt sitting in the ledger and applied no
    /// backoff -- so a task that reliably outlived its lease was retried for ever and never
    /// dead-lettered. Reaping makes lease expiry cost exactly what a reported failure costs.
    ///
    /// Returns how many rows were reaped. Defaulted to a no-op for doubles that never lease.
    async fn reap_expired_task_leases(&self) -> AppResult<u64> {
        Ok(0)
    }

    /// How much work is stuck right now, by kind.
    ///
    /// One query rather than seven: the maintenance sweep runs on a timer against the same pool
    /// the workers claim from, and seven round trips every thirty seconds to answer one question
    /// is a poor trade for a sweep that usually finds nothing.
    ///
    /// The default reports a quiet system, which is the honest answer for a double that stores no
    /// queue at all.
    async fn census_stuck_work(
        &self,
        _thresholds: StuckWorkThresholds,
    ) -> AppResult<StuckWorkCensus> {
        Ok(StuckWorkCensus::default())
    }

    /// Every delivery this task handed to the transport, oldest first.
    ///
    /// Exists so the task view can show delivery state without the transport writing back into the
    /// task. The default returns nothing, which renders as "no deliveries".
    /// What the transport did with the emails one task produced.
    ///
    /// Read-only, and deliberately so — the task and the transport are separate processes, and
    /// this is the join the UI makes at render time, not a channel either one writes through.
    async fn list_task_deliveries(&self, _task_id: Uuid) -> AppResult<Vec<OutboxEntry>> {
        Ok(Vec::new())
    }

    /// One filtered page of the company's outbox, newest first unless `sort_asc`.
    ///
    /// `limit` is the caller's probe size, so whether a further page exists comes back with the
    /// page itself; see [`crate::entities::outbox::OutboxFilter::probe_limit`].
    async fn list_company_outbox_page(
        &self,
        _company_id: Uuid,
        _channel_id: Option<Uuid>,
        _status: Option<OutboxStatus>,
        _sort_asc: bool,
        _offset: i64,
        _limit: i64,
    ) -> AppResult<Vec<OutboxEntry>> {
        Ok(Vec::new())
    }

    /// One outbox row by id. The caller checks its `company_id` before showing it — the id comes
    /// from a URL.
    async fn get_outbox_entry(&self, _outbox_id: Uuid) -> AppResult<Option<OutboxEntry>> {
        Ok(None)
    }

    /// Hand one email to the transport, exactly once per `idempotency_key`.
    ///
    /// `Some(outbox_id)` means this caller queued it; `None` means an equivalent send is already
    /// queued or delivered and this caller must not queue a second one. Delivery itself happens
    /// later, in the outbox poller, on whichever instance claims the row.
    ///
    /// The default accepts without recording, so test doubles are unaffected.
    async fn enqueue_outbound_send(&self, _send: OutboundSend) -> AppResult<Option<Uuid>> {
        Ok(Some(Uuid::new_v4()))
    }

    async fn get_outreach_thread_for_outbox(&self, _outbox_id: Uuid) -> AppResult<Option<Uuid>> {
        Ok(None)
    }

    async fn is_outbox_delivery_active(&self, _outbox_id: Uuid) -> AppResult<bool> {
        Ok(true)
    }

    async fn cancel_claimed_outbox(&self, _outbox_id: Uuid, _worker_id: Uuid) -> AppResult<bool> {
        Ok(false)
    }

    async fn enqueue_task(&self, new_task: NewTask) -> AppResult<BackgroundTask>;

    async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>>;

    /// Every execution attempt for one company-owned task, oldest first.
    ///
    /// The company predicate is part of the query because the task id originates in a browser
    /// route; authorization must scope the object being listed, not merely a preceding lookup.
    async fn list_task_attempts(
        &self,
        _company_id: Uuid,
        _task_id: Uuid,
    ) -> AppResult<Vec<TaskAttemptRecord>> {
        Ok(Vec::new())
    }

    async fn get_task_by_source_message_id(
        &self,
        _company_id: Uuid,
        _message_id: &str,
    ) -> AppResult<Option<BackgroundTask>> {
        Ok(None)
    }

    async fn update_task_payload(&self, id: Uuid, payload: Value) -> AppResult<()>;

    /// Commit one dispatch's entire visible effect, or none of it.
    ///
    /// No default: a double that silently reported success would let the dispatch believe it had
    /// delivered a reply it never queued.
    async fn commit_agent_dispatch(
        &self,
        commit: AgentDispatchCommit<'_>,
    ) -> AppResult<DispatchCommit>;

    /// Extend this run's lease. `false` means the run no longer owns the task and must stop.
    ///
    /// No default, for the same reason: a double that always renews cannot exercise lease loss,
    /// which is the behaviour every caller of this branches on.
    async fn renew_task_lease(
        &self,
        lease: TaskLeaseRef,
        lock_expires_at: DateTime<Utc>,
    ) -> AppResult<bool>;

    /// Open a `task_attempts` row for a run that is starting, so its duration and token cost have
    /// somewhere to land.
    ///
    /// Defaulted to a no-op for the same reason as [`Self::renew_task_lease`]: the hand-written
    /// mocks across the suite assert on queue transitions, and this ledger is not one.
    async fn begin_task_attempt(&self, attempt: TaskAttemptRef) -> AppResult<()> {
        let _ = attempt;
        Ok(())
    }

    /// Close the row opened by [`Self::begin_task_attempt`]. `false` means the row was not still
    /// open — another worker took the task over — and the caller's numbers are not the ones of
    /// record.
    async fn finish_task_attempt(&self, outcome: &TaskAttemptOutcome) -> AppResult<bool> {
        let _ = outcome;
        Ok(true)
    }

    async fn claim_pending_tasks(
        &self,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>>;

    async fn claim_task(
        &self,
        id: Uuid,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
    ) -> AppResult<bool>;

    async fn mark_task_completed(&self, lease: TaskLeaseRef) -> AppResult<bool>;

    async fn mark_task_failed(
        &self,
        lease: TaskLeaseRef,
        error_msg: &str,
        next_run_at: DateTime<Utc>,
        is_dead_letter: bool,
    ) -> AppResult<bool>;

    /// The same fenced failure transition with its bounded operational reason attached to the
    /// status event. Older in-memory doubles may keep implementing [`Self::mark_task_failed`].
    async fn mark_task_failed_with_reason(
        &self,
        lease: TaskLeaseRef,
        error_msg: &str,
        next_run_at: DateTime<Utc>,
        is_dead_letter: bool,
        _reason: TaskStopReason,
    ) -> AppResult<bool> {
        self.mark_task_failed(lease, error_msg, next_run_at, is_dead_letter)
            .await
    }

    async fn stop_task(&self, id: Uuid) -> AppResult<BackgroundTask>;

    async fn resume_task(&self, id: Uuid) -> AppResult<BackgroundTask>;

    /// Operator-attributed variants used by authenticated UI mutations.
    async fn stop_task_as(&self, id: Uuid, _operator_id: Uuid) -> AppResult<BackgroundTask> {
        self.stop_task(id).await
    }

    async fn resume_task_as(&self, id: Uuid, _operator_id: Uuid) -> AppResult<BackgroundTask> {
        self.resume_task(id).await
    }

    async fn resume_task_after_approval(
        &self,
        id: Uuid,
        _approval_id: Uuid,
    ) -> AppResult<BackgroundTask> {
        self.resume_task(id).await
    }

    /// The six-column correlation-chain read model. Defaults keep narrow test doubles small.
    async fn list_task_chain_board(
        &self,
        _company_id: Uuid,
        filter: TaskBoardFilter,
    ) -> AppResult<TaskChainBoard> {
        Ok(TaskChainBoard {
            cards: HashMap::new(),
            totals: HashMap::new(),
            per_column_limit: filter.per_column_limit,
        })
    }

    async fn get_task_chain_detail(
        &self,
        _company_id: Uuid,
        _correlation_id: CorrelationId,
    ) -> AppResult<Option<TaskChainDetail>> {
        Ok(None)
    }

    async fn list_task_status_events(
        &self,
        _company_id: Uuid,
        _correlation_id: CorrelationId,
        _cursor: Option<TaskStatusEventCursor>,
        _limit: usize,
    ) -> AppResult<Vec<TaskStatusEvent>> {
        Ok(Vec::new())
    }

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>>;

    async fn list_company_tasks_page(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let tasks = self
            .list_company_tasks(company_id, channel_id, status, sort_asc)
            .await?;
        Ok(tasks
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
            .collect())
    }
}

/// One row of the per-thread activity lookup.
#[derive(sqlx::FromRow)]
struct ThreadActivityDb {
    thread_id: Uuid,
    status: String,
    lock_expires_at: Option<DateTime<Utc>>,
}

#[async_trait]
impl TaskPersistence for PostgresPersistence {
    async fn list_thread_activity(
        &self,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadActivity>> {
        if thread_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // `DISTINCT ON` keeps one task per thread: the run still going if there is one, and
        // otherwise whichever of the thread's finished runs ended last.
        //
        // `completed` earns its place in that second group even though it draws no badge. A
        // dead letter is the thread's last word only until a later run answers the question, and
        // asking again is exactly what a reader does with a failure: without the successful run
        // in the comparison, the alert would come back the moment the retry finished and the
        // thread would look broken for good. Ordering by the clock alone cannot express that,
        // hence the boolean: unfinished work outranks any finished run however old it is.
        //
        // `stopped` and `failed` stay out. Neither is an answer, so neither should bury one.
        let rows = sqlx::query_as::<_, ThreadActivityDb>(
            r#"SELECT DISTINCT ON (thread_id) thread_id, status, lock_expires_at
               FROM background_tasks
               WHERE thread_id = ANY($1)
                 AND status IN ('pending', 'processing', 'pending_approval',
                                'waiting_for_third_party_reply', 'dead_letter', 'completed')
               ORDER BY thread_id,
                        status IN ('dead_letter', 'completed'),
                        updated_at DESC, id DESC"#,
        )
        .bind(thread_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        let now = Utc::now();
        rows.into_iter()
            .map(|row| {
                let status = TaskStatus::from_str(&row.status)
                    .map_err(|error| AppError::Internal(error.to_string()))?;
                Ok((
                    row.thread_id,
                    ThreadActivity::from_task(status, row.lock_expires_at, now),
                ))
            })
            // `completed` is queried for its position in that ordering, not for a badge: reaching
            // here it means the thread's last word was a run that worked, which is nothing to
            // show. Dropping every `None` also keeps a status added to the query later from
            // turning into a badge nobody chose.
            .filter_map(|entry: AppResult<_>| match entry {
                Ok((thread_id, Some(activity))) => Some(Ok((thread_id, activity))),
                Ok((_, None)) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    async fn create_outreach_and_pause(
        &self,
        request: CreateOutreachRequest,
    ) -> AppResult<OutreachProgress> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let outreach = sqlx::query_as::<_, OutreachDb>(
            r#"INSERT INTO task_outreaches (
                    id, task_id, outreach_key, status, required_threshold_percent,
                    expires_at, subject, body
               )
               SELECT $1, id, $2, 'waiting', $3, $4, $5, $6
               FROM background_tasks
               WHERE id = $7 AND company_id = $8
                 AND status = 'processing' AND worker_id = $9
                 AND lock_expires_at > CURRENT_TIMESTAMP
               ON CONFLICT (task_id, outreach_key) DO UPDATE
                   SET outreach_key = EXCLUDED.outreach_key
               RETURNING id, task_id, status,
                         required_threshold_percent::double precision AS required_threshold_percent,
                         expires_at"#,
        )
        .bind(request.id)
        .bind(&request.outreach_key)
        .bind(request.required_threshold_percent)
        .bind(request.expires_at)
        .bind(&request.subject)
        .bind(&request.body)
        .bind(request.task_id)
        .bind(request.company_id)
        .bind(request.worker_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Internal("Outreach task lease was lost before creation".into()))?;

        let created = outreach.id == request.id;
        if created {
            for (position, target) in request.targets.iter().enumerate() {
                sqlx::query(
                    r#"INSERT INTO email_outbox (
                            id, company_id, channel_id, task_id, correlation_id,
                            idempotency_key, payload
                       ) VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                )
                .bind(target.outbox_id)
                .bind(request.company_id)
                .bind(request.channel_id)
                .bind(request.task_id)
                .bind(request.correlation_id.as_uuid())
                .bind(format!("outreach:{}:target:{}", outreach.id, position))
                .bind(&target.outbox_payload)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;

                sqlx::query(
                    r#"INSERT INTO task_outreach_targets (outreach_id, email, outbox_id)
                       VALUES ($1, $2, $3)"#,
                )
                .bind(outreach.id)
                .bind(target.email.as_str())
                .bind(target.outbox_id)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
            }
        }

        let status = OutreachStatus::from_str(&outreach.status)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let suspended = status == OutreachStatus::Waiting;
        if suspended {
            set_task_transition_context(
                &mut tx,
                TaskTransitionReason::OutreachStarted,
                TaskTransitionActorKind::Outreach,
                None,
            )
            .await?;
            set_task_transition_source(&mut tx, None, Some(outreach.id)).await?;
            let paused = sqlx::query(
                r#"UPDATE background_tasks
                   SET status = 'waiting_for_third_party_reply', wait_expires_at = $1,
                       worker_id = NULL, execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL,
                       updated_at = CURRENT_TIMESTAMP
                   WHERE id = $2 AND company_id = $3
                     AND status = 'processing' AND worker_id = $4
                     AND lock_expires_at > CURRENT_TIMESTAMP"#,
            )
            .bind(outreach.expires_at)
            .bind(request.task_id)
            .bind(request.company_id)
            .bind(request.worker_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            if paused.rows_affected() != 1 {
                return Err(AppError::Internal(
                    "Outreach task could not be paused".into(),
                ));
            }
        }

        let (target_count, response_count): (i64, i64) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint,
                      COUNT(*) FILTER (WHERE responded_at IS NOT NULL)::bigint
               FROM task_outreach_targets WHERE outreach_id = $1"#,
        )
        .bind(outreach.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;

        Ok(outreach_progress(
            &outreach,
            status,
            target_count,
            response_count,
            suspended,
        ))
    }

    async fn find_correlated_outreach_reply(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Uuid,
        sender: &str,
        references: &[MessageId],
    ) -> AppResult<Option<OutreachReplyMatch>> {
        if references.is_empty() {
            return Ok(None);
        }
        let reference_strs: Vec<&str> = references.iter().map(MessageId::as_str).collect();
        let row = sqlx::query_as::<_, (Uuid, Uuid, String)>(
            r#"SELECT outreach.id, task.id, target.email::text
               FROM task_outreaches outreach
               JOIN background_tasks task ON task.id = outreach.task_id
               JOIN task_outreach_targets target ON target.outreach_id = outreach.id
               JOIN email_outbox outbox ON outbox.id = target.outbox_id
               WHERE task.company_id = $1 AND task.channel_id = $2 AND task.thread_id = $3
                 AND target.email = $4
                  AND outreach.status IN (
                      'waiting', 'timeout_pending_approval', 'threshold_met', 'completed'
                  )
                 AND outbox.status = 'sent'
                 AND outbox.provider_message_id = ANY($5)
               ORDER BY outreach.created_at DESC
               LIMIT 1"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(thread_id)
        .bind(sender.trim())
        .bind(&reference_strs)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(
            row.map(|(outreach_id, task_id, target_email)| OutreachReplyMatch {
                outreach_id,
                task_id,
                target_email: target_email.into(),
            }),
        )
    }

    async fn record_outreach_reply(
        &self,
        matched: &OutreachReplyMatch,
        response_message_id: Uuid,
    ) -> AppResult<OutreachProgress> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let mut outreach = sqlx::query_as::<_, OutreachDb>(
            r#"SELECT id, task_id, status,
                      required_threshold_percent::double precision AS required_threshold_percent,
                      expires_at
               FROM task_outreaches WHERE id = $1 FOR UPDATE"#,
        )
        .bind(matched.outreach_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;

        sqlx::query(
            r#"UPDATE task_outreach_targets
               SET responded_at = CURRENT_TIMESTAMP, response_message_id = $3
               WHERE outreach_id = $1 AND email = $2 AND responded_at IS NULL"#,
        )
        .bind(matched.outreach_id)
        .bind(matched.target_email.as_str())
        .bind(response_message_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        let (target_count, response_count): (i64, i64) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint,
                      COUNT(*) FILTER (WHERE responded_at IS NOT NULL)::bigint
               FROM task_outreach_targets WHERE outreach_id = $1"#,
        )
        .bind(matched.outreach_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let required = required_response_count(target_count, outreach.required_threshold_percent);
        let current_status = OutreachStatus::from_str(&outreach.status)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let reached = response_count >= required as i64;

        if reached
            && matches!(
                current_status,
                OutreachStatus::Waiting | OutreachStatus::TimeoutPendingApproval
            )
        {
            set_task_transition_context(
                &mut tx,
                TaskTransitionReason::OutreachReplyReceived,
                TaskTransitionActorKind::Outreach,
                None,
            )
            .await?;
            set_task_transition_source(&mut tx, None, Some(outreach.id)).await?;
            sqlx::query(
                r#"UPDATE task_outreaches SET status = 'threshold_met',
                       updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            )
            .bind(matched.outreach_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            sqlx::query(
                r#"UPDATE background_tasks SET status = 'pending', run_at = CURRENT_TIMESTAMP,
                       wait_expires_at = NULL, worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                       lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
                   WHERE id = $1 AND status IN (
                       'waiting_for_third_party_reply', 'pending_approval'
                   )"#,
            )
            .bind(outreach.task_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            sqlx::query(
                r#"UPDATE human_approvals SET status = 'expired', updated_at = CURRENT_TIMESTAMP
                   WHERE task_id = $1 AND action_type = 'quorum_timeout'
                     AND status = 'pending'"#,
            )
            .bind(outreach.task_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            outreach.status = OutreachStatus::ThresholdMet.as_str().to_string();
        }
        tx.commit().await.map_err(AppError::from)?;

        let status = OutreachStatus::from_str(&outreach.status)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        Ok(outreach_progress(
            &outreach,
            status,
            target_count,
            response_count,
            false,
        ))
    }

    async fn list_due_outreaches(
        &self,
        due_at: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<DueOutreach>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                Uuid,
                Uuid,
                Option<Uuid>,
                f64,
                i64,
                i64,
                DateTime<Utc>,
            ),
        >(
            r#"SELECT outreach.id, task.id, task.company_id, task.channel_id, task.thread_id,
                      outreach.required_threshold_percent::double precision,
                      COUNT(target.*)::bigint,
                      COUNT(target.*) FILTER (WHERE target.responded_at IS NOT NULL)::bigint,
                      outreach.expires_at
               FROM task_outreaches outreach
               JOIN background_tasks task ON task.id = outreach.task_id
               JOIN task_outreach_targets target ON target.outreach_id = outreach.id
               WHERE outreach.status = 'waiting' AND outreach.expires_at <= $1
                 AND task.status = 'waiting_for_third_party_reply'
               GROUP BY outreach.id, task.id
               ORDER BY outreach.expires_at, outreach.id
               LIMIT $2"#,
        )
        .bind(due_at)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    outreach_id,
                    task_id,
                    company_id,
                    channel_id,
                    thread_id,
                    required_threshold_percent,
                    target_count,
                    response_count,
                    expires_at,
                )| DueOutreach {
                    outreach_id,
                    task_id,
                    company_id,
                    channel_id,
                    thread_id,
                    required_threshold_percent,
                    target_count: target_count as usize,
                    response_count: response_count as usize,
                    expires_at,
                },
            )
            .collect())
    }

    async fn mark_outreach_timeout_pending(&self, outreach_id: Uuid) -> AppResult<bool> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let task_id = sqlx::query_scalar::<_, Uuid>(
            r#"UPDATE task_outreaches
               SET status = 'timeout_pending_approval', updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'waiting' AND expires_at <= CURRENT_TIMESTAMP
               RETURNING task_id"#,
        )
        .bind(outreach_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let Some(task_id) = task_id else {
            tx.rollback().await.map_err(AppError::from)?;
            return Ok(false);
        };
        set_task_transition_context(
            &mut tx,
            TaskTransitionReason::OutreachTimedOut,
            TaskTransitionActorKind::Outreach,
            None,
        )
        .await?;
        set_task_transition_source(&mut tx, None, Some(outreach_id)).await?;
        let updated = sqlx::query(
            r#"UPDATE background_tasks
               SET status = 'pending_approval', wait_expires_at = NULL,
                   worker_id = NULL, execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'waiting_for_third_party_reply'"#,
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(updated.rows_affected() == 1)
    }

    async fn restore_outreach_waiting(&self, outreach_id: Uuid) -> AppResult<()> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let row = sqlx::query_as::<_, (Uuid, DateTime<Utc>)>(
            r#"UPDATE task_outreaches SET status = 'waiting', updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'timeout_pending_approval'
               RETURNING task_id, expires_at"#,
        )
        .bind(outreach_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if let Some((task_id, expires_at)) = row {
            set_task_transition_context(
                &mut tx,
                TaskTransitionReason::OutreachExtended,
                TaskTransitionActorKind::Outreach,
                None,
            )
            .await?;
            set_task_transition_source(&mut tx, None, Some(outreach_id)).await?;
            sqlx::query(
                r#"UPDATE background_tasks
                   SET status = 'waiting_for_third_party_reply', wait_expires_at = $2,
                       updated_at = CURRENT_TIMESTAMP
                   WHERE id = $1 AND status = 'pending_approval'"#,
            )
            .bind(task_id)
            .bind(expires_at)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok(())
    }

    async fn get_outreach_context(&self, task_id: Uuid) -> AppResult<Option<String>> {
        let rows = sqlx::query_as::<_, (String, String, String, f64, String, bool)>(
            r#"SELECT outreach.subject, outreach.body, outreach.status,
                      outreach.required_threshold_percent::double precision,
                      target.email::text, target.responded_at IS NOT NULL
               FROM task_outreaches outreach
               JOIN task_outreach_targets target ON target.outreach_id = outreach.id
               WHERE outreach.id = (
                   SELECT id FROM task_outreaches
                   WHERE task_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1
               )
               ORDER BY outreach.created_at DESC, target.email"#,
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        let Some((subject, body, status, threshold, _, _)) = rows.first() else {
            return Ok(None);
        };
        let responded = rows
            .iter()
            .filter(|row| row.5)
            .map(|row| row.4.clone())
            .collect::<Vec<_>>();
        let outstanding = rows
            .iter()
            .filter(|row| !row.5)
            .map(|row| row.4.clone())
            .collect::<Vec<_>>();
        Ok(Some(format!(
            "Outreach status: {status}\nSubject: {subject}\nRequest: {body}\nRequired threshold: {threshold:.1}%\nResponses: {}/{}\nRespondents: {}\nOutstanding: {}",
            responded.len(),
            rows.len(),
            if responded.is_empty() {
                "none".to_string()
            } else {
                responded.join(", ")
            },
            if outstanding.is_empty() {
                "none".to_string()
            } else {
                outstanding.join(", ")
            },
        )))
    }

    async fn complete_outreach(&self, task_id: Uuid) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE task_outreaches SET status = 'completed', updated_at = CURRENT_TIMESTAMP
               WHERE task_id = $1 AND status IN ('threshold_met', 'proceed_partial')"#,
        )
        .bind(task_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(())
    }

    async fn claim_outbox_emails(
        &self,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<OutboxEmail>> {
        // Only `pending` rows: an expired `sending` lease is reaped into `pending` first, by
        // `reap_expired_outbox_leases`, so that redelivery is a counted attempt.
        sqlx::query_as::<_, OutboxEmail>(
            r#"WITH claimable AS (
                   SELECT id FROM email_outbox
                   WHERE status = 'pending' AND available_at <= CURRENT_TIMESTAMP
                   ORDER BY available_at, id
                   FOR UPDATE SKIP LOCKED
                   LIMIT $1
               )
               UPDATE email_outbox outbox
               SET status = 'sending', worker_id = $2, locked_at = CURRENT_TIMESTAMP,
                   lock_expires_at = $3, updated_at = CURRENT_TIMESTAMP
               FROM claimable
               WHERE outbox.id = claimable.id
               RETURNING outbox.id, outbox.payload, outbox.idempotency_key"#,
        )
        .bind(limit)
        .bind(worker_id)
        .bind(lock_expires_at)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)
    }

    async fn list_task_deliveries(&self, task_id: Uuid) -> AppResult<Vec<OutboxEntry>> {
        let db_list = sqlx::query_as::<_, OutboxEntryDb>(&format!(
            "SELECT {OUTBOX_COLUMNS} FROM email_outbox WHERE task_id = $1 ORDER BY created_at, id"
        ))
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(OutboxEntry::try_from).collect()
    }

    async fn enqueue_outbound_send(&self, send: OutboundSend) -> AppResult<Option<Uuid>> {
        // The unique index on `idempotency_key` is the lock: whoever inserts first owns this send.
        // The row lands 'pending' with no worker or lease — the outbox poller claims it, so a
        // caller that dies right after queueing still gets its email delivered.
        let row: Option<(Uuid,)> = sqlx::query_as(
            r#"INSERT INTO email_outbox (
                    id, company_id, channel_id, task_id, correlation_id,
                    idempotency_key, payload
               ) VALUES ($1, $2, $3, $4, $5, $6, $7)
               ON CONFLICT (idempotency_key) DO NOTHING
               RETURNING id"#,
        )
        .bind(Uuid::new_v4())
        .bind(send.company_id)
        .bind(send.channel_id)
        .bind(send.task_id)
        .bind(send.correlation_id.as_uuid())
        .bind(&send.idempotency_key)
        .bind(&send.payload)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(row.map(|(id,)| id))
    }

    async fn mark_outbox_email_sent(
        &self,
        id: Uuid,
        worker_id: Uuid,
        provider_message_id: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE email_outbox
               SET status = 'sent', provider_message_id = $3, sent_at = CURRENT_TIMESTAMP,
                   worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'sending' AND worker_id = $2"#,
        )
        .bind(id)
        .bind(worker_id)
        .bind(provider_message_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn mark_outbox_email_failed(
        &self,
        id: Uuid,
        worker_id: Uuid,
        error: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(&format!(
            "UPDATE email_outbox {}
             WHERE id = $1 AND status = 'sending' AND worker_id = $2",
            outbox_attempt_failed_set("$3")
        ))
        .bind(id)
        .bind(worker_id)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn mark_outbox_email_dead(
        &self,
        id: Uuid,
        worker_id: Uuid,
        error: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE email_outbox
               SET status = 'failed', retry_count = retry_count + 1, last_error = $3,
                   worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'sending' AND worker_id = $2"#,
        )
        .bind(id)
        .bind(worker_id)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn census_stuck_work(
        &self,
        thresholds: StuckWorkThresholds,
    ) -> AppResult<StuckWorkCensus> {
        // Counted in one pass with FILTER rather than seven statements. Each arm is a bounded
        // index scan: the status arms hit `background_tasks_company_status_created_idx` and
        // `email_outbox_pending_idx`, and the lease arm hits
        // `background_tasks_processing_lease_idx`.
        //
        // `wait_expires_at` is what the reply arm compares against rather than the parked
        // threshold: an outreach states its own deadline, and a task waiting inside the window it
        // asked for is not stuck. The threshold is the fallback for a parked row that named no
        // deadline at all.
        let row: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            r#"WITH tasks AS (
                   SELECT
                       count(*) FILTER (WHERE status = 'dead_letter') AS dead_lettered,
                       count(*) FILTER (
                           WHERE status = 'pending'
                             AND run_at < CURRENT_TIMESTAMP - $1::interval
                       ) AS queue_overdue,
                       count(*) FILTER (
                           WHERE status = 'processing'
                             AND lock_expires_at < CURRENT_TIMESTAMP
                       ) AS lease_expired,
                       count(*) FILTER (
                           WHERE status = 'pending_approval'
                             AND updated_at < CURRENT_TIMESTAMP - $2::interval
                       ) AS approval_overdue,
                       count(*) FILTER (
                           WHERE status = 'waiting_for_third_party_reply'
                             AND COALESCE(
                                     wait_expires_at,
                                     updated_at + $2::interval
                                 ) < CURRENT_TIMESTAMP
                       ) AS reply_overdue
                   FROM background_tasks
               ),
               outbox AS (
                   SELECT
                       count(*) FILTER (WHERE status = 'failed') AS outbox_failed,
                       count(*) FILTER (
                           WHERE status = 'pending'
                             AND available_at < CURRENT_TIMESTAMP - $1::interval
                       ) AS outbox_overdue
                   FROM email_outbox
               )
               SELECT tasks.dead_lettered, tasks.queue_overdue, tasks.lease_expired,
                      tasks.approval_overdue, tasks.reply_overdue,
                      outbox.outbox_failed, outbox.outbox_overdue
               FROM tasks, outbox"#,
        )
        .bind(
            PgInterval::try_from(thresholds.queue_overdue_after()).map_err(|error| {
                AppError::Internal(format!("Invalid queue-overdue threshold: {error}"))
            })?,
        )
        .bind(
            PgInterval::try_from(thresholds.parked_overdue_after()).map_err(|error| {
                AppError::Internal(format!("Invalid parked-overdue threshold: {error}"))
            })?,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(StuckWorkCensus {
            dead_lettered: row.0,
            queue_overdue: row.1,
            lease_expired: row.2,
            approval_overdue: row.3,
            reply_overdue: row.4,
            outbox_failed: row.5,
            outbox_overdue: row.6,
        })
    }

    async fn reap_expired_task_leases(&self) -> AppResult<u64> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;

        // Hits `background_tasks_processing_lease_idx`. No worker guard: the lease is expired, so
        // by definition no run still holds it.
        let reaped = sqlx::query_as::<_, (Uuid, i32)>(
            r#"UPDATE background_tasks
               SET retry_count = retry_count + 1,
                   status = CASE
                       WHEN retry_count + 1 >= max_retries THEN 'dead_letter'
                       ELSE 'pending'
                   END,
                   last_error = $1,
                   -- The same exponential backoff a reported failure gets, with the exponent
                   -- capped so it cannot run away: 30s * 2^attempt.
                   run_at = CURRENT_TIMESTAMP
                       + (30 * POWER(2, LEAST(retry_count + 1, 10))) * INTERVAL '1 second',
                   worker_id = NULL,
                   execution_generation = NULL,
                   locked_at = NULL,
                   lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE status = 'processing'
                 AND (lock_expires_at IS NULL OR lock_expires_at <= CURRENT_TIMESTAMP)
               RETURNING id, retry_count"#,
        )
        .bind(LEASE_EXPIRED_ERROR)
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::from)?;

        // Close each reaped run's ledger row. `retry_count` was just incremented, and the attempt
        // that vanished was numbered with the value it now holds -- attempt N is the run made
        // after N-1 failures.
        for (task_id, retry_count) in &reaped {
            sqlx::query(
                r#"UPDATE task_attempts
                   SET status = $3,
                       error = $4,
                       stop_reason = $5,
                       finished_at = CURRENT_TIMESTAMP
                   WHERE task_id = $1 AND attempt_number = $2 AND status = 'processing'"#,
            )
            .bind(task_id)
            .bind(retry_count)
            .bind(TaskAttemptStatus::Failed.as_str())
            .bind(LEASE_EXPIRED_ERROR)
            .bind(TaskStopReason::LeaseLost.as_str())
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }

        tx.commit().await.map_err(AppError::from)?;
        Ok(reaped.len() as u64)
    }

    async fn reap_expired_outbox_leases(&self) -> AppResult<u64> {
        // Hits `email_outbox_sending_lease_idx`. No worker guard: the lease is expired, so by
        // definition no worker still holds a claim on the row.
        let result = sqlx::query(&format!(
            "UPDATE email_outbox {}
             WHERE status = 'sending'
               AND (worker_id IS NULL OR locked_at IS NULL OR lock_expires_at IS NULL
                    OR lock_expires_at <= locked_at
                    OR lock_expires_at <= CURRENT_TIMESTAMP)",
            outbox_attempt_failed_set("'Delivery lease expired without a result'")
        ))
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected())
    }

    async fn get_outreach_thread_for_outbox(&self, outbox_id: Uuid) -> AppResult<Option<Uuid>> {
        sqlx::query_scalar(
            r#"SELECT task.thread_id
               FROM task_outreach_targets target
               JOIN task_outreaches outreach ON outreach.id = target.outreach_id
               JOIN background_tasks task ON task.id = outreach.task_id
               WHERE target.outbox_id = $1"#,
        )
        .bind(outbox_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)
        .map(Option::flatten)
    }

    async fn is_outbox_delivery_active(&self, outbox_id: Uuid) -> AppResult<bool> {
        sqlx::query_scalar::<_, bool>(
            r#"SELECT CASE
                   WHEN target.outbox_id IS NULL THEN true
                   ELSE outreach.status = 'waiting'
               END
               FROM email_outbox outbox
               LEFT JOIN task_outreach_targets target ON target.outbox_id = outbox.id
               LEFT JOIN task_outreaches outreach ON outreach.id = target.outreach_id
               WHERE outbox.id = $1"#,
        )
        .bind(outbox_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)
        .map(|active| active.unwrap_or(false))
    }

    async fn cancel_claimed_outbox(&self, outbox_id: Uuid, worker_id: Uuid) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE email_outbox SET status = 'failed', last_error = 'Outreach closed',
                   worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'sending' AND worker_id = $2"#,
        )
        .bind(outbox_id)
        .bind(worker_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn enqueue_task(&self, new_task: NewTask) -> AppResult<BackgroundTask> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let task = insert_task(&mut tx, new_task).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(task)
    }

    async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>> {
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, correlation_id, task_type, status, payload,
                       retry_count, max_retries, last_error, worker_id, execution_generation, locked_at, lock_expires_at,
                       run_at, created_at, updated_at
               FROM background_tasks WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        match db {
            Some(d) => Ok(Some(d.try_into()?)),
            None => Ok(None),
        }
    }

    async fn list_task_attempts(
        &self,
        company_id: Uuid,
        task_id: Uuid,
    ) -> AppResult<Vec<TaskAttemptRecord>> {
        let rows = sqlx::query_as::<_, TaskAttemptRecordDb>(
            r#"SELECT attempt.attempt_number, attempt.status, attempt.error,
                      attempt.stop_reason, attempt.prompt_tokens, attempt.completion_tokens,
                      attempt.result, attempt.started_at, attempt.finished_at,
                      attempt.execution_generation
               FROM task_attempts AS attempt
               JOIN background_tasks AS task ON task.id = attempt.task_id
               WHERE task.company_id = $1 AND attempt.task_id = $2
               ORDER BY attempt.attempt_number"#,
        )
        .bind(company_id)
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    /// The board's six columns, projected fresh on every render.
    ///
    /// `eligible` selects at row level — "unfinished, or touched since the cutoff" — before any
    /// aggregate runs. That matters because every predicate in `staged` is an aggregate over a
    /// `GROUP BY correlation_id` that Postgres cannot push below its own grouping, so on its own it
    /// prunes only *after* scanning every task the company has ever run.
    async fn list_task_chain_board(
        &self,
        company_id: Uuid,
        filter: TaskBoardFilter,
    ) -> AppResult<TaskChainBoard> {
        let started = Instant::now();
        let rows = sqlx::query_as::<_, TaskChainCardDb>(&BOARD_QUERY)
            .bind(company_id)
            .bind(filter.channel_id)
            .bind(filter.terminal_since)
            .bind(filter.per_column_limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;
        warn_if_board_projection_is_slow(company_id, started.elapsed(), &rows);

        let mut board = TaskChainBoard {
            cards: ChainStage::ALL
                .into_iter()
                .map(|stage| (stage, Vec::new()))
                .collect(),
            totals: ChainStage::ALL
                .into_iter()
                .map(|stage| (stage, 0))
                .collect(),
            per_column_limit: filter.per_column_limit,
        };
        for row in rows {
            let (card, total) = row.try_into()?;
            board.totals.insert(card.stage, total);
            board.cards.entry(card.stage).or_default().push(card);
        }
        Ok(board)
    }

    async fn list_task_status_events(
        &self,
        company_id: Uuid,
        correlation_id: CorrelationId,
        cursor: Option<TaskStatusEventCursor>,
        limit: usize,
    ) -> AppResult<Vec<TaskStatusEvent>> {
        let rows = chain_status_events_query(
            company_id,
            correlation_id,
            cursor,
            limit.clamp(1, MAX_STATUS_EVENT_PAGE) as i64,
        )
        .build_query_as::<TaskStatusEventDb>()
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    /// One chain, whole, in a fixed number of queries.
    ///
    /// Every collection is read in one batched, bounded statement rather than per task: the
    /// per-task loop this replaced cost roughly `1 + 1 + 2n + 3` round trips, so a 200-task chain
    /// spent ~405 sequential trips to the database on a pane a viewer opens live.
    async fn get_task_chain_detail(
        &self,
        company_id: Uuid,
        correlation_id: CorrelationId,
    ) -> AppResult<Option<TaskChainDetail>> {
        let header = sqlx::query_as::<_, (String, Vec<String>, Vec<String>)>(
            r#"SELECT
                   (array_agg(
                       COALESCE(NULLIF(thread.subject, ''), task.task_type)
                       ORDER BY task.created_at, task.id
                   ))[1] AS title,
                   array_agg(DISTINCT channel.name ORDER BY channel.name) AS channel_names,
                   COALESCE(
                       array_agg(DISTINCT agent.name ORDER BY agent.name)
                           FILTER (WHERE agent.id IS NOT NULL),
                       ARRAY[]::text[]
                   ) AS agent_names
               FROM background_tasks AS task
               JOIN channels AS channel
                 ON channel.company_id = task.company_id AND channel.id = task.channel_id
               LEFT JOIN threads AS thread
                 ON thread.company_id = task.company_id
                AND thread.channel_id = task.channel_id AND thread.id = task.thread_id
               LEFT JOIN channel_agents AS assignment
                 ON assignment.company_id = channel.company_id
                AND assignment.channel_id = channel.id
               LEFT JOIN agents AS agent ON agent.id = assignment.agent_id
               WHERE task.company_id = $1 AND task.correlation_id = $2
               GROUP BY task.correlation_id"#,
        )
        .bind(company_id)
        .bind(correlation_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        let Some((title, channel_names, agent_names)) = header else {
            return Ok(None);
        };

        let mut truncated = false;

        let mut task_rows = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, correlation_id, task_type, status,
                      payload, retry_count, max_retries, last_error, worker_id,
                      execution_generation, locked_at, lock_expires_at, run_at, created_at,
                      updated_at
               FROM background_tasks
               WHERE company_id = $1 AND correlation_id = $2
               ORDER BY created_at, id
               LIMIT $3"#,
        )
        .bind(company_id)
        .bind(correlation_id.as_uuid())
        .bind(probe_limit(CHAIN_DETAIL_MAX_TASKS))
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        truncated |= trim_to_limit(&mut task_rows, CHAIN_DETAIL_MAX_TASKS);
        let task_ids = task_rows.iter().map(|row| row.id).collect::<Vec<_>>();

        let mut attempt_rows = sqlx::query_as::<_, ChainAttemptDb>(
            r#"SELECT attempt.task_id, attempt.attempt_number, attempt.status, attempt.error,
                      attempt.stop_reason, attempt.prompt_tokens, attempt.completion_tokens,
                      attempt.result, attempt.started_at, attempt.finished_at,
                      attempt.execution_generation
               FROM task_attempts AS attempt
               JOIN background_tasks AS task ON task.id = attempt.task_id
               WHERE task.company_id = $1 AND attempt.task_id = ANY($2)
               ORDER BY attempt.task_id, attempt.attempt_number
               LIMIT $3"#,
        )
        .bind(company_id)
        .bind(&task_ids)
        .bind(probe_limit(CHAIN_DETAIL_MAX_ATTEMPTS))
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        truncated |= trim_to_limit(&mut attempt_rows, CHAIN_DETAIL_MAX_ATTEMPTS);
        let mut attempts = group_by_task(
            attempt_rows
                .into_iter()
                .map(|row| Ok((row.task_id, TaskAttemptRecord::try_from(row.record)?)))
                .collect::<AppResult<Vec<_>>>()?,
        );

        let mut delivery_rows = sqlx::query_as::<_, OutboxEntryDb>(&format!(
            "SELECT {OUTBOX_COLUMNS} FROM email_outbox
              WHERE company_id = $1 AND task_id = ANY($2)
              ORDER BY task_id, created_at, id
              LIMIT $3"
        ))
        .bind(company_id)
        .bind(&task_ids)
        .bind(probe_limit(CHAIN_DETAIL_MAX_DELIVERIES))
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        truncated |= trim_to_limit(&mut delivery_rows, CHAIN_DETAIL_MAX_DELIVERIES);
        let mut deliveries = group_by_task(
            delivery_rows
                .into_iter()
                .map(|row| {
                    // `task_id = ANY($2)` is what selected these rows, so the column cannot be
                    // null here; the entry carries it as an `Option` because an outbox row in
                    // general need not belong to a task.
                    let entry = OutboxEntry::try_from(row)?;
                    Ok(entry.task_id.map(|task_id| (task_id, entry)))
                })
                .collect::<AppResult<Vec<_>>>()?
                .into_iter()
                .flatten(),
        );

        let tasks = task_rows
            .into_iter()
            .map(|row| {
                let task: BackgroundTask = row.try_into()?;
                Ok(TaskChainTaskDetail {
                    attempts: attempts.remove(&task.id).unwrap_or_default(),
                    deliveries: deliveries.remove(&task.id).unwrap_or_default(),
                    task,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;

        let mut event_rows = chain_status_events_query(
            company_id,
            correlation_id,
            None,
            probe_limit(CHAIN_DETAIL_MAX_EVENTS),
        )
        .build_query_as::<TaskStatusEventDb>()
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        truncated |= trim_to_limit(&mut event_rows, CHAIN_DETAIL_MAX_EVENTS);
        let events = event_rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<AppResult<Vec<_>>>()?;

        let mut approvals =
            sqlx::query_as::<_, (Uuid, Uuid, String, String, DateTime<Utc>, DateTime<Utc>)>(
                r#"SELECT approval.id, approval.task_id, approval.status, approval.action_title,
                      approval.created_at, approval.updated_at
               FROM human_approvals AS approval
               JOIN background_tasks AS task ON task.id = approval.task_id
               WHERE task.company_id = $1 AND task.correlation_id = $2
               ORDER BY approval.created_at, approval.id
               LIMIT $3"#,
            )
            .bind(company_id)
            .bind(correlation_id.as_uuid())
            .bind(probe_limit(CHAIN_DETAIL_MAX_APPROVALS))
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(|row| TaskApprovalContext {
                id: row.0,
                task_id: row.1,
                status: row.2,
                action_title: row.3,
                created_at: row.4,
                updated_at: row.5,
            })
            .collect::<Vec<_>>();
        truncated |= trim_to_limit(&mut approvals, CHAIN_DETAIL_MAX_APPROVALS);

        let mut outreaches = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                String,
                f64,
                i64,
                i64,
                DateTime<Utc>,
                DateTime<Utc>,
            ),
        >(
            r#"SELECT outreach.id, outreach.task_id, outreach.status,
                      outreach.required_threshold_percent::double precision,
                      COUNT(target.*)::bigint,
                      COUNT(target.*) FILTER (WHERE target.responded_at IS NOT NULL)::bigint,
                      outreach.expires_at, outreach.created_at
               FROM task_outreaches AS outreach
               JOIN background_tasks AS task ON task.id = outreach.task_id
               LEFT JOIN task_outreach_targets AS target ON target.outreach_id = outreach.id
               WHERE task.company_id = $1 AND task.correlation_id = $2
               GROUP BY outreach.id
               ORDER BY outreach.created_at, outreach.id
               LIMIT $3"#,
        )
        .bind(company_id)
        .bind(correlation_id.as_uuid())
        .bind(probe_limit(CHAIN_DETAIL_MAX_OUTREACHES))
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?
        .into_iter()
        .map(|row| TaskOutreachContext {
            id: row.0,
            task_id: row.1,
            status: row.2,
            required_threshold_percent: row.3,
            target_count: row.4,
            response_count: row.5,
            expires_at: row.6,
            created_at: row.7,
        })
        .collect::<Vec<_>>();
        truncated |= trim_to_limit(&mut outreaches, CHAIN_DETAIL_MAX_OUTREACHES);

        Ok(Some(TaskChainDetail {
            company_id,
            correlation_id,
            title,
            channel_names,
            agent_names,
            tasks,
            events,
            approvals,
            outreaches,
            truncated,
        }))
    }

    async fn get_task_by_source_message_id(
        &self,
        company_id: Uuid,
        message_id: &str,
    ) -> AppResult<Option<BackgroundTask>> {
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, correlation_id, task_type, status, payload,
                      retry_count, max_retries, last_error, worker_id, execution_generation, locked_at, lock_expires_at,
                      run_at, created_at, updated_at
               FROM background_tasks
               WHERE company_id = $1 AND source_message_id = $2"#,
        )
        .bind(company_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        db.map(TryInto::try_into).transpose()
    }

    async fn update_task_payload(&self, id: Uuid, payload: Value) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE background_tasks
               SET payload = $1, updated_at = CURRENT_TIMESTAMP
               WHERE id = $2"#,
        )
        .bind(payload)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
    }

    async fn commit_agent_dispatch(
        &self,
        commit: AgentDispatchCommit<'_>,
    ) -> AppResult<DispatchCommit> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;

        // The fence goes first. If this run no longer owns the task the transaction rolls back
        // having written nothing, rather than queueing an email for work someone else has taken
        // over. Every other write below is unguarded precisely because this one guards them all.
        let fenced = sqlx::query(
            r#"UPDATE background_tasks
               SET payload = $1, updated_at = CURRENT_TIMESTAMP
               WHERE id = $2 AND status = 'processing' AND worker_id = $3
                  AND execution_generation = $4
                  AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(commit.payload)
        .bind(commit.lease.task_id)
        .bind(commit.lease.worker_id)
        .bind(commit.lease.execution_generation)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if fenced.rows_affected() != 1 {
            return Ok(DispatchCommit::LeaseLost);
        }

        for message in commit.messages {
            super::thread::insert_message_on(&mut tx, message).await?;
        }

        let outbox_id = match commit.outbound {
            Some(send) => {
                // The unique index on `idempotency_key` is the lock: whoever inserts first owns
                // this send. `None` means an equivalent send is already queued.
                let row: Option<(Uuid,)> = sqlx::query_as(
                    r#"INSERT INTO email_outbox (
                            id, company_id, channel_id, task_id, correlation_id,
                            idempotency_key, payload
                       ) VALUES ($1, $2, $3, $4, $5, $6, $7)
                       ON CONFLICT (idempotency_key) DO NOTHING
                       RETURNING id"#,
                )
                .bind(Uuid::new_v4())
                .bind(send.company_id)
                .bind(send.channel_id)
                .bind(send.task_id)
                .bind(send.correlation_id.as_uuid())
                .bind(&send.idempotency_key)
                .bind(&send.payload)
                .fetch_optional(&mut *tx)
                .await
                .map_err(AppError::from)?;
                row.map(|(id,)| id)
            }
            None => None,
        };

        if commit.complete_outreach {
            sqlx::query(
                r#"UPDATE task_outreaches SET status = 'completed', updated_at = CURRENT_TIMESTAMP
                   WHERE task_id = $1 AND status IN ('threshold_met', 'proceed_partial')"#,
            )
            .bind(commit.lease.task_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }

        tx.commit().await.map_err(AppError::from)?;
        Ok(DispatchCommit::Committed { outbox_id })
    }

    async fn renew_task_lease(
        &self,
        lease: TaskLeaseRef,
        lock_expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE background_tasks
               SET lock_expires_at = $3, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'processing' AND worker_id = $2
                 AND execution_generation = $4
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(lease.task_id)
        .bind(lease.worker_id)
        .bind(lock_expires_at)
        .bind(lease.execution_generation)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn begin_task_attempt(&self, attempt: TaskAttemptRef) -> AppResult<()> {
        sqlx::query(BEGIN_ATTEMPT_SQL)
            .bind(Uuid::new_v4())
            .bind(attempt.task_id)
            .bind(attempt.attempt_number)
            .bind(attempt.execution_generation)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;
        Ok(())
    }

    async fn finish_task_attempt(&self, outcome: &TaskAttemptOutcome) -> AppResult<bool> {
        // `usize` tokens into an `INTEGER` column: clamp rather than wrap, so an absurd count is
        // recorded as saturated instead of negative — the CHECK constraint rejects negatives.
        let tokens = |pick: fn(&TokenUsage) -> usize| {
            outcome
                .tokens
                .as_ref()
                .map(|usage| i32::try_from(pick(usage)).unwrap_or(i32::MAX))
        };

        let result = sqlx::query(FINISH_ATTEMPT_SQL)
            .bind(outcome.attempt.task_id)
            .bind(outcome.attempt.attempt_number)
            .bind(outcome.attempt.execution_generation)
            .bind(outcome.status.as_str())
            .bind(outcome.error.as_deref())
            .bind(tokens(|usage| usage.prompt_tokens))
            .bind(tokens(|usage| usage.completion_tokens))
            .bind(outcome.stop_reason.as_str())
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(result.rows_affected() == 1)
    }

    async fn claim_pending_tasks(
        &self,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let db_list = sqlx::query_as::<_, BackgroundTaskDb>(
            // Pending rows only. An expired `processing` row used to be stolen right here, which
            // re-ran it without spending an attempt, without closing the open attempt and
            // without any backoff -- so a task that reliably outlived its lease looped for ever
            // instead of dead-lettering. `reap_expired_task_leases` now turns those back into
            // pending rows, paying an attempt each time.
            r#"WITH ranked AS (
                   SELECT id, run_at, created_at,
                          ROW_NUMBER() OVER (
                              PARTITION BY company_id
                              ORDER BY run_at ASC, created_at ASC, id ASC
                          ) AS company_round
                   FROM background_tasks
                   WHERE status = 'pending' AND run_at <= CURRENT_TIMESTAMP
               ), claimable AS (
                   SELECT task.id
                   FROM background_tasks AS task
                   JOIN ranked ON ranked.id = task.id
                   ORDER BY ranked.company_round ASC, ranked.run_at ASC,
                            ranked.created_at ASC, task.id ASC
                   FOR UPDATE SKIP LOCKED
                   LIMIT $1
               )
               UPDATE background_tasks AS task
               SET status = 'processing',
                   worker_id = $2,
                   execution_generation = gen_random_uuid(),
                   locked_at = CURRENT_TIMESTAMP,
                   lock_expires_at = $3,
                   updated_at = CURRENT_TIMESTAMP
               FROM claimable
               WHERE task.id = claimable.id
               RETURNING task.id, task.company_id, task.channel_id, task.thread_id,
                         task.correlation_id, task.task_type, task.status, task.payload,
                         task.retry_count,
                         task.max_retries, task.last_error, task.worker_id, task.execution_generation, task.locked_at,
                         task.lock_expires_at, task.run_at, task.created_at, task.updated_at"#,
        )
        .bind(limit)
        .bind(worker_id)
        .bind(lock_expires_at)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        let mut tasks = Vec::new();
        for db in db_list {
            tasks.push(db.try_into()?);
        }
        Ok(tasks)
    }

    async fn claim_task(
        &self,
        id: Uuid,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let res = sqlx::query(CLAIM_TASK_SQL)
            .bind(id)
            .bind(worker_id)
            .bind(lock_expires_at)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(res.rows_affected() > 0)
    }

    async fn mark_task_completed(&self, lease: TaskLeaseRef) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE background_tasks
               SET status = 'completed', worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                   lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'processing' AND worker_id = $2
                 AND execution_generation = $3
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(lease.task_id)
        .bind(lease.worker_id)
        .bind(lease.execution_generation)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.rows_affected() > 0)
    }

    async fn mark_task_failed(
        &self,
        lease: TaskLeaseRef,
        error_msg: &str,
        next_run_at: DateTime<Utc>,
        is_dead_letter: bool,
    ) -> AppResult<bool> {
        mark_task_failed_on(
            &self.pool,
            lease,
            error_msg,
            next_run_at,
            is_dead_letter,
            TaskStopReason::RetryableFailure,
        )
        .await
    }

    async fn mark_task_failed_with_reason(
        &self,
        lease: TaskLeaseRef,
        error_msg: &str,
        next_run_at: DateTime<Utc>,
        is_dead_letter: bool,
        reason: TaskStopReason,
    ) -> AppResult<bool> {
        mark_task_failed_on(
            &self.pool,
            lease,
            error_msg,
            next_run_at,
            is_dead_letter,
            reason,
        )
        .await
    }

    async fn stop_task(&self, id: Uuid) -> AppResult<BackgroundTask> {
        stop_task_on(&self.pool, id, None).await
    }

    async fn stop_task_as(&self, id: Uuid, operator_id: Uuid) -> AppResult<BackgroundTask> {
        stop_task_on(&self.pool, id, Some(operator_id)).await
    }

    async fn resume_task(&self, id: Uuid) -> AppResult<BackgroundTask> {
        resume_task_on(&self.pool, id, None).await
    }

    async fn resume_task_as(&self, id: Uuid, operator_id: Uuid) -> AppResult<BackgroundTask> {
        resume_task_on(
            &self.pool,
            id,
            Some(TransitionAttribution {
                reason: TaskTransitionReason::OperatorResumed,
                actor_kind: TaskTransitionActorKind::Operator,
                actor_id: Some(operator_id),
                approval_id: None,
                outreach_id: None,
            }),
        )
        .await
    }

    async fn resume_task_after_approval(
        &self,
        id: Uuid,
        approval_id: Uuid,
    ) -> AppResult<BackgroundTask> {
        resume_task_on(
            &self.pool,
            id,
            Some(TransitionAttribution {
                reason: TaskTransitionReason::ApprovalAccepted,
                actor_kind: TaskTransitionActorKind::Approval,
                actor_id: None,
                approval_id: Some(approval_id),
                outreach_id: None,
            }),
        )
        .await
    }

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>> {
        self.list_company_tasks_page(company_id, channel_id, status, sort_asc, 0, 200)
            .await
    }

    async fn list_company_tasks_page(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let mut query = QueryBuilder::<Postgres>::new(
            r#"SELECT id, company_id, channel_id, thread_id, correlation_id, task_type, status, payload,
                      retry_count, max_retries, last_error, worker_id, execution_generation, locked_at, lock_expires_at,
                      run_at, created_at, updated_at
               FROM background_tasks WHERE company_id = "#,
        );
        query.push_bind(company_id);
        if let Some(channel_id) = channel_id {
            query.push(" AND channel_id = ").push_bind(channel_id);
        }
        if let Some(status) = status {
            query.push(" AND status = ").push_bind(status.as_str());
        }
        if sort_asc {
            query.push(" ORDER BY created_at ASC, id ASC");
        } else {
            query.push(" ORDER BY created_at DESC, id DESC");
        }
        query
            .push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        let db_list = query
            .build_query_as::<BackgroundTaskDb>()
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        let mut tasks = Vec::new();
        for db in db_list {
            tasks.push(db.try_into()?);
        }
        Ok(tasks)
    }

    async fn list_company_outbox_page(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<OutboxStatus>,
        sort_asc: bool,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<OutboxEntry>> {
        let mut query = QueryBuilder::<Postgres>::new(format!(
            "SELECT {OUTBOX_COLUMNS} FROM email_outbox WHERE company_id = "
        ));
        query.push_bind(company_id);
        if let Some(channel_id) = channel_id {
            query.push(" AND channel_id = ").push_bind(channel_id);
        }
        if let Some(status) = status {
            query.push(" AND status = ").push_bind(status.as_str());
        }
        // Matches `email_outbox_company_created_idx`, or the channel-qualified
        // `email_outbox_company_channel_created_idx` when one is asked for; ties are broken by id
        // so paging cannot show the same row twice.
        if sort_asc {
            query.push(" ORDER BY created_at ASC, id ASC");
        } else {
            query.push(" ORDER BY created_at DESC, id DESC");
        }
        query
            .push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        let db_list = query
            .build_query_as::<OutboxEntryDb>()
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        let mut entries = Vec::new();
        for db in db_list {
            entries.push(db.try_into()?);
        }
        Ok(entries)
    }

    async fn get_outbox_entry(&self, outbox_id: Uuid) -> AppResult<Option<OutboxEntry>> {
        let db = sqlx::query_as::<_, OutboxEntryDb>(&format!(
            "SELECT {OUTBOX_COLUMNS} FROM email_outbox WHERE id = $1"
        ))
        .bind(outbox_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(OutboxEntry::try_from).transpose()
    }
}

async fn mark_task_failed_on(
    pool: &sqlx::PgPool,
    lease: TaskLeaseRef,
    error_msg: &str,
    next_run_at: DateTime<Utc>,
    is_dead_letter: bool,
    stop_reason: TaskStopReason,
) -> AppResult<bool> {
    let status = if is_dead_letter {
        "dead_letter"
    } else {
        "pending"
    };
    let reason = match stop_reason {
        TaskStopReason::RetryableFailure => TaskTransitionReason::RetryableFailure,
        TaskStopReason::TerminalFailure => TaskTransitionReason::TerminalFailure,
        TaskStopReason::TimedOut => TaskTransitionReason::TimedOut,
        TaskStopReason::Shutdown => TaskTransitionReason::Shutdown,
        TaskStopReason::LeaseLost => TaskTransitionReason::LeaseLost,
        TaskStopReason::Completed => TaskTransitionReason::Completed,
    };
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    set_task_transition_context(
        &mut tx,
        reason,
        TaskTransitionActorKind::Worker,
        Some(lease.worker_id),
    )
    .await?;
    let result = sqlx::query(
        r#"UPDATE background_tasks
           SET status = $1, retry_count = retry_count + 1, last_error = $2,
               run_at = $3, worker_id = NULL, execution_generation = NULL, locked_at = NULL,
               lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
           WHERE id = $4 AND status = 'processing' AND worker_id = $5
             AND execution_generation = $6
             AND lock_expires_at > CURRENT_TIMESTAMP"#,
    )
    .bind(status)
    .bind(error_msg)
    .bind(next_run_at)
    .bind(lease.task_id)
    .bind(lease.worker_id)
    .bind(lease.execution_generation)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;
    tx.commit().await.map_err(AppError::from)?;
    Ok(result.rows_affected() == 1)
}

async fn stop_task_on(
    pool: &sqlx::PgPool,
    id: Uuid,
    operator_id: Option<Uuid>,
) -> AppResult<BackgroundTask> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    if let Some(operator_id) = operator_id {
        set_task_transition_context(
            &mut tx,
            TaskTransitionReason::OperatorStopped,
            TaskTransitionActorKind::Operator,
            Some(operator_id),
        )
        .await?;
    }
    let db = sqlx::query_as::<_, BackgroundTaskDb>(
        r#"UPDATE background_tasks
           SET status = 'stopped', worker_id = NULL, execution_generation = NULL, locked_at = NULL,
               lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
           WHERE id = $1
             AND status IN ('pending', 'processing', 'pending_approval',
                            'waiting_for_third_party_reply', 'failed', 'dead_letter')
           RETURNING id, company_id, channel_id, thread_id, correlation_id, task_type, status,
                     payload, retry_count, max_retries, last_error, worker_id,
                     execution_generation, locked_at, lock_expires_at, run_at, created_at,
                     updated_at"#,
    )
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

struct TransitionAttribution {
    reason: TaskTransitionReason,
    actor_kind: TaskTransitionActorKind,
    actor_id: Option<Uuid>,
    approval_id: Option<Uuid>,
    outreach_id: Option<Uuid>,
}

async fn resume_task_on(
    pool: &sqlx::PgPool,
    id: Uuid,
    attribution: Option<TransitionAttribution>,
) -> AppResult<BackgroundTask> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    if let Some(attribution) = attribution {
        set_task_transition_context(
            &mut tx,
            attribution.reason,
            attribution.actor_kind,
            attribution.actor_id,
        )
        .await?;
        set_task_transition_source(&mut tx, attribution.approval_id, attribution.outreach_id)
            .await?;
    }
    let db = sqlx::query_as::<_, BackgroundTaskDb>(
        r#"UPDATE background_tasks
           SET status = 'pending', run_at = CURRENT_TIMESTAMP, worker_id = NULL,
               execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL,
               updated_at = CURRENT_TIMESTAMP
           WHERE id = $1
             AND status IN ('stopped', 'pending_approval',
                            'waiting_for_third_party_reply', 'failed', 'dead_letter')
           RETURNING id, company_id, channel_id, thread_id, correlation_id, task_type, status,
                     payload, retry_count, max_retries, last_error, worker_id,
                     execution_generation, locked_at, lock_expires_at, run_at, created_at,
                     updated_at"#,
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::from)?;
    tx.commit().await.map_err(AppError::from)?;
    db.try_into()
}

fn required_response_count(target_count: i64, threshold_percent: f64) -> usize {
    ((target_count as f64 * threshold_percent / 100.0).ceil() as usize).max(1)
}

fn outreach_progress(
    outreach: &OutreachDb,
    status: OutreachStatus,
    target_count: i64,
    response_count: i64,
    suspended: bool,
) -> OutreachProgress {
    OutreachProgress {
        id: outreach.id,
        task_id: outreach.task_id,
        status,
        required_threshold_percent: outreach.required_threshold_percent,
        target_count: target_count as usize,
        response_count: response_count as usize,
        required_response_count: required_response_count(
            target_count,
            outreach.required_threshold_percent,
        ),
        expires_at: outreach.expires_at,
        suspended,
    }
}

/// The SQL half of enqueueing: the task row and its channel-target fan-out, which have to commit
/// together. The `ON CONFLICT` clause makes a redelivered source message return the task it already
/// has rather than starting a second run of it.
async fn insert_task(
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

struct TaskTarget {
    channel_id: Uuid,
    thread_id: Uuid,
    recipient_role: String,
}

fn task_targets(
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

fn json_uuid(value: &Value, pointer: &str) -> AppResult<Uuid> {
    let raw = value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal(format!("Missing task target field {pointer}")))?;
    Uuid::parse_str(raw)
        .map_err(|_| AppError::Internal(format!("Invalid task target UUID at {pointer}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::{UNSCOPED_CLAIM, test_pool};
    use crate::entities::message::{Message, MessageDirection, MessageRole};
    use crate::services::outbound_dispatcher::OutboundEmail;
    use crate::use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        thread::ThreadPersistence,
        user::UserPersistence,
    };

    #[test]
    fn quorum_threshold_rounds_up() {
        assert_eq!(required_response_count(1, 100.0), 1);
        assert_eq!(required_response_count(3, 50.0), 2);
        assert_eq!(required_response_count(4, 50.0), 2);
        assert_eq!(required_response_count(10, 20.0), 2);
    }

    #[tokio::test]
    async fn task_chain_board_groups_by_correlation_and_keeps_complete_chain_under_channel_filter()
    {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("board_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Board Test".into(),
                slug: format!("board-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let first_channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "First Channel".into(),
                slug: "first".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let second_channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Second Channel".into(),
                slug: "second".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let participant = crate::entities::value_objects::EmailAddress::from(email);
        let thread = persistence
            .create_thread(
                first_channel.id,
                "Root chain subject",
                std::slice::from_ref(&participant),
            )
            .await
            .unwrap();
        let root = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                first_channel.id,
                Some(thread.id),
                "root_task",
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        let nested = persistence
            .enqueue_task(NewTask {
                company_id: company.id,
                channel_id: second_channel.id,
                thread_id: None,
                task_type: "nested_task".into(),
                payload: serde_json::json!({}),
                correlation_id: root.correlation_id,
            })
            .await
            .unwrap();
        let unrelated = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                first_channel.id,
                Some(thread.id),
                "unrelated_same_thread",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        let filter = TaskBoardFilter::new(Some(second_channel.id), Utc::now());
        let board = persistence
            .list_task_chain_board(company.id, filter)
            .await
            .unwrap();
        assert_eq!(board.total(ChainStage::Queued), 1);
        let card = &board.cards(ChainStage::Queued)[0];
        assert_eq!(card.correlation_id, root.correlation_id);
        assert_eq!(card.counts.total_tasks, 2);
        assert_eq!(card.title, "Root chain subject");
        assert_eq!(
            card.channel_names,
            vec!["First Channel".to_string(), "Second Channel".to_string()]
        );

        let detail = persistence
            .get_task_chain_detail(company.id, root.correlation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            detail
                .tasks
                .iter()
                .map(|item| item.task.id)
                .collect::<Vec<_>>(),
            vec![root.id, nested.id]
        );
        assert_eq!(detail.events.len(), 2);
        assert!(
            detail
                .events
                .iter()
                .all(|event| event.reason == TaskTransitionReason::Enqueued)
        );
        assert!(
            persistence
                .get_task_chain_detail(Uuid::new_v4(), root.correlation_id)
                .await
                .unwrap()
                .is_none()
        );
        assert_ne!(unrelated.correlation_id, root.correlation_id);

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn status_event_constraints_reject_cross_company_rows_and_duplicate_sequences() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("event_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Event Test".into(),
                slug: format!("event-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Events".into(),
                slug: "events".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "event_test",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        let insert = |company_id: Uuid, sequence: i32| {
            sqlx::query(
                r#"INSERT INTO task_status_events (
                       id, company_id, task_id, correlation_id, sequence, from_status, to_status,
                       reason, actor_kind, retry_count, run_at, transitioned_at
                   ) VALUES ($1, $2, $3, $4, $5, 'pending', 'pending',
                             'operator_resumed', 'operator', 0, CURRENT_TIMESTAMP,
                             CURRENT_TIMESTAMP)"#,
            )
            .bind(Uuid::new_v4())
            .bind(company_id)
            .bind(task.id)
            .bind(task.correlation_id.as_uuid())
            .bind(sequence)
            .execute(&pool)
        };
        assert!(insert(Uuid::new_v4(), 2).await.is_err());
        assert!(insert(company.id, 1).await.is_err());

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn guarded_transitions_emit_only_on_success_and_operator_actions_record_the_user() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("actor_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Actor Test".into(),
                slug: format!("actor-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Actors".into(),
                slug: "actors".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "actor_test",
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        let worker = Uuid::new_v4();
        let expires = Utc::now() + chrono::Duration::minutes(5);
        assert!(
            persistence
                .claim_task(task.id, worker, expires)
                .await
                .unwrap()
        );
        assert!(
            !persistence
                .claim_task(task.id, Uuid::new_v4(), expires)
                .await
                .unwrap()
        );
        let before_stop = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap();
        assert_eq!(before_stop.len(), 2, "the failed guard must emit no event");

        persistence.stop_task_as(task.id, owner.id).await.unwrap();
        persistence.resume_task_as(task.id, owner.id).await.unwrap();
        let events = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap();
        for reason in [
            TaskTransitionReason::OperatorStopped,
            TaskTransitionReason::OperatorResumed,
        ] {
            let event = events.iter().find(|event| event.reason == reason).unwrap();
            assert_eq!(event.actor_kind, TaskTransitionActorKind::Operator);
            assert_eq!(event.actor_id, Some(owner.id));
        }

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn pending_claims_take_one_company_round_before_a_second_task_from_a_backlog() {
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("fair_owner_{suffix}@example.com");
        persistence
            .create_user(&format!("fair_owner_{suffix}"), &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();

        let mut companies = Vec::new();
        let mut channels = Vec::new();
        for label in ["backlog", "waiting"] {
            let company = CompanyPersistence::create(
                &persistence,
                owner.id,
                CompanyWrite {
                    name: format!("Fair {label}"),
                    slug: format!("fair-{label}-{suffix}"),
                    ..CompanyWrite::default()
                },
            )
            .await
            .unwrap();
            let channel = ChannelPersistence::create(
                &persistence,
                company.id,
                ChannelWrite {
                    name: label.into(),
                    slug: label.into(),
                    enabled: false,
                    ..ChannelWrite::default()
                },
            )
            .await
            .unwrap();
            companies.push(company);
            channels.push(channel);
        }

        let mut backlog_ids = Vec::new();
        for _ in 0..3 {
            backlog_ids.push(
                persistence
                    .enqueue_task(NewTask::starting_new_chain(
                        companies[0].id,
                        channels[0].id,
                        None,
                        "fairness",
                        serde_json::json!({}),
                    ))
                    .await
                    .unwrap()
                    .id,
            );
        }
        let waiting = persistence
            .enqueue_task(NewTask::starting_new_chain(
                companies[1].id,
                channels[1].id,
                None,
                "fairness",
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        let mut owned_ids = backlog_ids.clone();
        owned_ids.push(waiting.id);
        sqlx::query(
            "UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP - INTERVAL '300 years' WHERE id = ANY($1)",
        )
        .bind(&owned_ids)
        .execute(&pool)
        .await
        .unwrap();

        let claimed = persistence
            .claim_pending_tasks(Uuid::new_v4(), Utc::now() + chrono::Duration::minutes(5), 2)
            .await
            .unwrap();
        assert_eq!(
            claimed
                .iter()
                .filter(|task| task.company_id == companies[0].id)
                .count(),
            1
        );
        assert_eq!(
            claimed
                .iter()
                .filter(|task| task.company_id == companies[1].id)
                .count(),
            1
        );

        for company in companies {
            CompanyPersistence::delete(&persistence, company.id)
                .await
                .unwrap();
        }
    }

    /// The mailbox asks for a whole page of threads at once, and each thread must report the state
    /// of its *current* run rather than whichever task happens to be found first.
    #[tokio::test]
    async fn thread_activity_reports_the_latest_task_per_thread() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("activity_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Activity Test".to_string(),
                slug: format!("activity-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Activity".into(),
                slug: "activity".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());

        let mut threads = Vec::new();
        for subject in ["running", "blocked", "finished", "superseded"] {
            threads.push(
                persistence
                    .create_thread(channel.id, subject, std::slice::from_ref(&email_addr))
                    .await
                    .unwrap(),
            );
        }

        let enqueue = async |thread_id: Uuid| {
            persistence
                .enqueue_task(NewTask::starting_new_chain(
                    company.id,
                    channel.id,
                    Some(thread_id),
                    "email_agent_dispatch",
                    serde_json::json!({}),
                ))
                .await
                .unwrap()
        };

        let running = enqueue(threads[0].id).await;
        let blocked = enqueue(threads[1].id).await;
        let finished = enqueue(threads[2].id).await;
        let old = enqueue(threads[3].id).await;
        let current = enqueue(threads[3].id).await;

        // `background_tasks_lease_check` gives the lease columns to `processing` rows and to no
        // other status, so they move together here exactly as the worker moves them.
        let set_status = async |id: Uuid, status: &str, lease: Option<DateTime<Utc>>| {
            sqlx::query(
                "UPDATE background_tasks
                 SET status = $2,
                     lock_expires_at = $3,
                     worker_id = CASE WHEN $3::timestamptz IS NULL THEN NULL ELSE gen_random_uuid() END,
                     execution_generation =
                         CASE WHEN $3::timestamptz IS NULL THEN NULL ELSE gen_random_uuid() END,
                     -- Derived from the lease, not from now: the check also demands
                     -- lock_expires_at > locked_at, and an expired lease is set in the past.
                     locked_at = $3::timestamptz - interval '10 minutes',
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = $1",
            )
            .bind(id)
            .bind(status)
            .bind(lease)
            .execute(&pool)
            .await
            .unwrap();
        };

        let live_lease = Utc::now() + chrono::Duration::minutes(5);
        set_status(running.id, "processing", Some(live_lease)).await;
        set_status(blocked.id, "pending_approval", None).await;
        set_status(finished.id, "completed", None).await;
        // Older task on the same thread ended badly; the newer one is what the reader should see.
        set_status(old.id, "dead_letter", None).await;
        set_status(current.id, "processing", Some(live_lease)).await;

        let ids: Vec<Uuid> = threads.iter().map(|thread| thread.id).collect();
        let activity = persistence.list_thread_activity(&ids).await.unwrap();

        assert_eq!(activity.get(&threads[0].id), Some(&ThreadActivity::Working));
        assert_eq!(
            activity.get(&threads[1].id),
            Some(&ThreadActivity::WaitingApproval)
        );
        assert_eq!(
            activity.get(&threads[2].id),
            None,
            "a finished thread reports nothing at all, rather than an idle badge"
        );
        assert_eq!(
            activity.get(&threads[3].id),
            Some(&ThreadActivity::Working),
            "the newest task wins over an older dead letter on the same thread"
        );

        // Asking again after a failure and getting an answer settles the thread: the run that
        // worked is its last word, and the dead letter behind it is history rather than a badge.
        set_status(current.id, "completed", None).await;
        let activity = persistence.list_thread_activity(&ids).await.unwrap();
        assert_eq!(
            activity.get(&threads[3].id),
            None,
            "a successful run buries the dead letter it was asked to make up for"
        );

        // The failure still stands on its own while nothing has answered it.
        set_status(current.id, "stopped", None).await;
        let activity = persistence.list_thread_activity(&ids).await.unwrap();
        assert_eq!(
            activity.get(&threads[3].id),
            Some(&ThreadActivity::Failed),
            "a run that was stopped rather than answered leaves the failure showing"
        );
        set_status(current.id, "completed", None).await;

        // An abandoned worker leaves `processing` behind; that is queued work, not a live agent.
        set_status(
            running.id,
            "processing",
            Some(Utc::now() - chrono::Duration::minutes(5)),
        )
        .await;
        let activity = persistence.list_thread_activity(&ids).await.unwrap();
        assert_eq!(activity.get(&threads[0].id), Some(&ThreadActivity::Queued));

        assert!(
            persistence
                .list_thread_activity(&[])
                .await
                .unwrap()
                .is_empty(),
            "an empty page must not hit the database"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    /// An expired lease must cost an attempt, close its ledger row, and back the task off --
    /// and eventually dead-letter it.
    ///
    /// Regression for the shape where `claim_pending_tasks` stole expired `processing` rows
    /// directly. That re-ran the task with `retry_count` untouched, left the abandoned attempt
    /// sitting in `task_attempts` as `processing` for ever, and applied no backoff, so a task
    /// that reliably outlived its lease was retried in a tight loop and never dead-lettered.
    #[tokio::test]
    async fn an_expired_task_lease_costs_an_attempt_and_eventually_dead_letters() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("reaper_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Reaper Test".to_string(),
                slug: format!("reaper-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Reaper".into(),
                slug: "reaper".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
        let thread = persistence
            .create_thread(channel.id, "Reaper", std::slice::from_ref(&email_addr))
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread.id),
                "test",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        // Run the task up to its retry ceiling, losing the lease every time.
        let max_retries = task.max_retries;
        let mut generations = Vec::new();
        for attempt in 1..=max_retries {
            // Due now, whatever backoff the previous reap applied.
            sqlx::query("UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP WHERE id = $1")
                .bind(task.id)
                .execute(&pool)
                .await
                .unwrap();

            // By id rather than a batch claim: the batch sweeps the whole queue, so tests
            // running beside this one would fill it or take this row first.
            assert!(
                persistence
                    .claim_task(
                        task.id,
                        Uuid::new_v4(),
                        Utc::now() + chrono::Duration::minutes(5)
                    )
                    .await
                    .unwrap(),
                "the task is pending and due, so it must be claimable"
            );
            let claimed = persistence
                .get_task_by_id(task.id)
                .await
                .unwrap()
                .expect("the task still exists");
            let lease = TaskLeaseRef::of(&claimed).expect("a claim records its lease");
            generations.push(lease.execution_generation);
            persistence
                .begin_task_attempt(TaskAttemptRef::of(&claimed, lease))
                .await
                .unwrap();

            // The run vanishes: its lease lapses with nothing reported.
            sqlx::query(
                "UPDATE background_tasks
                 SET locked_at = CURRENT_TIMESTAMP - interval '20 minutes',
                     lock_expires_at = CURRENT_TIMESTAMP - interval '1 second'
                 WHERE id = $1",
            )
            .bind(task.id)
            .execute(&pool)
            .await
            .unwrap();

            // The sweep is global, so a test running beside this one may reap this row first.
            // What matters is the state the row ends in, not whose call got there.
            persistence.reap_expired_task_leases().await.unwrap();

            let after = persistence
                .get_task_by_id(task.id)
                .await
                .unwrap()
                .expect("the task still exists");
            assert_eq!(
                after.retry_count, attempt,
                "each lapsed lease must spend exactly one attempt"
            );
            assert!(after.worker_id.is_none());
            assert!(after.execution_generation.is_none());

            if attempt < max_retries {
                assert_eq!(after.status, TaskStatus::Pending);
                assert!(
                    after.run_at > Utc::now(),
                    "a reaped task must wait out its backoff"
                );
            } else {
                assert_eq!(
                    after.status,
                    TaskStatus::DeadLetter,
                    "the attempt budget is spent, so the task must stop rather than loop"
                );
            }
        }

        // Every claim minted its own generation, which is what fences a superseded run.
        let unique = generations.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(
            unique.len(),
            generations.len(),
            "each claim must mint a distinct execution generation"
        );

        // No attempt was left open: the reaper closed each one as it went.
        let open: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attempts WHERE task_id = $1 AND status = 'processing'",
        )
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(open, 0, "a reaped run must not leave its ledger row open");
        let lease_lost: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_attempts WHERE task_id = $1 AND stop_reason = $2",
        )
        .bind(task.id)
        .bind(TaskStopReason::LeaseLost.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            lease_lost,
            i64::from(task.max_retries),
            "every reaped attempt records why execution stopped"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    /// A run whose lease was reaped must not be able to write anything, even if the same worker
    /// id re-claims the task. Only the generation can tell those two runs apart.
    #[tokio::test]
    async fn a_superseded_run_cannot_renew_write_or_close_the_task() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("fence_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Fence Test".to_string(),
                slug: format!("fence-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Fence".into(),
                slug: "fence".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
        let thread = persistence
            .create_thread(channel.id, "Fence", std::slice::from_ref(&email_addr))
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread.id),
                "test",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        // Deliberately the *same* worker both times, so only the generation differs. This is the
        // case a `worker_id = $me` guard cannot catch.
        let worker = Uuid::new_v4();

        // By id rather than a batch claim: the batch sweeps the whole queue, so a test running
        // beside this one could take this row first.
        assert!(
            persistence
                .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
                .await
                .unwrap()
        );
        let first = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        let stale = TaskLeaseRef::of(&first).expect("a claim records its lease");

        sqlx::query(
            "UPDATE background_tasks
             SET locked_at = CURRENT_TIMESTAMP - interval '20 minutes',
                 lock_expires_at = CURRENT_TIMESTAMP - interval '1 second'
             WHERE id = $1",
        )
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();
        persistence.reap_expired_task_leases().await.unwrap();

        sqlx::query("UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(task.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            persistence
                .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
                .await
                .unwrap()
        );
        let second = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        let current = TaskLeaseRef::of(&second).expect("a claim records its lease");

        assert_eq!(stale.worker_id, current.worker_id, "same worker both times");
        assert_ne!(stale.execution_generation, current.execution_generation);

        // Nothing the superseded run tries may land.
        assert!(
            !persistence
                .renew_task_lease(stale, Utc::now() + chrono::Duration::minutes(5))
                .await
                .unwrap()
        );
        assert!(!persistence.mark_task_completed(stale).await.unwrap());
        assert!(
            !persistence
                .mark_task_failed(stale, "stale", Utc::now(), false)
                .await
                .unwrap()
        );

        // The payload the superseded run tried to write never landed.
        let after = persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .expect("the task still exists");
        assert_eq!(after.status, TaskStatus::Processing);
        assert!(after.payload.get("stale").is_none());

        // The run that actually owns the task is unaffected.
        assert!(persistence.mark_task_completed(current).await.unwrap());

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    /// One dispatch's reply, its outbox row and its task payload land together or not at all.
    ///
    /// They used to be three independent commits: the outbox row, then a `create_message` per
    /// answered thread, then the payload. A crash or a lost lease part-way left a thread showing
    /// an answer that was never sent, or an email going out for a task whose payload said it had
    /// never run -- and the retry then had to reconcile the difference.
    #[tokio::test]
    async fn a_dispatch_commits_its_reply_outbox_row_and_payload_together_or_not_at_all() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("commit_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Commit Test".to_string(),
                slug: format!("commit-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Commit".into(),
                slug: "commit".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
        let thread = persistence
            .create_thread(channel.id, "Commit", std::slice::from_ref(&email_addr))
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread.id),
                "test",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        let worker = Uuid::new_v4();
        assert!(
            persistence
                .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
                .await
                .unwrap()
        );
        let claimed = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        let lease = TaskLeaseRef::of(&claimed).expect("a claim records its lease");

        let reply = |message_id: &str| Message {
            id: Uuid::new_v4(),
            thread_id: thread.id,
            message_id: MessageId::from(message_id.to_string()),
            in_reply_to: None,
            references_list: Vec::new(),
            sender: crate::entities::value_objects::EmailAddress::from("agent@example.com"),
            recipients_to: vec![email_addr.clone()],
            recipients_cc: Vec::new(),
            subject: "Re: Commit".to_string(),
            clean_text_body: "the answer".to_string(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            thread_index: None,
            created_at: Utc::now(),
        };
        let send = |key: &str| OutboundSend {
            correlation_id: CorrelationId::new(),
            company_id: company.id,
            channel_id: channel.id,
            task_id: Some(task.id),
            idempotency_key: key.to_string(),
            payload: serde_json::json!({"body": "the answer"}),
        };

        let outbound_rows = async || -> i64 {
            sqlx::query_scalar("SELECT COUNT(*) FROM email_outbox WHERE task_id = $1")
                .bind(task.id)
                .fetch_one(&pool)
                .await
                .unwrap()
        };
        let thread_rows = async || -> i64 {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM thread_messages WHERE thread_id = $1 AND direction = 'outbound'",
            )
            .bind(thread.id)
            .fetch_one(&pool)
            .await
            .unwrap()
        };

        // A superseded run: the same task and worker, a generation that is no longer current.
        let stale = TaskLeaseRef {
            execution_generation: Uuid::new_v4(),
            ..lease
        };
        let outcome = persistence
            .commit_agent_dispatch(AgentDispatchCommit {
                lease: stale,
                messages: &[reply("<stale@example.com>")],
                outbound: Some(send("stale-key")),
                payload: serde_json::json!({"stale": true}),
                complete_outreach: false,
            })
            .await
            .unwrap();
        assert_eq!(outcome, DispatchCommit::LeaseLost);

        // Not one of the three parts may have landed.
        assert_eq!(thread_rows().await, 0, "no reply may be stored");
        assert_eq!(outbound_rows().await, 0, "no email may be queued");
        let after_stale = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        assert!(
            after_stale.payload.get("stale").is_none(),
            "no payload may be written"
        );

        // The run that actually owns the lease commits all three.
        let outcome = persistence
            .commit_agent_dispatch(AgentDispatchCommit {
                lease,
                messages: &[reply("<live@example.com>")],
                outbound: Some(send("live-key")),
                payload: serde_json::json!({"committed": true}),
                complete_outreach: false,
            })
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            DispatchCommit::Committed { outbox_id: Some(_) }
        ));
        assert_eq!(thread_rows().await, 1);
        assert_eq!(outbound_rows().await, 1);
        let after = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        assert_eq!(
            after.payload.get("committed"),
            Some(&serde_json::json!(true))
        );

        // Re-queueing the same logical send is the idempotency key doing its job, not a failure,
        // and it must not duplicate the outbox row.
        let outcome = persistence
            .commit_agent_dispatch(AgentDispatchCommit {
                lease,
                messages: &[],
                outbound: Some(send("live-key")),
                payload: serde_json::json!({"committed": true}),
                complete_outreach: false,
            })
            .await
            .unwrap();
        assert_eq!(outcome, DispatchCommit::Committed { outbox_id: None });
        assert_eq!(
            outbound_rows().await,
            1,
            "the same send must not queue twice"
        );

        // A failure part-way must roll back what already succeeded in the same transaction. The
        // payload write happens first, so a message that cannot be stored has to undo it: this is
        // the case three separate commits could not handle at all.
        let orphan = Message {
            thread_id: Uuid::new_v4(),
            ..reply("<orphan@example.com>")
        };
        let failed = persistence
            .commit_agent_dispatch(AgentDispatchCommit {
                lease,
                messages: &[orphan],
                outbound: Some(send("orphan-key")),
                payload: serde_json::json!({"rolled_back": true}),
                complete_outreach: false,
            })
            .await;
        assert!(failed.is_err(), "a message with no thread cannot be stored");

        let after_rollback = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        assert!(
            after_rollback.payload.get("rolled_back").is_none(),
            "the payload write must be rolled back when a later write fails"
        );
        assert_eq!(
            after_rollback.payload.get("committed"),
            Some(&serde_json::json!(true)),
            "and the previously committed payload must survive untouched"
        );
        assert_eq!(
            outbound_rows().await,
            1,
            "the failed dispatch must not leave an outbox row behind"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn concurrent_workers_claim_once_and_a_failed_task_is_not_immediately_reclaimed() {
        // Both claims below are unscoped, and the row is deliberately sorted to the very front of
        // the queue -- which makes it the first thing any *other* unscoped claim takes too. Held
        // from before the row is queued until after the last claim, so the only claims racing for
        // it are this test's own two.
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("queue_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Queue Test".to_string(),
                slug: format!("queue-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Queue".into(),
                slug: "queue".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
        let thread = persistence
            .create_thread(channel.id, "Queue", std::slice::from_ref(&email_addr))
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread.id),
                "test",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        // `claim_pending_tasks` polls the whole queue, not this company's slice of it. Its first
        // sort key is the per-company round, and a brand-new company's only task is always round 1
        // — so sorting this row ahead of every other round-1 row is what puts it first overall,
        // ahead of concurrent tests' rows and any orphans a previously aborted run left behind.
        // Without it both single-slot workers fill up elsewhere and never reach this task. Keeping
        // the limit at 1 also means this test steals at most one foreign task.
        sqlx::query(
            "UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP - INTERVAL '100 years' WHERE id = $1",
        )
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();

        let first_worker = Uuid::new_v4();
        let second_worker = Uuid::new_v4();
        let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
        let (first, second) = tokio::join!(
            persistence.claim_pending_tasks(first_worker, expires_at, 1),
            persistence.claim_pending_tasks(second_worker, expires_at, 1)
        );
        let claimed: Vec<_> = first.unwrap().into_iter().chain(second.unwrap()).collect();

        // The invariant that matters is that *this* task went to exactly one worker — asserting on
        // the combined queue total would count whatever else the other worker legitimately claimed.
        assert_eq!(
            claimed.iter().filter(|claim| claim.id == task.id).count(),
            1,
            "a pending task must be claimed by exactly one worker"
        );
        let events = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.reason == TaskTransitionReason::Claimed)
                .count(),
            1,
            "competing claimants must produce one claimed transition event"
        );

        // Two claims, one of which is ours by construction — so the other necessarily took someone
        // else's task, and it now holds a five-minute lease on it. Hand it straight back: whichever
        // test queued it is about to find its own task already claimed and fail on a state it never
        // set. Releasing to 'pending' is where a reaped lease lands anyway.
        let borrowed: Vec<Uuid> = claimed
            .iter()
            .map(|claim| claim.id)
            .filter(|id| *id != task.id)
            .collect();
        if !borrowed.is_empty() {
            sqlx::query(
                "UPDATE background_tasks
                    SET status = 'pending', worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                        lock_expires_at = NULL
                  WHERE id = ANY($1)",
            )
            .bind(&borrowed)
            .execute(&pool)
            .await
            .unwrap();
        }

        // This single row fills the worker's one-task batch. Failing it must move it behind
        // persisted backoff before `MoreWaiting` sends the worker straight into another
        // iteration; otherwise a poison task is reclaimed without any clock advance.
        let claimed_task = claimed
            .iter()
            .find(|claim| claim.id == task.id)
            .expect("this task was claimed");
        let claimed_lease =
            TaskLeaseRef::of(claimed_task).expect("a claimed task records its lease");
        assert!(
            persistence
                .mark_task_failed(
                    claimed_lease,
                    "poison task",
                    Utc::now() + chrono::Duration::minutes(1),
                    false,
                )
                .await
                .unwrap()
        );

        let immediate_worker = Uuid::new_v4();
        let immediate = persistence
            .claim_pending_tasks(
                immediate_worker,
                Utc::now() + chrono::Duration::minutes(5),
                1,
            )
            .await
            .unwrap();
        assert!(
            immediate.iter().all(|claim| claim.id != task.id),
            "a failed full batch must not reclaim the same task on the zero-delay iteration"
        );
        let immediate_borrowed: Vec<Uuid> = immediate.iter().map(|claim| claim.id).collect();
        if !immediate_borrowed.is_empty() {
            sqlx::query(
                "UPDATE background_tasks
                    SET status = 'pending', worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                        lock_expires_at = NULL
                  WHERE id = ANY($1) AND worker_id = $2",
            )
            .bind(&immediate_borrowed)
            .bind(immediate_worker)
            .execute(&pool)
            .await
            .unwrap();
        }

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn only_one_worker_queues_an_outbound_send() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("enqueue_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Enqueue Test".to_string(),
                slug: format!("enqueue-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Enqueue".into(),
                slug: "enqueue".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();

        let key = format!("task:{suffix}:agent-reply");
        let send = || OutboundSend {
            correlation_id: CorrelationId::new(),
            company_id: company.id,
            channel_id: channel.id,
            task_id: None,
            idempotency_key: key.clone(),
            payload: serde_json::json!({}),
        };

        // Two workers race to hand the transport the same logical reply; only one may queue it,
        // or the customer receives the answer twice.
        let (first, second) = tokio::join!(
            persistence.enqueue_outbound_send(send()),
            persistence.enqueue_outbound_send(send())
        );
        let queued: Vec<_> = [first.unwrap(), second.unwrap()]
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(
            queued.len(),
            1,
            "the unique idempotency key must admit exactly one send"
        );

        // Put the row out of reach before asking what state it is in. Claiming is unscoped by
        // design — `claim_outbox_emails` takes any `pending` row whose `available_at` has arrived,
        // because that is what a real poller does — so a concurrent test would otherwise claim this
        // row and the assertions below would be reporting on that, not on what queueing did.
        // Pushing `available_at` out excludes it from every claim set without changing the columns
        // under test.
        sqlx::query(
            "UPDATE email_outbox SET available_at = CURRENT_TIMESTAMP + interval '1 hour'
             WHERE id = $1",
        )
        .bind(queued[0])
        .execute(&pool)
        .await
        .unwrap();

        // The row is left for the poller to claim: 'pending', unleased, and not owned by anyone.
        let (status, worker_id): (String, Option<Uuid>) =
            sqlx::query_as("SELECT status, worker_id FROM email_outbox WHERE id = $1")
                .bind(queued[0])
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "pending");
        assert!(
            worker_id.is_none(),
            "queueing must not claim the row; the outbox poller does that"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_outbox_lists_one_channel_at_a_time() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("outbox_channel_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Outbox Channel Test".to_string(),
                slug: format!("outbox-channel-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        let mut channels = Vec::new();
        for name in ["Support", "Billing"] {
            let channel = ChannelPersistence::create(
                &persistence,
                company.id,
                ChannelWrite {
                    name: name.into(),
                    slug: name.to_lowercase(),
                    enabled: false,
                    ..ChannelWrite::default()
                },
            )
            .await
            .unwrap();
            persistence
                .enqueue_outbound_send(OutboundSend {
                    correlation_id: CorrelationId::new(),
                    company_id: company.id,
                    channel_id: channel.id,
                    task_id: None,
                    idempotency_key: format!("{suffix}:{name}"),
                    payload: serde_json::json!({ "subject": name }),
                })
                .await
                .unwrap()
                .unwrap();
            channels.push(channel);
        }

        // Unfiltered, the company sees both channels' mail.
        let all = persistence
            .list_company_outbox_page(company.id, None, None, false, 0, 50)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);

        // Filtered, it sees exactly one — the whole point of promoting the channel out of the
        // payload and indexing it.
        let support = persistence
            .list_company_outbox_page(company.id, Some(channels[0].id), None, false, 0, 50)
            .await
            .unwrap();
        assert_eq!(support.len(), 1);
        assert_eq!(support[0].channel_id, Some(channels[0].id));
        assert_eq!(support[0].subject(), Some("Support"));

        // Deleting the channel must not delete the record that mail went out for it.
        ChannelPersistence::delete(&persistence, channels[0].id)
            .await
            .unwrap();
        let orphaned = persistence
            .list_company_outbox_page(company.id, None, None, false, 0, 50)
            .await
            .unwrap();
        assert_eq!(orphaned.len(), 2);
        assert!(
            orphaned
                .iter()
                .any(|entry| entry.subject() == Some("Support") && entry.channel_id.is_none()),
            "a deleted channel must null the column, not cascade the send record away"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn failed_outbox_batch_is_backed_off_and_expired_leases_reach_the_cap() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("reap_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Reap Test".to_string(),
                slug: format!("reap-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Reap".into(),
                slug: "reap".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();

        let outbox_id = persistence
            .enqueue_outbound_send(OutboundSend {
                correlation_id: CorrelationId::new(),
                company_id: company.id,
                channel_id: channel.id,
                task_id: None,
                idempotency_key: format!("reap:{suffix}"),
                payload: serde_json::json!({}),
            })
            .await
            .unwrap()
            .unwrap();

        let state = async |id: Uuid| -> (String, i32, bool) {
            sqlx::query_as::<_, (String, i32, bool)>(
                "SELECT status, retry_count, available_at > CURRENT_TIMESTAMP
                 FROM email_outbox WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
        };
        // Claiming is the subject here, so this calls the real query rather than leasing the row
        // by hand — but the query is unscoped by design and would take whatever else is queued.
        // `claim_outbox_emails` orders by `(available_at, id)`, so this row has to sort ahead of
        // every neighbour for a LIMIT of 1 to claim precisely it. Backdating by a fixed hour does
        // not achieve that: the test database accumulates `pending` rows from earlier runs, and one
        // left behind yesterday is older than any constant offset from now. So the offset is taken
        // from the queue's own minimum, which puts this row strictly first whatever is already
        // there. The release below covers the rest: once the backoff pushes this row into the
        // future it stops sorting first, and the claim would otherwise carry off whichever row did.
        sqlx::query(
            "UPDATE email_outbox
                SET available_at =
                    (SELECT LEAST(MIN(available_at), CURRENT_TIMESTAMP) FROM email_outbox)
                    - interval '1 hour'
              WHERE id = $1",
        )
        .bind(outbox_id)
        .execute(&pool)
        .await
        .unwrap();

        let worker_id = Uuid::new_v4();
        let claimed_ours = async |limit: i64| -> bool {
            let claimed = persistence
                .claim_outbox_emails(worker_id, Utc::now() + chrono::Duration::minutes(15), limit)
                .await
                .unwrap();
            let ours = claimed.iter().any(|email| email.id == outbox_id);

            let borrowed: Vec<Uuid> = claimed
                .iter()
                .map(|email| email.id)
                .filter(|id| *id != outbox_id)
                .collect();
            if !borrowed.is_empty() {
                sqlx::query(
                    "UPDATE email_outbox
                        SET status = 'pending', worker_id = NULL, locked_at = NULL,
                            lock_expires_at = NULL
                      WHERE id = ANY($1) AND worker_id = $2",
                )
                .bind(&borrowed)
                .bind(worker_id)
                .execute(&pool)
                .await
                .unwrap();
            }

            ours
        };

        assert!(
            claimed_ours(1).await,
            "a pending row is the poller's to take"
        );

        // One row fills this test's batch. A retryable delivery failure must make it unavailable
        // before `MoreWaiting` drives the next iteration, so the unchanged clock cannot feed the
        // same poison email back to the worker.
        assert!(
            persistence
                .mark_outbox_email_failed(outbox_id, worker_id, "poison delivery")
                .await
                .unwrap()
        );
        assert!(
            !claimed_ours(1).await,
            "a failed full batch must not reclaim the same email on the zero-delay iteration"
        );

        // Make the same row due again so the remainder of the test can exercise lease expiry.
        sqlx::query(
            "UPDATE email_outbox SET available_at = CURRENT_TIMESTAMP - interval '1 second'
              WHERE id = $1",
        )
        .bind(outbox_id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(claimed_ours(1).await);

        // A lease that is still running belongs to the worker holding it.
        persistence.reap_expired_outbox_leases().await.unwrap();
        assert_eq!(state(outbox_id).await.0, "sending");

        // Now the worker dies mid-delivery: the lease lapses with no result ever written. Before
        // expiry was counted, this row came back every lease period at retry_count 0, forever.
        // Two attempts are already spent so the backoff is long enough to observe.
        sqlx::query(
            "UPDATE email_outbox SET retry_count = 2,
                 locked_at = CURRENT_TIMESTAMP - interval '2 minutes',
                 lock_expires_at = CURRENT_TIMESTAMP - interval '1 minute' WHERE id = $1",
        )
        .bind(outbox_id)
        .execute(&pool)
        .await
        .unwrap();

        assert!(persistence.reap_expired_outbox_leases().await.unwrap() >= 1);
        assert_eq!(
            state(outbox_id).await,
            ("pending".to_string(), 3, true),
            "a lapsed lease spends an attempt and backs the row off"
        );
        assert!(
            !claimed_ours(1).await,
            "the backoff must hold the row back, or the reaper is just a slower redelivery loop"
        );

        // One attempt short of the cap: the next lapse is terminal, without any worker having
        // managed to write a failure.
        sqlx::query(
            "UPDATE email_outbox SET status = 'sending', retry_count = 4, worker_id = $2,
                 locked_at = CURRENT_TIMESTAMP - interval '2 minutes',
                 lock_expires_at = CURRENT_TIMESTAMP - interval '1 minute' WHERE id = $1",
        )
        .bind(outbox_id)
        .bind(worker_id)
        .execute(&pool)
        .await
        .unwrap();

        persistence.reap_expired_outbox_leases().await.unwrap();
        assert_eq!(state(outbox_id).await.0, "failed");
        assert!(!claimed_ours(1).await);

        let incoherent = sqlx::query("UPDATE email_outbox SET status = 'sending' WHERE id = $1")
            .bind(outbox_id)
            .execute(&pool)
            .await;
        assert!(
            incoherent.is_err(),
            "a sending row without complete lease ownership must be rejected"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn an_undeliverable_payload_is_dead_lettered_on_the_first_attempt() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("dead_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Dead Letter Test".to_string(),
                slug: format!("dead-letter-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Dead".into(),
                slug: "dead".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();

        let outbox_id = persistence
            .enqueue_outbound_send(OutboundSend {
                correlation_id: CorrelationId::new(),
                company_id: company.id,
                channel_id: channel.id,
                task_id: None,
                idempotency_key: format!("dead:{suffix}"),
                payload: serde_json::json!({ "not": "an OutboundEmail" }),
            })
            .await
            .unwrap()
            .unwrap();

        // Take the lease on this row alone, rather than calling `claim_outbox_emails`. Claiming is
        // setup here, not the subject — what is under test is the `worker_id` guard below — and the
        // real claim is unscoped: it would take up to `limit` rows belonging to whatever else is
        // running, and could equally lose this row to another claimer before the guard is reached.
        let worker_id = Uuid::new_v4();
        let leased = sqlx::query(
            "UPDATE email_outbox
                SET status = 'sending', worker_id = $2, locked_at = CURRENT_TIMESTAMP,
                    lock_expires_at = CURRENT_TIMESTAMP + interval '15 minutes'
              WHERE id = $1",
        )
        .bind(outbox_id)
        .bind(worker_id)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(leased.rows_affected(), 1, "the row exists and is now ours");

        // Only the worker holding the lease may dead-letter the row.
        assert!(
            !persistence
                .mark_outbox_email_dead(outbox_id, Uuid::new_v4(), "wrong worker")
                .await
                .unwrap()
        );
        assert!(
            persistence
                .mark_outbox_email_dead(outbox_id, worker_id, "payload will never deserialize")
                .await
                .unwrap()
        );

        let entry = persistence
            .get_outbox_entry(outbox_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.status, OutboxStatus::Failed);
        assert_eq!(
            entry.last_error.as_deref(),
            Some("payload will never deserialize")
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn task_deliveries_are_visible_without_the_transport_writing_back() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("delivery_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Delivery Test".to_string(),
                slug: format!("delivery-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Delivery".into(),
                slug: "delivery".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "test",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        // A task that sent nothing shows no delivery section at all.
        assert!(
            persistence
                .list_task_deliveries(task.id)
                .await
                .unwrap()
                .is_empty()
        );

        let outbox_id = persistence
            .enqueue_outbound_send(OutboundSend {
                correlation_id: CorrelationId::new(),
                company_id: company.id,
                channel_id: channel.id,
                task_id: Some(task.id),
                idempotency_key: format!("task:{}:agent-reply", task.id),
                payload: serde_json::json!({}),
            })
            .await
            .unwrap()
            .unwrap();

        // This test asserts the row is still `Pending`, so it must not be claimable while it does:
        // claiming is unscoped, and a concurrent poller taking the row would move it to 'sending'.
        // A future `available_at` puts it outside every claim set without touching `status`.
        sqlx::query(
            "UPDATE email_outbox SET available_at = CURRENT_TIMESTAMP + interval '1 hour'
             WHERE id = $1",
        )
        .bind(outbox_id)
        .execute(&pool)
        .await
        .unwrap();

        let deliveries = persistence.list_task_deliveries(task.id).await.unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].id, outbox_id);
        assert_eq!(deliveries[0].status, OutboxStatus::Pending);
        assert_eq!(deliveries[0].retry_count, 0);
        assert!(deliveries[0].sent_at.is_none());

        // A dead-lettered delivery stays visible against a task that is not itself failed — that
        // separation is the whole point of joining transport state in at read time.
        sqlx::query(
            "UPDATE email_outbox SET status = 'failed', retry_count = 5, last_error = 'no route' WHERE id = $1",
        )
        .bind(outbox_id)
        .execute(&pool)
        .await
        .unwrap();

        let deliveries = persistence.list_task_deliveries(task.id).await.unwrap();
        assert_eq!(deliveries[0].status, OutboxStatus::Failed);
        assert_eq!(deliveries[0].retry_count, 5);
        assert_eq!(deliveries[0].last_error.as_deref(), Some("no route"));

        let task_after = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        assert_ne!(
            task_after.status,
            TaskStatus::Failed,
            "a failed delivery must not fail the task that produced it"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn outreach_reply_reaches_quorum_and_resumes_task() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let owner_email = format!("outreach_owner_{suffix}@example.com");
        persistence
            .create_user(&format!("outreach_owner_{suffix}"), &owner_email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &owner_email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Outreach Test".to_string(),
                slug: format!("outreach-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Outreach".into(),
                slug: "outreach".into(),
                participant_emails: Some(vec![owner_email.clone()]),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let owner_email_addr =
            crate::entities::value_objects::EmailAddress::from(owner_email.clone());
        let thread = persistence
            .create_thread(
                channel.id,
                "Need response",
                std::slice::from_ref(&owner_email_addr),
            )
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread.id),
                "email_agent_dispatch",
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        let worker_id = Uuid::new_v4();
        assert!(
            persistence
                .claim_task(
                    task.id,
                    worker_id,
                    chrono::Utc::now() + chrono::Duration::minutes(5),
                )
                .await
                .unwrap()
        );
        let outreach_id = Uuid::new_v4();
        let outbox_id = Uuid::new_v4();
        let target_email = "vendor@supplier.example";
        let outbox_payload = serde_json::to_value(OutboundEmail {
            correlation_id: CorrelationId::new(),
            channel_id: channel.id,
            channel_name: channel.name.clone(),
            channel_slug: channel.slug.clone(),
            company_slug: company.slug.clone(),
            trigger_message_id: "<request@example.com>".into(),
            thread_references: Vec::new(),
            recipient_to: target_email.into(),
            recipients_cc: Vec::new(),
            subject: "Question".into(),
            body_text: "Please respond".into(),
            hop_count: 0,
            trace_channels: Vec::new(),
        })
        .unwrap();
        let progress = persistence
            .create_outreach_and_pause(CreateOutreachRequest {
                correlation_id: CorrelationId::new(),
                id: outreach_id,
                task_id: task.id,
                company_id: company.id,
                channel_id: channel.id,
                worker_id,
                outreach_key: "integration-outreach".into(),
                required_threshold_percent: 100.0,
                expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
                subject: "Question".into(),
                body: "Please respond".into(),
                targets: vec![crate::entities::outreach::OutreachTargetRequest {
                    email: target_email.into(),
                    outbox_id,
                    outbox_payload,
                }],
            })
            .await
            .unwrap();
        assert!(progress.suspended);
        assert_eq!(
            persistence
                .get_task_by_id(task.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::WaitingForThirdPartyReply
        );
        let started = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap()
            .into_iter()
            .find(|event| event.reason == TaskTransitionReason::OutreachStarted)
            .expect("outreach suspension records its source");
        assert_eq!(started.related_outreach_id, Some(outreach_id));

        let outbound_message_id = "<outreach-vendor@mailagents.test>";
        sqlx::query(
            "UPDATE email_outbox SET status = 'sent', provider_message_id = $2 WHERE id = $1",
        )
        .bind(outbox_id)
        .bind(outbound_message_id)
        .execute(&persistence.pool)
        .await
        .unwrap();
        let matched = persistence
            .find_correlated_outreach_reply(
                company.id,
                channel.id,
                thread.id,
                target_email,
                &[outbound_message_id.into()],
            )
            .await
            .unwrap()
            .unwrap();
        let response = persistence
            .create_message(&Message {
                id: Uuid::new_v4(),
                thread_id: thread.id,
                message_id: "<vendor-response@supplier.example>".into(),
                in_reply_to: Some(outbound_message_id.into()),
                references_list: vec![outbound_message_id.into()],
                sender: target_email.into(),
                recipients_to: vec![owner_email.clone().into()],
                recipients_cc: Vec::new(),
                subject: "Re: Question".into(),
                clean_text_body: "Confirmed".into(),
                raw_text_body: None,
                raw_html_body: None,
                attachments: None,
                direction: MessageDirection::Inbound,
                role: MessageRole::Human,
                thread_index: None,
                created_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
        let progress = persistence
            .record_outreach_reply(&matched, response.id)
            .await
            .unwrap();
        assert_eq!(progress.status, OutreachStatus::ThresholdMet);
        assert_eq!(progress.response_count, 1);
        assert_eq!(
            persistence
                .get_task_by_id(task.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::Pending
        );
        let replied = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap()
            .into_iter()
            .find(|event| event.reason == TaskTransitionReason::OutreachReplyReceived)
            .expect("outreach reply records the exact resumption reason");
        assert_eq!(replied.related_outreach_id, Some(outreach_id));

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    /// A company and an enabled channel to hang tasks off, with a unique slug per call so
    /// database-backed tests do not collide with each other or with a previous run.
    async fn seed_company_and_channel(
        persistence: &PostgresPersistence,
    ) -> (
        crate::entities::company::Company,
        crate::entities::channel::Channel,
    ) {
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("chain_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            persistence,
            owner.id,
            CompanyWrite {
                name: "Chain Test".to_string(),
                slug: format!("chain-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            persistence,
            company.id,
            ChannelWrite {
                name: "Chain".into(),
                slug: "chain".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        (company, channel)
    }

    /// Move a chain's tasks back beyond the board's window.
    ///
    /// Only `updated_at` moves. The status-event trigger fires on `status`, so the rows end up
    /// looking exactly like work that has genuinely been idle for a fortnight, with no ledger
    /// entry claiming something happened to them.
    async fn age_chain_tasks(pool: &sqlx::PgPool, correlation_id: CorrelationId) {
        sqlx::query("UPDATE background_tasks SET updated_at = $1 WHERE correlation_id = $2")
            .bind(Utc::now() - chrono::Duration::days(30))
            .bind(correlation_id.as_uuid())
            .execute(pool)
            .await
            .unwrap();
    }

    async fn age_chain_deliveries(pool: &sqlx::PgPool, correlation_id: CorrelationId) {
        sqlx::query("UPDATE email_outbox SET updated_at = $1 WHERE correlation_id = $2")
            .bind(Utc::now() - chrono::Duration::days(30))
            .bind(correlation_id.as_uuid())
            .execute(pool)
            .await
            .unwrap();
    }

    /// The board's cards as one chain-selection produces them, in an order two runs can be
    /// compared in.
    async fn board_cards(
        pool: &sqlx::PgPool,
        sql: &str,
        company_id: Uuid,
        filter: TaskBoardFilter,
    ) -> Vec<TaskChainCard> {
        let mut cards = sqlx::query_as::<_, TaskChainCardDb>(sql)
            .bind(company_id)
            .bind(filter.channel_id)
            .bind(filter.terminal_since)
            .bind(filter.per_column_limit as i64)
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| <(TaskChainCard, i64)>::try_from(row).unwrap().0)
            .collect::<Vec<_>>();
        cards.sort_by_key(|card| card.correlation_id.as_uuid());
        cards
    }

    async fn enqueue_chain(
        persistence: &PostgresPersistence,
        company_id: Uuid,
        channel_id: Uuid,
        task_type: &str,
    ) -> BackgroundTask {
        persistence
            .enqueue_task(NewTask::starting_new_chain(
                company_id,
                channel_id,
                None,
                task_type,
                serde_json::json!({}),
            ))
            .await
            .unwrap()
    }

    /// Take the lease a fenced write needs, the way the worker does.
    async fn claim(persistence: &PostgresPersistence, task_id: Uuid) -> TaskLeaseRef {
        assert!(
            persistence
                .claim_task(
                    task_id,
                    Uuid::new_v4(),
                    Utc::now() + chrono::Duration::minutes(5)
                )
                .await
                .unwrap()
        );
        let claimed = persistence.get_task_by_id(task_id).await.unwrap().unwrap();
        TaskLeaseRef::of(&claimed).unwrap()
    }

    #[tokio::test]
    async fn board_window_pushdown_selects_the_same_chains_as_the_aggregate_filter() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let (company, channel) = seed_company_and_channel(&persistence).await;

        // Every branch the board's selection has, each on its own chain.
        let active = enqueue_chain(&persistence, company.id, channel.id, "active").await;

        let unresolved = enqueue_chain(&persistence, company.id, channel.id, "unresolved").await;
        let lease = claim(&persistence, unresolved.id).await;
        persistence
            .mark_task_failed(lease, "gave up", Utc::now(), true)
            .await
            .unwrap();
        // Unresolved work is selected however old it is — that is the point of the status arm.
        age_chain_tasks(&pool, unresolved.correlation_id).await;

        let stopped_recent =
            enqueue_chain(&persistence, company.id, channel.id, "stopped-in").await;
        persistence.stop_task(stopped_recent.id).await.unwrap();

        let stopped_old = enqueue_chain(&persistence, company.id, channel.id, "stopped-out").await;
        persistence.stop_task(stopped_old.id).await.unwrap();
        age_chain_tasks(&pool, stopped_old.correlation_id).await;

        let completed_recent =
            enqueue_chain(&persistence, company.id, channel.id, "completed-in").await;
        let lease = claim(&persistence, completed_recent.id).await;
        persistence.mark_task_completed(lease).await.unwrap();

        let completed_old =
            enqueue_chain(&persistence, company.id, channel.id, "completed-out").await;
        let lease = claim(&persistence, completed_old.id).await;
        persistence.mark_task_completed(lease).await.unwrap();
        age_chain_tasks(&pool, completed_old.correlation_id).await;

        // Finished, aged-out work whose only recent trace is a delivery. The pre-pushdown query
        // caught this one only incidentally, through the aggregate.
        let delivery_recent =
            enqueue_chain(&persistence, company.id, channel.id, "delivery-in").await;
        let lease = claim(&persistence, delivery_recent.id).await;
        persistence.mark_task_completed(lease).await.unwrap();
        let delivered_recently =
            queue_one_email(&persistence, &delivery_recent, "delivery-in").await;
        mark_delivered(&pool, delivered_recently).await;
        age_chain_tasks(&pool, delivery_recent.correlation_id).await;

        // The control for that arm: same shape, but the delivery is old too.
        let delivery_old =
            enqueue_chain(&persistence, company.id, channel.id, "delivery-out").await;
        let lease = claim(&persistence, delivery_old.id).await;
        persistence.mark_task_completed(lease).await.unwrap();
        let delivered_long_ago = queue_one_email(&persistence, &delivery_old, "delivery-out").await;
        mark_delivered(&pool, delivered_long_ago).await;
        age_chain_tasks(&pool, delivery_old.correlation_id).await;
        age_chain_deliveries(&pool, delivery_old.correlation_id).await;

        let filter = TaskBoardFilter::new(None, Utc::now());
        let pushdown = board_cards(&pool, &BOARD_QUERY, company.id, filter).await;
        let control = board_cards(
            &pool,
            &board_query_sql(BOARD_ELIGIBLE_EVERY_CHAIN),
            company.id,
            filter,
        )
        .await;
        assert_eq!(
            pushdown, control,
            "the row-level selection and the aggregate filter must pick the same chains"
        );

        // Agreement alone would also be satisfied by both being wrong, so pin the membership too.
        let mut selected = pushdown
            .iter()
            .map(|card| card.correlation_id)
            .collect::<Vec<_>>();
        selected.sort_by_key(|id| id.as_uuid());
        let mut expected = vec![
            active.correlation_id,
            unresolved.correlation_id,
            stopped_recent.correlation_id,
            completed_recent.correlation_id,
            delivery_recent.correlation_id,
        ];
        expected.sort_by_key(|id| id.as_uuid());
        assert_eq!(selected, expected);
        assert!(!selected.contains(&stopped_old.correlation_id));
        assert!(!selected.contains(&completed_old.correlation_id));
        assert!(!selected.contains(&delivery_old.correlation_id));

        // The channel filter is applied on top of the row-level selection, not instead of it.
        let other_channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Elsewhere".into(),
                slug: "elsewhere".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let filtered = TaskBoardFilter::new(Some(other_channel.id), Utc::now());
        assert!(
            board_cards(&pool, &BOARD_QUERY, company.id, filtered)
                .await
                .is_empty()
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    async fn queue_one_email(
        persistence: &PostgresPersistence,
        task: &BackgroundTask,
        key: &str,
    ) -> Uuid {
        persistence
            .enqueue_outbound_send(OutboundSend {
                company_id: task.company_id,
                channel_id: task.channel_id,
                task_id: Some(task.id),
                correlation_id: task.correlation_id,
                idempotency_key: format!("{key}-{}", Uuid::new_v4()),
                payload: serde_json::json!({"to": ["someone@example.com"]}),
            })
            .await
            .unwrap()
            .unwrap()
    }

    /// Land one queued email as delivered.
    ///
    /// Written directly rather than through `claim_outbox_emails`, which claims from the whole
    /// table: these tests share a database with everything else running in parallel, and a claim
    /// would take rows they do not own.
    async fn mark_delivered(pool: &sqlx::PgPool, outbox_id: Uuid) {
        let updated = sqlx::query(
            "UPDATE email_outbox
                SET status = 'sent', sent_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
              WHERE id = $1",
        )
        .bind(outbox_id)
        .execute(pool)
        .await
        .unwrap();
        assert_eq!(updated.rows_affected(), 1);
    }

    #[tokio::test]
    async fn chain_detail_attaches_every_attempt_and_delivery_to_its_own_task() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let (company, channel) = seed_company_and_channel(&persistence).await;

        let first = enqueue_chain(&persistence, company.id, channel.id, "grouped-first").await;
        let mut tasks = vec![first.clone()];
        for index in 1..3 {
            tasks.push(
                persistence
                    .enqueue_task(NewTask {
                        company_id: company.id,
                        channel_id: channel.id,
                        thread_id: None,
                        task_type: format!("grouped-{index}"),
                        payload: serde_json::json!({}),
                        correlation_id: first.correlation_id,
                    })
                    .await
                    .unwrap(),
            );
        }

        // Task n gets n attempts and n deliveries, so a grouping that loses the association shows
        // up as a wrong count rather than only as a wrong order.
        for (index, task) in tasks.iter().enumerate() {
            let expected = index + 1;
            for attempt_number in 1..=expected as i32 {
                let attempt = TaskAttemptRef {
                    task_id: task.id,
                    attempt_number,
                    execution_generation: Uuid::new_v4(),
                };
                persistence.begin_task_attempt(attempt).await.unwrap();
            }
            for _ in 0..expected {
                queue_one_email(&persistence, task, "grouped").await;
            }
        }

        let detail = persistence
            .get_task_chain_detail(company.id, first.correlation_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!detail.truncated);
        assert_eq!(detail.tasks.len(), 3);
        for (index, item) in detail.tasks.iter().enumerate() {
            let expected = index + 1;
            assert_eq!(item.task.id, tasks[index].id);
            assert_eq!(item.attempts.len(), expected, "attempts for task {index}");
            assert_eq!(
                item.deliveries.len(),
                expected,
                "deliveries for task {index}"
            );
            assert!(
                item.deliveries
                    .iter()
                    .all(|delivery| delivery.task_id == Some(item.task.id))
            );
            assert_eq!(
                item.attempts
                    .iter()
                    .map(|attempt| attempt.attempt_number)
                    .collect::<Vec<_>>(),
                (1..=expected as i32).collect::<Vec<_>>()
            );
        }

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    /// One chain with `count` extra tasks alongside the one it starts from.
    ///
    /// Bulk-inserted in a single statement: these fixtures cross limits in the low thousands, and
    /// a round trip per row would make the test slower than everything else in the suite put
    /// together.
    async fn bulk_tasks(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
        sqlx::query(
            "INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type)
             SELECT gen_random_uuid(), $1, $2, $3, 'bulk-' || series
               FROM generate_series(1, $4::int) AS series",
        )
        .bind(task.company_id)
        .bind(task.channel_id)
        .bind(task.correlation_id.as_uuid())
        .bind(count)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn bulk_attempts(pool: &sqlx::PgPool, task_id: Uuid, count: i32) {
        sqlx::query(
            "INSERT INTO task_attempts (id, task_id, attempt_number, status, execution_generation)
             SELECT gen_random_uuid(), $1, series, 'completed', gen_random_uuid()
               FROM generate_series(1, $2::int) AS series",
        )
        .bind(task_id)
        .bind(count)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn bulk_deliveries(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
        sqlx::query(
            "INSERT INTO email_outbox
                 (id, company_id, task_id, correlation_id, idempotency_key, payload, status)
             SELECT gen_random_uuid(), $1, $2, $3, $4 || '-' || series, '{}'::jsonb, 'sent'
               FROM generate_series(1, $5::int) AS series",
        )
        .bind(task.company_id)
        .bind(task.id)
        .bind(task.correlation_id.as_uuid())
        .bind(Uuid::new_v4().to_string())
        .bind(count)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Extra ledger rows past the one the enqueue already wrote, hence the sequence offset.
    async fn bulk_status_events(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
        sqlx::query(
            "INSERT INTO task_status_events
                 (id, company_id, task_id, correlation_id, sequence, to_status, reason,
                  actor_kind, retry_count, run_at)
             SELECT gen_random_uuid(), $1, $2, $3, series + 100, 'pending', 'enqueued',
                    'system', 0, CURRENT_TIMESTAMP
               FROM generate_series(1, $4::int) AS series",
        )
        .bind(task.company_id)
        .bind(task.id)
        .bind(task.correlation_id.as_uuid())
        .bind(count)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn bulk_approvals(pool: &sqlx::PgPool, task: &BackgroundTask, count: i32) {
        sqlx::query(
            "INSERT INTO human_approvals
                 (id, company_id, channel_id, task_id, step_key, approver_email, action_type,
                  action_title, action_summary, token, expires_at)
             SELECT gen_random_uuid(), $1, $2, $3, $4 || '-' || series, 'approver@example.com',
                    'send', 'Bulk approval', 'Bulk summary', gen_random_uuid(),
                    CURRENT_TIMESTAMP + interval '1 day'
               FROM generate_series(1, $5::int) AS series",
        )
        .bind(task.company_id)
        .bind(task.channel_id)
        .bind(task.id)
        .bind(Uuid::new_v4().to_string())
        .bind(count)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn bulk_outreaches(pool: &sqlx::PgPool, task_id: Uuid, count: i32) {
        sqlx::query(
            "INSERT INTO task_outreaches
                 (id, task_id, status, required_threshold_percent, expires_at, outreach_key,
                  subject, body)
             SELECT gen_random_uuid(), $1, 'waiting', 50,
                    CURRENT_TIMESTAMP + interval '1 day', $2 || '-' || series, 'Subject', 'Body'
               FROM generate_series(1, $3::int) AS series",
        )
        .bind(task_id)
        .bind(Uuid::new_v4().to_string())
        .bind(count)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn chain_detail_bounds_every_collection_and_reports_the_truncation() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let (company, channel) = seed_company_and_channel(&persistence).await;
        let detail_of = async |correlation_id| {
            persistence
                .get_task_chain_detail(company.id, correlation_id)
                .await
                .unwrap()
                .unwrap()
        };

        // Tasks. The 201st task also pushes the chain past the event limit, since every enqueue
        // writes one ledger row — which is exactly why `truncated` is one flag and not six.
        let many_tasks = enqueue_chain(&persistence, company.id, channel.id, "limit-tasks").await;
        bulk_tasks(&pool, &many_tasks, CHAIN_DETAIL_MAX_TASKS as i32).await;
        let detail = detail_of(many_tasks.correlation_id).await;
        assert!(detail.truncated);
        assert_eq!(detail.tasks.len(), CHAIN_DETAIL_MAX_TASKS as usize);

        // Attempts, one past the limit.
        let many_attempts =
            enqueue_chain(&persistence, company.id, channel.id, "limit-attempts").await;
        bulk_attempts(
            &pool,
            many_attempts.id,
            CHAIN_DETAIL_MAX_ATTEMPTS as i32 + 1,
        )
        .await;
        let detail = detail_of(many_attempts.correlation_id).await;
        assert!(detail.truncated);
        assert_eq!(
            detail.tasks[0].attempts.len(),
            CHAIN_DETAIL_MAX_ATTEMPTS as usize
        );

        // Attempts, exactly at the limit. A full page is not a truncated one, and only the
        // sentinel row can tell the two apart.
        let full_attempts =
            enqueue_chain(&persistence, company.id, channel.id, "limit-attempts-exact").await;
        bulk_attempts(&pool, full_attempts.id, CHAIN_DETAIL_MAX_ATTEMPTS as i32).await;
        let detail = detail_of(full_attempts.correlation_id).await;
        assert!(!detail.truncated);
        assert_eq!(
            detail.tasks[0].attempts.len(),
            CHAIN_DETAIL_MAX_ATTEMPTS as usize
        );

        // Deliveries.
        let many_deliveries =
            enqueue_chain(&persistence, company.id, channel.id, "limit-deliveries").await;
        bulk_deliveries(
            &pool,
            &many_deliveries,
            CHAIN_DETAIL_MAX_DELIVERIES as i32 + 1,
        )
        .await;
        let detail = detail_of(many_deliveries.correlation_id).await;
        assert!(detail.truncated);
        assert_eq!(
            detail.tasks[0].deliveries.len(),
            CHAIN_DETAIL_MAX_DELIVERIES as usize
        );

        // Events: the enqueue wrote one, so the limit is crossed by the limit's worth on top.
        let many_events = enqueue_chain(&persistence, company.id, channel.id, "limit-events").await;
        bulk_status_events(&pool, &many_events, CHAIN_DETAIL_MAX_EVENTS as i32).await;
        let detail = detail_of(many_events.correlation_id).await;
        assert!(detail.truncated);
        assert_eq!(detail.events.len(), CHAIN_DETAIL_MAX_EVENTS as usize);

        // Approvals.
        let many_approvals =
            enqueue_chain(&persistence, company.id, channel.id, "limit-approvals").await;
        bulk_approvals(
            &pool,
            &many_approvals,
            CHAIN_DETAIL_MAX_APPROVALS as i32 + 1,
        )
        .await;
        let detail = detail_of(many_approvals.correlation_id).await;
        assert!(detail.truncated);
        assert_eq!(detail.approvals.len(), CHAIN_DETAIL_MAX_APPROVALS as usize);

        // Outreaches.
        let many_outreaches =
            enqueue_chain(&persistence, company.id, channel.id, "limit-outreaches").await;
        bulk_outreaches(
            &pool,
            many_outreaches.id,
            CHAIN_DETAIL_MAX_OUTREACHES as i32 + 1,
        )
        .await;
        let detail = detail_of(many_outreaches.correlation_id).await;
        assert!(detail.truncated);
        assert_eq!(
            detail.outreaches.len(),
            CHAIN_DETAIL_MAX_OUTREACHES as usize
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[test]
    fn a_full_page_is_not_a_truncated_one() {
        let mut exactly_full = (0..3).collect::<Vec<_>>();
        assert!(!trim_to_limit(&mut exactly_full, 3));
        assert_eq!(exactly_full.len(), 3);

        let mut one_over = (0..probe_limit(3)).collect::<Vec<_>>();
        assert!(trim_to_limit(&mut one_over, 3));
        assert_eq!(one_over, vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn a_status_write_that_changes_nothing_wakes_no_board() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let (company, channel) = seed_company_and_channel(&persistence).await;

        // Everything is in place before the listener attaches, so the only notifications it can
        // see are the ones this test provokes.
        let quiet = enqueue_chain(&persistence, company.id, channel.id, "quiet").await;
        let outbox_id = queue_one_email(&persistence, &quiet, "quiet").await;
        bulk_approvals(&pool, &quiet, 1).await;
        bulk_outreaches(&pool, quiet.id, 1).await;
        let fence = enqueue_chain(&persistence, company.id, channel.id, "fence").await;
        let fence_outbox = queue_one_email(&persistence, &fence, "fence").await;

        let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
            .await
            .unwrap();
        listener.listen("task_chain_changed").await.unwrap();

        // `UPDATE OF status` fires whenever the column is in the SET list, value unchanged or not.
        for statement in [
            "UPDATE email_outbox SET status = status, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
            "UPDATE human_approvals SET status = status, updated_at = CURRENT_TIMESTAMP
              WHERE task_id = $1",
            "UPDATE task_outreaches SET status = status, updated_at = CURRENT_TIMESTAMP
              WHERE task_id = $1",
        ] {
            let target = if statement.contains("email_outbox") {
                outbox_id
            } else {
                quiet.id
            };
            let affected = sqlx::query(statement)
                .bind(target)
                .execute(&pool)
                .await
                .unwrap();
            assert_eq!(affected.rows_affected(), 1, "no-op write must hit its row");
        }

        // A real change on another chain, committed after all three no-ops. Notifications arrive
        // in commit order on one connection, so reaching this one means the no-ops sent nothing.
        mark_delivered(&pool, fence_outbox).await;

        let quiet_wakes =
            drain_chain_notifications(&mut listener, company.id, fence.correlation_id)
                .await
                .into_iter()
                .filter(|id| *id == quiet.correlation_id.as_uuid())
                .count();
        assert_eq!(
            quiet_wakes, 0,
            "no-op status writes must not wake the board"
        );

        // The same three tables, actually changing status, still do wake it.
        mark_delivered(&pool, outbox_id).await;
        for statement in [
            "UPDATE human_approvals SET status = 'approved' WHERE task_id = $1",
            "UPDATE task_outreaches SET status = 'completed' WHERE task_id = $1",
        ] {
            sqlx::query(statement)
                .bind(quiet.id)
                .execute(&pool)
                .await
                .unwrap();
        }
        mark_delivered(
            &pool,
            queue_one_email(&persistence, &fence, "fence-2").await,
        )
        .await;
        let real_wakes = drain_chain_notifications(&mut listener, company.id, fence.correlation_id)
            .await
            .into_iter()
            .filter(|id| *id == quiet.correlation_id.as_uuid())
            .count();
        assert_eq!(
            real_wakes, 3,
            "outbox, approval and outreach each moved status"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    /// Every chain notification this company emitted up to and including `until`.
    ///
    /// The fence is what makes a negative assertion possible: waiting for a notification that
    /// should never arrive can only ever be a timeout, whereas waiting for one that must arrive
    /// after it proves the earlier writes had their chance.
    async fn drain_chain_notifications(
        listener: &mut sqlx::postgres::PgListener,
        company_id: Uuid,
        until: CorrelationId,
    ) -> Vec<Uuid> {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut seen = Vec::new();
            loop {
                let notification = listener.recv().await.unwrap();
                let payload: serde_json::Value =
                    serde_json::from_str(notification.payload()).unwrap();
                if payload["company_id"] != company_id.to_string() {
                    continue;
                }
                let correlation_id =
                    Uuid::parse_str(payload["correlation_id"].as_str().unwrap()).unwrap();
                if correlation_id == until.as_uuid() {
                    return seen;
                }
                seen.push(correlation_id);
            }
        })
        .await
        .expect("the fencing notification arrives")
    }

    #[tokio::test]
    async fn task_chain_notifications_contain_identifiers_only() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let (company, channel) = seed_company_and_channel(&persistence).await;
        let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
            .await
            .unwrap();
        listener.listen("task_chain_changed").await.unwrap();

        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "notification-test",
                serde_json::json!({"body": "must never be notified", "token": "secret"}),
            ))
            .await
            .unwrap();
        let payload = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let notification = listener.recv().await.unwrap();
                let payload: serde_json::Value =
                    serde_json::from_str(notification.payload()).unwrap();
                if payload["company_id"] == company.id.to_string()
                    && payload["correlation_id"] == task.correlation_id.to_string()
                {
                    break payload;
                }
            }
        })
        .await
        .expect("task-chain notification arrives");
        assert_eq!(payload.as_object().unwrap().len(), 2);
        assert_eq!(payload["company_id"], company.id.to_string());
        assert_eq!(payload["correlation_id"], task.correlation_id.to_string());
        assert!(payload.get("body").is_none());
        assert!(payload.get("token").is_none());

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn worker_outcomes_record_exact_transition_reasons() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let (company, channel) = seed_company_and_channel(&persistence).await;
        let cases = [
            (TaskStopReason::RetryableFailure, false),
            (TaskStopReason::TimedOut, false),
            (TaskStopReason::Shutdown, false),
            (TaskStopReason::TerminalFailure, true),
        ];
        for (stop_reason, dead_letter) in cases {
            let task = persistence
                .enqueue_task(NewTask::starting_new_chain(
                    company.id,
                    channel.id,
                    None,
                    format!("reason-{}", stop_reason.as_str()),
                    serde_json::json!({}),
                ))
                .await
                .unwrap();
            let worker_id = Uuid::new_v4();
            assert!(
                persistence
                    .claim_task(
                        task.id,
                        worker_id,
                        Utc::now() + chrono::Duration::minutes(5),
                    )
                    .await
                    .unwrap()
            );
            let claimed = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
            let lease = TaskLeaseRef::of(&claimed).unwrap();
            assert!(
                persistence
                    .mark_task_failed_with_reason(
                        lease,
                        "bounded test failure",
                        Utc::now() + chrono::Duration::minutes(1),
                        dead_letter,
                        stop_reason,
                    )
                    .await
                    .unwrap()
            );
            let expected = match stop_reason {
                TaskStopReason::RetryableFailure => TaskTransitionReason::RetryableFailure,
                TaskStopReason::TimedOut => TaskTransitionReason::TimedOut,
                TaskStopReason::Shutdown => TaskTransitionReason::Shutdown,
                TaskStopReason::TerminalFailure => TaskTransitionReason::TerminalFailure,
                _ => unreachable!(),
            };
            let events = persistence
                .list_task_status_events(company.id, task.correlation_id, None, 20)
                .await
                .unwrap();
            assert!(events.iter().any(|event| event.reason == expected));
        }

        let completed = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "reason-completed",
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        let worker_id = Uuid::new_v4();
        persistence
            .claim_task(
                completed.id,
                worker_id,
                Utc::now() + chrono::Duration::minutes(5),
            )
            .await
            .unwrap();
        let claimed = persistence
            .get_task_by_id(completed.id)
            .await
            .unwrap()
            .unwrap();
        persistence
            .mark_task_completed(TaskLeaseRef::of(&claimed).unwrap())
            .await
            .unwrap();
        let events = persistence
            .list_task_status_events(company.id, completed.correlation_id, None, 20)
            .await
            .unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.reason == TaskTransitionReason::Completed)
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn task_attempt_history_is_ordered_and_company_scoped() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let (company, channel) = seed_company_and_channel(&persistence).await;
        let (other_company, _) = seed_company_and_channel(&persistence).await;
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "attempt-history",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        for attempt_number in [1, 2] {
            let attempt = TaskAttemptRef {
                task_id: task.id,
                attempt_number,
                execution_generation: Uuid::new_v4(),
            };
            persistence.begin_task_attempt(attempt).await.unwrap();
            persistence
                .finish_task_attempt(&TaskAttemptOutcome {
                    attempt,
                    status: if attempt_number == 1 {
                        TaskAttemptStatus::Failed
                    } else {
                        TaskAttemptStatus::Completed
                    },
                    stop_reason: if attempt_number == 1 {
                        TaskStopReason::RetryableFailure
                    } else {
                        TaskStopReason::Completed
                    },
                    error: (attempt_number == 1).then(|| "retry me".to_string()),
                    tokens: Some(TokenUsage::new(
                        attempt_number as usize * 10,
                        attempt_number as usize * 2,
                    )),
                })
                .await
                .unwrap();
        }

        let attempts = persistence
            .list_task_attempts(company.id, task.id)
            .await
            .unwrap();
        assert_eq!(
            attempts
                .iter()
                .map(|attempt| attempt.attempt_number)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(attempts[0].status, TaskAttemptRecordStatus::Failed);
        assert_eq!(
            attempts[0].stop_reason,
            Some(TaskStopReason::RetryableFailure)
        );
        assert_eq!(attempts[0].total_tokens(), Some(12));
        assert!(attempts[0].duration_ms().is_some());
        assert!(
            persistence
                .list_task_attempts(other_company.id, task.id)
                .await
                .unwrap()
                .is_empty(),
            "another company cannot read a guessed task's attempt history"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
        CompanyPersistence::delete(&persistence, other_company.id)
            .await
            .unwrap();
    }

    /// The whole point of the correlation id: one query returns the trail.
    #[tokio::test]
    async fn a_chain_is_inherited_by_children_and_readable_in_one_query() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let (company, channel) = seed_company_and_channel(&persistence).await;

        // The inbound message mints one chain; a second, unrelated message mints another.
        let chain = CorrelationId::new();
        let other_chain = CorrelationId::new();

        let parent = persistence
            .enqueue_task(NewTask {
                company_id: company.id,
                channel_id: channel.id,
                thread_id: None,
                task_type: "email_agent_dispatch".to_string(),
                payload: serde_json::json!({}),
                correlation_id: chain,
            })
            .await
            .unwrap();
        assert_eq!(parent.correlation_id, chain, "the chain round-trips");

        // A task the run spawns -- an outreach into another channel, say -- inherits it.
        let child = persistence
            .enqueue_task(NewTask::caused_by(
                &parent,
                channel.id,
                None,
                "email_agent_dispatch",
                serde_json::json!({ "spawned": true }),
            ))
            .await
            .unwrap();
        assert_eq!(child.correlation_id, chain);

        let unrelated = persistence
            .enqueue_task(NewTask {
                company_id: company.id,
                channel_id: channel.id,
                thread_id: None,
                task_type: "email_agent_dispatch".to_string(),
                payload: serde_json::json!({ "unrelated": true }),
                correlation_id: other_chain,
            })
            .await
            .unwrap();

        // The email the run sends carries the chain too, so the outbound leg is on the trail.
        persistence
            .enqueue_outbound_send(OutboundSend {
                company_id: company.id,
                channel_id: channel.id,
                task_id: Some(parent.id),
                correlation_id: chain,
                idempotency_key: format!("chain-test:{}", parent.id),
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();

        let task_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM background_tasks WHERE correlation_id = $1 ORDER BY created_at",
        )
        .bind(chain.as_uuid())
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(task_ids.contains(&parent.id));
        assert!(task_ids.contains(&child.id));
        assert!(
            !task_ids.contains(&unrelated.id),
            "a different message must not join this chain"
        );

        let sent: i64 =
            sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE correlation_id = $1")
                .bind(chain.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(sent, 1, "the outbound leg is on the same trail");
    }

    /// A redelivered message must not fork the chain its first delivery started.
    #[tokio::test]
    async fn a_redelivered_message_rejoins_its_original_chain() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let (company, channel) = seed_company_and_channel(&persistence).await;

        let message_id = format!("<redelivery-{}@example.com>", Uuid::new_v4());
        let payload = serde_json::json!({ "inbound_message": { "message_id": message_id } });
        let first_chain = CorrelationId::new();

        let first = persistence
            .enqueue_task(NewTask {
                company_id: company.id,
                channel_id: channel.id,
                thread_id: None,
                task_type: "email_agent_dispatch".to_string(),
                payload: payload.clone(),
                correlation_id: first_chain,
            })
            .await
            .unwrap();

        // The same message arrives again and is given a fresh id by ingress.
        let second = persistence
            .enqueue_task(NewTask {
                company_id: company.id,
                channel_id: channel.id,
                thread_id: None,
                task_type: "email_agent_dispatch".to_string(),
                payload,
                correlation_id: CorrelationId::new(),
            })
            .await
            .unwrap();

        assert_eq!(
            second.id, first.id,
            "the duplicate returns the original task"
        );
        assert_eq!(
            second.correlation_id, first_chain,
            "and keeps the chain it already had"
        );
    }

    /// The census must see each kind of stall.
    ///
    /// Asserted as "at least ours" rather than as an exact delta: the suite shares one database
    /// and runs in parallel, so other tests are planting and completing tasks throughout. The
    /// counting *logic* is pinned by the unit tests on [`StuckWorkCensus`]; what this checks is
    /// that the SQL classifies a real row into the right bucket at all.
    #[tokio::test]
    async fn the_census_sees_each_kind_of_stuck_work() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let (company, channel) = seed_company_and_channel(&persistence).await;
        let thresholds = StuckWorkThresholds::default();

        let dead = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "census-dead",
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        sqlx::query("UPDATE background_tasks SET status = 'dead_letter' WHERE id = $1")
            .bind(dead.id)
            .execute(&pool)
            .await
            .unwrap();

        let overdue = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "census-overdue",
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP - interval '1 hour' \
             WHERE id = $1",
        )
        .bind(overdue.id)
        .execute(&pool)
        .await
        .unwrap();

        let census = persistence.census_stuck_work(thresholds).await.unwrap();
        assert!(
            census.dead_lettered >= 1,
            "the dead-lettered task is counted"
        );
        assert!(census.queue_overdue >= 1, "the overdue task is counted");
        assert!(!census.is_quiet());

        // The discriminating case, which is safe to assert exactly because it is about one row:
        // a task queued to run now is not overdue, so flipping only `run_at` back must drop it
        // out of the bucket again.
        sqlx::query("UPDATE background_tasks SET run_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(overdue.id)
            .execute(&pool)
            .await
            .unwrap();
        let still_overdue: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM background_tasks \
             WHERE id = $1 AND status = 'pending' \
               AND run_at < CURRENT_TIMESTAMP - $2::interval",
        )
        .bind(overdue.id)
        .bind(PgInterval::try_from(thresholds.queue_overdue_after()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(still_overdue, 0, "a task due now is not stuck");
    }
}
