//! The read side of `/ui/dashboard`: aggregates over the tables the queues already write.
//!
//! Kept out of [`super::task`] deliberately — that module is the queue's own read/write path and is
//! already long; this one is reporting, never writes, and its queries are shaped by what a panel
//! needs rather than by what a worker does.
//!
//! # Scope
//!
//! Every query takes `company: Option<Uuid>` and filters with `($1::uuid IS NULL OR company_id =
//! $1)`, so the company rollup and the operator's cross-company rollup are the *same* statement with
//! a different bind. Two statements would be two places for the scoping rule to drift, and the one
//! that drifts open leaks another company's traffic.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::Row;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        dashboard::{
            AttemptStats, DashboardSnapshot, DashboardWindow, OUTSTANDING_LIMIT, OutboxHealth,
            OutboxStatusCount, OutstandingTask, TaskQueueHealth, TaskStatusCount, ThroughputBucket,
        },
        outbox::OutboxStatus,
        task::TaskStatus,
    },
};

#[async_trait]
pub trait DashboardPersistence: Send + Sync {
    /// One complete reading, for `company` or — with `None` — for every company at once.
    async fn dashboard_snapshot(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<DashboardSnapshot>;
}

/// Counts of `background_tasks` grouped by status, plus the two states no grouping shows.
const TASK_QUEUE_SQL: &str = r#"
    SELECT status,
           COUNT(*)::bigint AS count
      FROM background_tasks
     WHERE ($1::uuid IS NULL OR company_id = $1)
     GROUP BY status
     ORDER BY status"#;

/// The states that matter operationally but are not a status of their own.
///
/// `stalled` is a claimed row whose lease has lapsed: no worker is heartbeating it, so it will be
/// re-claimed and its agent re-run. `due_now` separates a real backlog from tasks that are merely
/// scheduled for later by retry backoff — both are `pending`, and only one is a problem.
const TASK_PRESSURE_SQL: &str = r#"
    SELECT COUNT(*) FILTER (
               WHERE status = 'processing'
                 AND (lock_expires_at IS NULL OR lock_expires_at <= CURRENT_TIMESTAMP)
           )::bigint AS stalled,
           COUNT(*) FILTER (
               WHERE status = 'pending' AND run_at <= CURRENT_TIMESTAMP
           )::bigint AS due_now
      FROM background_tasks
     WHERE ($1::uuid IS NULL OR company_id = $1)"#;

const OUTBOX_QUEUE_SQL: &str = r#"
    SELECT status,
           COUNT(*)::bigint AS count
      FROM email_outbox
     WHERE ($1::uuid IS NULL OR company_id = $1)
     GROUP BY status
     ORDER BY status"#;

/// Deliveries claimed under a lease that has already run out, and the ready-to-send backlog.
///
/// Unlike the task queue, nothing renews an outbox lease, so `expired_leases` counts rows the next
/// maintenance pass will fail with `'Delivery lease expired without a result'`.
const OUTBOX_PRESSURE_SQL: &str = r#"
    SELECT COUNT(*) FILTER (
               WHERE status = 'sending' AND lock_expires_at <= CURRENT_TIMESTAMP
           )::bigint AS expired_leases,
           COUNT(*) FILTER (
               WHERE status = 'pending' AND available_at <= CURRENT_TIMESTAMP
           )::bigint AS due_now
      FROM email_outbox
     WHERE ($1::uuid IS NULL OR company_id = $1)"#;

/// Terminal outcomes per time bucket across the window.
///
/// Bucketing is epoch-flooring rather than `date_trunc`, because `date_trunc` only offers whole
/// units and the window is sliced in fives. `updated_at` is when the row reached its current
/// status, which for a `completed` or `dead_letter` row is when it finished.
///
/// One row here is one *task*, not one attempt: a task that failed twice and then completed
/// contributes a single `completed`. Attempt-level counts come from [`ATTEMPT_STATS_SQL`].
const THROUGHPUT_SQL: &str = r#"
    SELECT to_timestamp(floor(extract(epoch FROM updated_at) / $2) * $2) AS bucket,
           COUNT(*) FILTER (WHERE status = 'completed')::bigint AS completed,
           COUNT(*) FILTER (WHERE status IN ('failed', 'dead_letter'))::bigint AS failed
      FROM background_tasks
     WHERE ($1::uuid IS NULL OR company_id = $1)
       AND status IN ('completed', 'failed', 'dead_letter')
       AND updated_at >= CURRENT_TIMESTAMP - make_interval(mins => $3)
     GROUP BY bucket
     ORDER BY bucket"#;

/// Duration percentiles and token spend over the window, from `task_attempts`.
///
/// `task_attempts` carries no `company_id` of its own, so the scope comes through its task. The
/// percentiles read `NULL` while nothing has finished — an attempt still running has a `NULL`
/// `finished_at`, and `percentile_disc` skips those rather than counting them as zero.
///
/// The `::double precision` on the ordering expression is load-bearing. `extract(epoch FROM ...)`
/// returns `numeric`, and `percentile_disc` returns whatever type it ordered by — so without the
/// cast the column comes back `NUMERIC` and decoding it as `f64` fails at runtime. It fails only
/// once something has actually finished inside the window, because a `NULL` never gets decoded,
/// which is exactly the kind of bug an empty table hides.
const ATTEMPT_STATS_SQL: &str = r#"
    SELECT COUNT(*)::bigint AS attempts,
           COUNT(*) FILTER (WHERE attempt.status = 'failed')::bigint AS failed,
           percentile_disc(0.5) WITHIN GROUP (
               ORDER BY (extract(epoch FROM (attempt.finished_at - attempt.started_at))
                         * 1000)::double precision
           ) AS p50_ms,
           percentile_disc(0.95) WITHIN GROUP (
               ORDER BY (extract(epoch FROM (attempt.finished_at - attempt.started_at))
                         * 1000)::double precision
           ) AS p95_ms,
           COALESCE(SUM(attempt.prompt_tokens), 0)::bigint AS prompt_tokens,
           COALESCE(SUM(attempt.completion_tokens), 0)::bigint AS completion_tokens
      FROM task_attempts attempt
      JOIN background_tasks task ON task.id = attempt.task_id
     WHERE ($1::uuid IS NULL OR task.company_id = $1)
       AND attempt.started_at >= CURRENT_TIMESTAMP - make_interval(mins => $2)"#;

/// The tasks a reader might want to open, newest trouble first.
///
/// "Outstanding" is everything that is not finished: running, waiting on a worker, parked on a human
/// or a third party, or dead-lettered. A `pending` task whose `run_at` is still ahead of it is on a
/// retry backoff and is excluded — it is not waiting on anything, and listing it would bury the rows
/// that are.
///
/// The ordering puts trouble first, by the same rule as `OutstandingTask::needs_attention`: a lapsed
/// lease or a dead letter, then the most recently changed. `updated_at` is when the row reached its
/// current state, so it reads as "stuck since".
const OUTSTANDING_SQL: &str = r#"
    SELECT task.id,
           task.company_id,
           company.name AS company_name,
           task.channel_id,
           channel.name AS channel_name,
           task.thread_id,
           task.task_type,
           task.status,
           task.retry_count,
           task.last_error,
           task.updated_at,
           (task.status = 'processing'
             AND (task.lock_expires_at IS NULL
                  OR task.lock_expires_at <= CURRENT_TIMESTAMP)) AS stalled
      FROM background_tasks AS task
      JOIN companies AS company ON company.id = task.company_id
      JOIN channels AS channel ON channel.id = task.channel_id
     WHERE ($1::uuid IS NULL OR task.company_id = $1)
       AND (
             task.status IN ('processing', 'pending_approval',
                             'waiting_for_third_party_reply', 'dead_letter')
             OR (task.status = 'pending' AND task.run_at <= CURRENT_TIMESTAMP)
           )
     ORDER BY (task.status = 'dead_letter'
                OR (task.status = 'processing'
                    AND (task.lock_expires_at IS NULL
                         OR task.lock_expires_at <= CURRENT_TIMESTAMP))) DESC,
              task.updated_at DESC,
              task.id DESC
     LIMIT $2"#;

#[async_trait]
impl DashboardPersistence for PostgresPersistence {
    async fn dashboard_snapshot(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<DashboardSnapshot> {
        // Sequential rather than joined: these are five unrelated aggregates over two tables, and a
        // single query producing all of them would be a cross join nobody could read or index.
        Ok(DashboardSnapshot {
            tasks: self.task_queue_health(company).await?,
            outbox: self.outbox_health(company).await?,
            throughput: self.throughput(company, window).await?,
            attempts: self.attempt_stats(company, window).await?,
            outstanding: self.outstanding(company).await?,
        })
    }
}

impl PostgresPersistence {
    async fn task_queue_health(&self, company: Option<Uuid>) -> AppResult<TaskQueueHealth> {
        let rows = sqlx::query(TASK_QUEUE_SQL)
            .bind(company)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        let mut by_status = Vec::with_capacity(rows.len());
        for row in rows {
            let raw: String = row.try_get("status").map_err(AppError::from)?;
            by_status.push(TaskStatusCount {
                status: TaskStatus::from_str(&raw)
                    .map_err(|err| AppError::Internal(err.to_string()))?,
                count: row.try_get("count").map_err(AppError::from)?,
            });
        }

        let pressure = sqlx::query(TASK_PRESSURE_SQL)
            .bind(company)
            .fetch_one(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(TaskQueueHealth {
            by_status,
            stalled: pressure.try_get("stalled").map_err(AppError::from)?,
            due_now: pressure.try_get("due_now").map_err(AppError::from)?,
        })
    }

    async fn outbox_health(&self, company: Option<Uuid>) -> AppResult<OutboxHealth> {
        let rows = sqlx::query(OUTBOX_QUEUE_SQL)
            .bind(company)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        let mut by_status = Vec::with_capacity(rows.len());
        for row in rows {
            let raw: String = row.try_get("status").map_err(AppError::from)?;
            by_status.push(OutboxStatusCount {
                status: OutboxStatus::from_str(&raw)
                    .map_err(|err| AppError::Internal(err.to_string()))?,
                count: row.try_get("count").map_err(AppError::from)?,
            });
        }

        let pressure = sqlx::query(OUTBOX_PRESSURE_SQL)
            .bind(company)
            .fetch_one(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(OutboxHealth {
            by_status,
            expired_leases: pressure.try_get("expired_leases").map_err(AppError::from)?,
            due_now: pressure.try_get("due_now").map_err(AppError::from)?,
        })
    }

    async fn throughput(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<Vec<ThroughputBucket>> {
        let rows = sqlx::query(THROUGHPUT_SQL)
            .bind(company)
            .bind(window.bucket_seconds())
            .bind(window.minutes() as i32)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        rows.into_iter()
            .map(|row| {
                Ok(ThroughputBucket {
                    bucket: row
                        .try_get::<DateTime<Utc>, _>("bucket")
                        .map_err(AppError::from)?,
                    completed: row.try_get("completed").map_err(AppError::from)?,
                    failed: row.try_get("failed").map_err(AppError::from)?,
                })
            })
            .collect()
    }

    async fn outstanding(&self, company: Option<Uuid>) -> AppResult<Vec<OutstandingTask>> {
        let rows = sqlx::query(OUTSTANDING_SQL)
            .bind(company)
            .bind(OUTSTANDING_LIMIT)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        rows.into_iter()
            .map(|row| {
                let raw: String = row.try_get("status").map_err(AppError::from)?;
                Ok(OutstandingTask {
                    id: row.try_get("id").map_err(AppError::from)?,
                    company_id: row.try_get("company_id").map_err(AppError::from)?,
                    company_name: row.try_get("company_name").map_err(AppError::from)?,
                    channel_id: row.try_get("channel_id").map_err(AppError::from)?,
                    channel_name: row.try_get("channel_name").map_err(AppError::from)?,
                    thread_id: row.try_get("thread_id").map_err(AppError::from)?,
                    task_type: row.try_get("task_type").map_err(AppError::from)?,
                    status: TaskStatus::from_str(&raw)
                        .map_err(|err| AppError::Internal(err.to_string()))?,
                    stalled: row.try_get("stalled").map_err(AppError::from)?,
                    retry_count: row.try_get("retry_count").map_err(AppError::from)?,
                    last_error: row.try_get("last_error").map_err(AppError::from)?,
                    since: row.try_get("updated_at").map_err(AppError::from)?,
                })
            })
            .collect()
    }

    async fn attempt_stats(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<AttemptStats> {
        let row = sqlx::query(ATTEMPT_STATS_SQL)
            .bind(company)
            .bind(window.minutes() as i32)
            .fetch_one(&self.pool)
            .await
            .map_err(AppError::from)?;

        // The percentiles come back as `double precision` and are NULL until something finishes, so
        // they are read as `Option<f64>` and rounded here rather than cast in SQL.
        let percentile = |name: &str| -> AppResult<Option<i64>> {
            let value: Option<f64> = row.try_get(name).map_err(AppError::from)?;
            Ok(value.map(|ms| ms.round() as i64))
        };

        Ok(AttemptStats {
            attempts: row.try_get("attempts").map_err(AppError::from)?,
            failed: row.try_get("failed").map_err(AppError::from)?,
            p50_ms: percentile("p50_ms")?,
            p95_ms: percentile("p95_ms")?,
            prompt_tokens: row.try_get("prompt_tokens").map_err(AppError::from)?,
            completion_tokens: row.try_get("completion_tokens").map_err(AppError::from)?,
        })
    }
}

#[cfg(test)]
mod tests {
    //! Exercises every dashboard query against a live database.
    //!
    //! These queries use the runtime sqlx API, so nothing checks their SQL or their parameter types
    //! at build time — a `make_interval(mins => $3)` handed a `float8` fails at request time, on the
    //! page, in production. Running each one for real is the only thing that catches it.
    //!
    //! No-ops without `DATABASE_URL`, exactly like the tests in [`super::super`], so the rest of the
    //! suite still runs without a database.

    use super::*;
    use crate::adapters::persistence::task::TaskPersistence;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::entities::task::{
        TaskAttemptOutcome, TaskAttemptRef, TaskAttemptStatus, TokenUsage,
    };

    async fn test_persistence() -> Option<PostgresPersistence> {
        Some(PostgresPersistence::new(test_pool().await?))
    }

    #[tokio::test]
    async fn the_global_snapshot_runs() {
        let Some(persistence) = test_persistence().await else {
            return;
        };

        persistence
            .dashboard_snapshot(None, DashboardWindow::last_hour())
            .await
            .expect("every dashboard query is valid SQL with the parameter types it is bound with");
    }

    #[tokio::test]
    async fn a_company_scoped_snapshot_runs_and_stays_inside_its_company() {
        let Some(persistence) = test_persistence().await else {
            return;
        };

        // A company that owns nothing: every count must be zero. If the `($1 IS NULL OR ...)`
        // guard were ever written so that a bound uuid fell through to the unfiltered branch, this
        // is what would catch it — the global snapshot above cannot, because it looks the same
        // either way.
        let nobody = Uuid::new_v4();
        let snapshot = persistence
            .dashboard_snapshot(Some(nobody), DashboardWindow::last_hour())
            .await
            .expect("the scoped queries run");

        assert_eq!(snapshot.tasks.total(), 0, "{:?}", snapshot.tasks);
        assert_eq!(snapshot.tasks.stalled, 0);
        assert_eq!(snapshot.outbox.total(), 0, "{:?}", snapshot.outbox);
        assert!(snapshot.throughput.is_empty(), "{:?}", snapshot.throughput);
        assert_eq!(snapshot.attempts, AttemptStats::default());
    }

    /// Reads the percentiles with a *finished* attempt in the window.
    ///
    /// [`the_global_snapshot_runs`] cannot catch a wrong percentile column type on its own: with
    /// nothing finished the value is `NULL`, and a `NULL` is never decoded. `extract(epoch ...)`
    /// returns `numeric`, so without the cast in [`ATTEMPT_STATS_SQL`] this is where it shows.
    #[tokio::test]
    async fn finished_attempts_decode_their_latency_percentiles() {
        let Some(persistence) = test_persistence().await else {
            return;
        };

        let task_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM background_tasks LIMIT 1")
            .fetch_optional(persistence.pool())
            .await
            .expect("background_tasks is readable");
        let Some(task_id) = task_id else {
            return;
        };

        let attempt = TaskAttemptRef {
            task_id,
            attempt_number: 9_998,
        };
        persistence
            .begin_task_attempt(attempt)
            .await
            .expect("the ledger row opens");
        persistence
            .finish_task_attempt(&TaskAttemptOutcome {
                attempt,
                status: TaskAttemptStatus::Completed,
                error: None,
                tokens: Some(TokenUsage::new(3, 5)),
            })
            .await
            .expect("the ledger row closes");

        let company: Uuid =
            sqlx::query_scalar("SELECT company_id FROM background_tasks WHERE id = $1")
                .bind(task_id)
                .fetch_one(persistence.pool())
                .await
                .expect("the task's company is readable");

        // Scoped to this task's company so a neighbouring test cannot empty the window from under
        // it — the assertion needs at least one finished attempt to be there.
        let snapshot = persistence
            .dashboard_snapshot(Some(company), DashboardWindow::last_hour())
            .await
            .expect("the percentile columns decode once something has finished");

        assert!(
            snapshot.attempts.p50_ms.is_some(),
            "a finished attempt must produce a latency: {:?}",
            snapshot.attempts
        );

        sqlx::query("DELETE FROM task_attempts WHERE task_id = $1 AND attempt_number = $2")
            .bind(task_id)
            .bind(9_998_i32)
            .execute(persistence.pool())
            .await
            .expect("the probe row is removed");
    }

    #[tokio::test]
    async fn an_attempt_is_ledgered_and_can_be_reopened_by_a_re_run() {
        let Some(persistence) = test_persistence().await else {
            return;
        };

        // Borrow a real task: `task_attempts.task_id` is a foreign key, so this needs one that
        // exists. Without any task in the database there is nothing to assert against.
        let task_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM background_tasks LIMIT 1")
            .fetch_optional(persistence.pool())
            .await
            .expect("background_tasks is readable");
        let Some(task_id) = task_id else {
            return;
        };

        // A number far above any real retry count, so this test cannot collide with live rows.
        let attempt = TaskAttemptRef {
            task_id,
            attempt_number: 9_999,
        };

        persistence
            .begin_task_attempt(attempt)
            .await
            .expect("the ledger row opens");

        let outcome = TaskAttemptOutcome {
            attempt,
            status: TaskAttemptStatus::Completed,
            error: None,
            tokens: Some(TokenUsage::new(11, 7)),
        };
        assert!(
            persistence
                .finish_task_attempt(&outcome)
                .await
                .expect("the ledger row closes"),
            "closing a row that was just opened must report that it wrote"
        );

        // Closing twice must not write twice: the second call finds no open row, which is the same
        // guard that stops a superseded run from overwriting the run that took its task over.
        assert!(
            !persistence
                .finish_task_attempt(&outcome)
                .await
                .expect("the second close runs"),
            "an already-closed attempt must not be closed again"
        );

        // A task re-claimed after its lease lapsed comes back with the same attempt number. The
        // conflict must reopen the row rather than fail the insert.
        persistence
            .begin_task_attempt(attempt)
            .await
            .expect("a re-run reopens the same attempt rather than colliding with it");

        let reopened: (String, Option<i32>) = sqlx::query_as(
            "SELECT status, prompt_tokens FROM task_attempts WHERE task_id = $1 AND attempt_number = $2",
        )
        .bind(task_id)
        .bind(9_999_i32)
        .fetch_one(persistence.pool())
        .await
        .expect("the reopened row is readable");

        assert_eq!(reopened.0, "processing", "the re-run is in progress again");
        assert_eq!(
            reopened.1, None,
            "the superseded run's token count must not be left behind on the new attempt"
        );

        sqlx::query("DELETE FROM task_attempts WHERE task_id = $1 AND attempt_number = $2")
            .bind(task_id)
            .bind(9_999_i32)
            .execute(persistence.pool())
            .await
            .expect("the probe row is removed");
    }
}
