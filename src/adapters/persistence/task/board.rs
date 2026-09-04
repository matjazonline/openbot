//! The correlation-chain read model: the Kanban board projection and the chain detail pane.
//!
//! Both are HTTP reads rather than steps in the worker's dispatch chain, so their bodies live here
//! as free functions and the trait impl forwards to them.

use chrono::{DateTime, Utc};
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tracing::warn;
use uuid::Uuid;

use super::*;
use crate::adapters::persistence::delivery::read::deliveries_for_tasks;
use crate::app_error::{AppError, AppResult};
use crate::entities::{
    correlation::CorrelationId,
    task::{
        ChainStage, TaskApprovalContext, TaskAttemptRecord, TaskBoardFilter, TaskChainBoard,
        TaskChainDetail, TaskChainTaskDetail, TaskOutreachContext, TaskStatusEvent,
        TaskStatusEventCursor,
    },
};

/// Which chains the board considers, selected one row at a time.
///
/// Every predicate in `staged` is an aggregate over `GROUP BY correlation_id`, and Postgres cannot
/// push an aggregate predicate below its own grouping — so on its own the window filter runs
/// *after* the scan it was meant to bound, over every task the company has ever run. These are the
/// same three conditions written at row level, where an index can serve them: unfinished work,
/// unresolved work, or anything touched since the cutoff. The delivery arm also closes a real gap —
/// a chain whose only recent activity is a delivery was previously selected only incidentally.
pub(crate) const BOARD_ELIGIBLE_RECENT: &str = r#"
                   SELECT correlation_id FROM (
                       SELECT correlation_id
                       FROM background_tasks
                       WHERE company_id = $1
                         AND (status IN ('pending', 'processing', 'pending_approval',
                                         'waiting_for_third_party_reply', 'failed', 'dead_letter')
                              OR updated_at >= $3)
                       UNION
                       SELECT correlation_id
                       FROM message_deliveries
                       WHERE company_id = $1
                         AND (status IN ('pending', 'sending', 'retryable',
                                         'outcome_unknown', 'dead_letter')
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
pub(crate) const BOARD_ELIGIBLE_EVERY_CHAIN: &str = r#"
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

/// The SQL representation of the board's stage precedence.
///
/// The board needs `stage` as a real column -- `PARTITION BY stage` drives both the per-column
/// `ROW_NUMBER()` limit and the `stage_total` count -- so the rule cannot move wholesale into Rust,
/// and SQL on the domain type would point the dependency the wrong way. So it exists twice, once
/// in each owning layer. `chain_stage_sql_matches_rust_derivation` pushes a matrix of counts
/// through this expression and compares it with [`ChainStage::derive`]; that test, not a
/// `debug_assert`, is what keeps the two rung-for-rung identical in a release build.
pub(crate) const CHAIN_STAGE_SQL_CASE: &str = r#"CASE
                              WHEN failed > 0 OR dead_letter > 0 OR stopped > 0
                                   OR expired_processing > 0 OR delivery_unresolved > 0
                                  THEN 'needs_attention'
                              WHEN pending_approval > 0 THEN 'waiting_approval'
                              WHEN processing > 0 OR delivery_sending > 0 THEN 'running'
                              WHEN waiting_reply > 0 THEN 'waiting_reply'
                              WHEN pending > 0 OR delivery_queued > 0 THEN 'queued'
                              WHEN total_tasks > 0 AND completed = total_tasks
                                   AND delivery_delivered = total_deliveries THEN 'completed'
                              ELSE 'needs_attention'
                          END"#;

/// The board projection over whichever chain selection it is given.
///
/// The selection is a parameter so the equivalence test can drive the identical projection from
/// [`BOARD_ELIGIBLE_EVERY_CHAIN`]; production always assembles it once from
/// [`BOARD_ELIGIBLE_RECENT`], in [`BOARD_QUERY`].
pub(crate) fn board_query_sql(eligible: &str) -> String {
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
                   SELECT delivery.correlation_id,
                          COUNT(*)::bigint AS total_deliveries,
                          COUNT(*) FILTER (
                              WHERE delivery.status IN ('pending', 'retryable')
                          )::bigint AS delivery_queued,
                          COUNT(*) FILTER (WHERE delivery.status = 'sending')::bigint
                              AS delivery_sending,
                          COUNT(*) FILTER (WHERE delivery.status = 'delivered')::bigint
                              AS delivery_delivered,
                          -- Dead letters and unconfirmed outcomes together: both need a human,
                          -- and a chain holding either did not finish.
                          COUNT(*) FILTER (
                              WHERE delivery.status IN ('dead_letter', 'outcome_unknown')
                          )::bigint AS delivery_unresolved,
                          MAX(delivery.updated_at) AS delivery_last_activity,
                          MIN(delivery.available_at) FILTER (
                              WHERE delivery.status IN ('pending', 'retryable')
                          ) AS delivery_next_action
                   FROM message_deliveries AS delivery
                   JOIN eligible ON eligible.correlation_id = delivery.correlation_id
                   WHERE delivery.company_id = $1
                   GROUP BY delivery.correlation_id
               ),
               combined AS (
                   SELECT task_rollup.correlation_id, task_rollup.title,
                          participant_rollup.channel_names, participant_rollup.agent_names,
                          task_rollup.total_tasks, task_rollup.pending, task_rollup.processing,
                          task_rollup.expired_processing, task_rollup.pending_approval,
                          task_rollup.waiting_reply, task_rollup.completed, task_rollup.failed,
                          task_rollup.dead_letter, task_rollup.stopped,
                          COALESCE(delivery_rollup.total_deliveries, 0) AS total_deliveries,
                          COALESCE(delivery_rollup.delivery_queued, 0) AS delivery_queued,
                          COALESCE(delivery_rollup.delivery_sending, 0) AS delivery_sending,
                          COALESCE(delivery_rollup.delivery_delivered, 0) AS delivery_delivered,
                          COALESCE(delivery_rollup.delivery_unresolved, 0) AS delivery_unresolved,
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
                              + COALESCE(delivery_rollup.delivery_queued, 0)
                              + COALESCE(delivery_rollup.delivery_sending, 0)) > 0 AS is_active,
                          (task_rollup.failed + task_rollup.dead_letter
                              + task_rollup.expired_processing
                              + COALESCE(delivery_rollup.delivery_unresolved, 0)) > 0 AS is_unresolved
                   FROM task_rollup
                   JOIN participant_rollup
                     ON participant_rollup.correlation_id = task_rollup.correlation_id
                   LEFT JOIN delivery_rollup
                     ON delivery_rollup.correlation_id = task_rollup.correlation_id
               ),
               staged AS (
                   SELECT combined.*,
                          {CHAIN_STAGE_SQL_CASE} AS stage
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
                      total_deliveries, delivery_queued, delivery_sending, delivery_delivered,
                      delivery_unresolved, created_at, last_activity_at, next_action_at, retry_count,
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
pub(crate) static BOARD_QUERY: LazyLock<String> =
    LazyLock::new(|| board_query_sql(BOARD_ELIGIBLE_RECENT));

/// The most status events one page of the chain timeline may carry. Public and documented on
/// [`crate::use_cases::thread::TaskPersistence::list_task_status_events`]; the detail loader keeps
/// its own limit rather than widening this one.
pub(crate) const MAX_STATUS_EVENT_PAGE: usize = 200;

/// A chain detail pane is an operational view, not an export: past these the pane truncates and
/// says so rather than projecting an unbounded ledger into one HTML response.
pub(crate) const CHAIN_DETAIL_MAX_TASKS: i64 = 200;
pub(crate) const CHAIN_DETAIL_MAX_ATTEMPTS: i64 = 1_000;
pub(crate) const CHAIN_DETAIL_MAX_DELIVERIES: i64 = 1_000;
pub(crate) const CHAIN_DETAIL_MAX_EVENTS: i64 = 200;
pub(crate) const CHAIN_DETAIL_MAX_APPROVALS: i64 = 200;
pub(crate) const CHAIN_DETAIL_MAX_OUTREACHES: i64 = 200;

/// What a bounded read actually asks the database for: one row past the limit.
///
/// That sentinel is the whole difference between "there are exactly `limit` of these" and "there
/// are more than `limit` of these" — a result that merely happens to be `limit` long is not
/// truncation, and must not be reported as such.
pub(crate) fn probe_limit(limit: i64) -> i64 {
    limit + 1
}

/// Drop the sentinel row [`probe_limit`] fetched, answering whether it was there.
pub(crate) fn trim_to_limit<T>(rows: &mut Vec<T>, limit: i64) -> bool {
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
pub(crate) fn group_by_task<T>(rows: impl IntoIterator<Item = (Uuid, T)>) -> HashMap<Uuid, Vec<T>> {
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
pub(crate) fn chain_status_events_query<'a>(
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
pub(crate) const BOARD_QUERY_WARN_THRESHOLD: Duration = Duration::from_millis(500);

/// Log the board projection when it runs long, with the number that actually decides whether the
/// query needs rethinking.
///
/// `returned_cards` is capped by the per-column display limit, so it plateaus and says nothing
/// about cost. Every row of a non-empty stage carries that stage's shared total, so summing one
/// total per stage recovers the working set the query really aggregated over — the value to watch
/// grow.
pub(crate) fn warn_if_board_projection_is_slow(
    company_id: Uuid,
    elapsed: Duration,
    rows: &[TaskChainCardDb],
) {
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

pub(crate) async fn chain_board_on(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    filter: TaskBoardFilter,
) -> AppResult<TaskChainBoard> {
    let started = Instant::now();
    let rows = sqlx::query_as::<_, TaskChainCardDb>(&BOARD_QUERY)
        .bind(company_id)
        .bind(filter.channel_id)
        .bind(filter.terminal_since)
        .bind(filter.per_column_limit as i64)
        .fetch_all(pool)
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

pub(crate) async fn chain_status_events_on(
    pool: &sqlx::PgPool,
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
    .fetch_all(pool)
    .await
    .map_err(AppError::from)?;
    rows.into_iter().map(TryInto::try_into).collect()
}

/// One chain, whole, in a fixed number of queries.
///
/// Every collection is read in one batched, bounded statement rather than per task: the
/// per-task loop this replaced cost roughly `1 + 1 + 2n + 3` round trips, so a 200-task chain
/// spent ~405 sequential trips to the database on a pane a viewer opens live.
pub(crate) async fn chain_detail_on(
    pool: &sqlx::PgPool,
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
    .fetch_optional(pool)
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
    .fetch_all(pool)
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
    .fetch_all(pool)
    .await
    .map_err(AppError::from)?;
    truncated |= trim_to_limit(&mut attempt_rows, CHAIN_DETAIL_MAX_ATTEMPTS);
    let mut attempts = group_by_task(
        attempt_rows
            .into_iter()
            .map(|row| Ok((row.task_id, TaskAttemptRecord::try_from(row.record)?)))
            .collect::<AppResult<Vec<_>>>()?,
    );

    let mut delivery_rows = deliveries_for_tasks(
        pool,
        company_id,
        &task_ids,
        probe_limit(CHAIN_DETAIL_MAX_DELIVERIES),
    )
    .await?;
    truncated |= trim_to_limit(&mut delivery_rows, CHAIN_DETAIL_MAX_DELIVERIES);
    let mut deliveries = group_by_task(
        delivery_rows
            .into_iter()
            // `task_id = ANY(..)` is what selected these rows, so the column cannot be null here;
            // the entry carries it as an `Option` because a delivery in general need not belong to
            // a task -- an approval notice does not.
            .filter_map(|entry| entry.task_id.map(|task_id| (task_id, entry))),
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
    .fetch_all(pool)
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
        .fetch_all(pool)
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
    .fetch_all(pool)
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
