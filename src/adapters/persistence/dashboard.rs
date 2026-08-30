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
use std::{str::FromStr, sync::LazyLock};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        dashboard::{
            AttemptStats, DashboardSnapshot, DashboardWindow, LatencyBucket, OUTSTANDING_LIMIT,
            OutboxHealth, OutboxStatusCount, OutstandingTask, QueueDepthBucket, RetryRateBucket,
            TaskQueueHealth, TaskStatusCount, ThroughputBucket,
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
               WHERE status = 'sending'
                 AND (worker_id IS NULL OR locked_at IS NULL OR lock_expires_at IS NULL
                      OR lock_expires_at <= locked_at
                      OR lock_expires_at <= CURRENT_TIMESTAMP)
           )::bigint AS expired_leases,
           COUNT(*) FILTER (
               WHERE status = 'pending' AND available_at <= CURRENT_TIMESTAMP
           )::bigint AS due_now
      FROM email_outbox
     WHERE ($1::uuid IS NULL OR company_id = $1)"#;

/// Every bucket boundary in the window, whether or not anything happened in it.
///
/// Prepended to each bucketed query so they all return exactly `window.bucket_count()` rows in
/// ascending order. Without it the aggregates are *sparse* — a quiet bucket produces no row at all —
/// which the old bar strip got away with because it drew whatever it was given, but which a chart
/// with a time axis cannot: the gap closes up and the axis then claims two adjacent columns were
/// five minutes apart when they were an hour apart.
///
/// The slots are floored on the same epoch grid as the data (`floor(epoch / $2) * $2`), because the
/// join is on equality: a boundary derived any other way would miss every bucket by a fraction of a
/// second and every row would come back zero.
///
/// The span reads `$3 * 60 - $2` rather than a bucket count because it is the same statement: from
/// the newest boundary, step back the whole window and forward one bucket, so the newest boundary is
/// included and the count comes out at `minutes / bucket_minutes` exactly.
///
/// Shared as one string rather than copied into each query — three copies of this arithmetic is
/// three chances for one chart's x-axis to silently disagree with the others'.
const SLOTS_CTE: &str = r#"
    WITH slots AS (
        SELECT generate_series(
                   to_timestamp(floor(extract(epoch FROM CURRENT_TIMESTAMP) / $2) * $2)
                       - make_interval(secs => $3 * 60 - $2),
                   to_timestamp(floor(extract(epoch FROM CURRENT_TIMESTAMP) / $2) * $2),
                   make_interval(secs => $2)
               ) AS bucket
    )"#;

/// Terminal outcomes per time bucket across the window, gap-filled by [`SLOTS_CTE`].
///
/// Bucketing is epoch-flooring rather than `date_trunc`, because `date_trunc` only offers whole
/// units and the window is sliced in fives. `updated_at` is when the row reached its current
/// status, which for a `completed` or `dead_letter` row is when it finished.
///
/// One row here is one *task*, not one attempt: a task that failed twice and then completed
/// contributes a single `completed`. Attempt-level counts come from [`ATTEMPT_STATS_SQL`].
const THROUGHPUT_BODY: &str = r#",
    finished AS (
        SELECT to_timestamp(floor(extract(epoch FROM updated_at) / $2) * $2) AS bucket,
               COUNT(*) FILTER (WHERE status = 'completed')::bigint AS completed,
               COUNT(*) FILTER (WHERE status IN ('failed', 'dead_letter'))::bigint AS failed
          FROM background_tasks
         WHERE ($1::uuid IS NULL OR company_id = $1)
           AND status IN ('completed', 'failed', 'dead_letter')
           AND updated_at >= CURRENT_TIMESTAMP - make_interval(mins => $3)
         GROUP BY bucket
    )
    SELECT slots.bucket,
           COALESCE(finished.completed, 0)::bigint AS completed,
           COALESCE(finished.failed, 0)::bigint AS failed
      FROM slots
      LEFT JOIN finished ON finished.bucket = slots.bucket
     ORDER BY slots.bucket"#;

/// Attempt duration percentiles per time bucket, gap-filled by [`SLOTS_CTE`].
///
/// The per-bucket twin of [`ATTEMPT_STATS_SQL`], and it inherits that query's `::double precision`
/// cast for the same load-bearing reason — see its comment before touching this one.
///
/// A bucket in which nothing finished keeps its `NULL` percentiles rather than being coalesced to
/// zero. The chart draws that as a break in the line: nobody measured a zero-millisecond attempt,
/// and a floor-scraping line would read as "suddenly very fast" when it means "nothing ran".
///
/// Bucketed on `started_at`, so an attempt lands in the slice it began in even if it ran past the
/// boundary — which keeps this consistent with `ATTEMPT_STATS_SQL`'s window filter.
const LATENCY_BODY: &str = r#",
    measured AS (
        SELECT to_timestamp(floor(extract(epoch FROM attempt.started_at) / $2) * $2) AS bucket,
               percentile_disc(0.5) WITHIN GROUP (
                   ORDER BY (extract(epoch FROM (attempt.finished_at - attempt.started_at))
                             * 1000)::double precision
               ) AS p50_ms,
               percentile_disc(0.95) WITHIN GROUP (
                   ORDER BY (extract(epoch FROM (attempt.finished_at - attempt.started_at))
                             * 1000)::double precision
               ) AS p95_ms
          FROM task_attempts attempt
          JOIN background_tasks task ON task.id = attempt.task_id
         WHERE ($1::uuid IS NULL OR task.company_id = $1)
           AND attempt.started_at >= CURRENT_TIMESTAMP - make_interval(mins => $3)
         GROUP BY bucket
    )
    SELECT slots.bucket,
           measured.p50_ms,
           measured.p95_ms
      FROM slots
      LEFT JOIN measured ON measured.bucket = slots.bucket
     ORDER BY slots.bucket"#;

/// How many tasks were still open at each bucket boundary, gap-filled by [`SLOTS_CTE`].
///
/// Nothing samples queue depth as it happens and there is no history table, so this reconstructs it
/// from the task rows themselves — which is only possible because `background_tasks` is never
/// pruned. A task counts as open at boundary `T` when it existed by then (`created_at <= T`) and had
/// not finished by then: either it is still not finished now, or its last write landed after `T`.
///
/// What that does *not* preserve: a row carries only its *last* update, so a task that failed,
/// backed off and was retried inside the window contributes one open-to-closed transition rather
/// than its real sawtooth. The boundary between open and closed is right; the path between them is
/// smoothed. A sampler writing real gauges is the only way to do better, and it would cost a table.
///
/// The `open_tasks` CTE narrows before the join on purpose. The join is buckets x tasks over a table
/// with no retention, so without it the 24-hour window scans every task the system has ever run;
/// with it the work is bounded by "unfinished, or finished recently", which is what the
/// `(company_id, status, created_at DESC, id DESC)` index is for.
const QUEUE_DEPTH_BODY: &str = r#",
    open_tasks AS (
        SELECT created_at, updated_at, status
          FROM background_tasks
         WHERE ($1::uuid IS NULL OR company_id = $1)
           AND (status IN ('pending', 'processing', 'pending_approval',
                           'waiting_for_third_party_reply')
                OR updated_at >= CURRENT_TIMESTAMP - make_interval(mins => $3))
    )
    SELECT slots.bucket,
           COUNT(open_tasks.created_at)::bigint AS open_count
      FROM slots
      LEFT JOIN open_tasks
             ON open_tasks.created_at <= slots.bucket
            AND (open_tasks.status IN ('pending', 'processing', 'pending_approval',
                                       'waiting_for_third_party_reply')
                 OR open_tasks.updated_at > slots.bucket)
     GROUP BY slots.bucket
     ORDER BY slots.bucket"#;

/// The four bucketed statements, each [`SLOTS_CTE`] followed by its own body.
///
/// Assembled once at first use rather than on every read: the dashboard re-reads on a five-second
/// tick for every connected tab, and there is no reason for that to rebuild four strings each time.
static THROUGHPUT_SQL: LazyLock<String> = LazyLock::new(|| format!("{SLOTS_CTE}{THROUGHPUT_BODY}"));
static LATENCY_SQL: LazyLock<String> = LazyLock::new(|| format!("{SLOTS_CTE}{LATENCY_BODY}"));
static RETRY_RATE_SQL: LazyLock<String> = LazyLock::new(|| format!("{SLOTS_CTE}{RETRY_RATE_BODY}"));
static QUEUE_DEPTH_SQL: LazyLock<String> =
    LazyLock::new(|| format!("{SLOTS_CTE}{QUEUE_DEPTH_BODY}"));

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
           COUNT(*) FILTER (WHERE attempt.attempt_number > 1)::bigint AS retries,
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
      FROM task_attempts AS attempt
      JOIN background_tasks AS task ON task.id = attempt.task_id
     WHERE ($1::uuid IS NULL OR task.company_id = $1)
       AND attempt.started_at >= CURRENT_TIMESTAMP - make_interval(mins => $2)"#;

/// Retry share per bucket, including empty buckets as `attempts = 0` so the chart keeps its time
/// axis without claiming that an idle interval had a zero-percent retry rate.
const RETRY_RATE_BODY: &str = r#",
    measured AS (
        SELECT to_timestamp(floor(extract(epoch FROM attempt.started_at) / $2) * $2) AS bucket,
               COUNT(*)::bigint AS attempts,
               COUNT(*) FILTER (WHERE attempt.attempt_number > 1)::bigint AS retries
          FROM task_attempts AS attempt
          JOIN background_tasks AS task ON task.id = attempt.task_id
         WHERE ($1::uuid IS NULL OR task.company_id = $1)
           AND attempt.started_at >= CURRENT_TIMESTAMP - make_interval(mins => $3)
         GROUP BY bucket
    )
    SELECT slots.bucket,
           COALESCE(measured.attempts, 0)::bigint AS attempts,
           COALESCE(measured.retries, 0)::bigint AS retries
      FROM slots
      LEFT JOIN measured ON measured.bucket = slots.bucket
     ORDER BY slots.bucket"#;

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
        // Sequential rather than joined: these are seven unrelated aggregates over two tables, and a
        // single query producing all of them would be a cross join nobody could read or index.
        Ok(DashboardSnapshot {
            tasks: self.task_queue_health(company).await?,
            outbox: self.outbox_health(company).await?,
            throughput: self.throughput(company, window).await?,
            latency: self.latency(company, window).await?,
            retry_rate: self.retry_rate(company, window).await?,
            queue_depth: self.queue_depth(company, window).await?,
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
        let rows = sqlx::query(&*THROUGHPUT_SQL)
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

    async fn latency(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<Vec<LatencyBucket>> {
        let rows = sqlx::query(&*LATENCY_SQL)
            .bind(company)
            .bind(window.bucket_seconds())
            .bind(window.minutes() as i32)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        rows.into_iter()
            .map(|row| {
                // `percentile_disc` hands back the type it ordered by, so these decode as `f64` and
                // are rounded here rather than cast in SQL -- a millisecond is the smallest unit the
                // page ever shows, and rounding once at the boundary keeps it that way.
                let millis = |column| -> AppResult<Option<i64>> {
                    Ok(row
                        .try_get::<Option<f64>, _>(column)
                        .map_err(AppError::from)?
                        .map(|value| value.round() as i64))
                };

                Ok(LatencyBucket {
                    bucket: row
                        .try_get::<DateTime<Utc>, _>("bucket")
                        .map_err(AppError::from)?,
                    p50_ms: millis("p50_ms")?,
                    p95_ms: millis("p95_ms")?,
                })
            })
            .collect()
    }

    async fn queue_depth(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<Vec<QueueDepthBucket>> {
        let rows = sqlx::query(&*QUEUE_DEPTH_SQL)
            .bind(company)
            .bind(window.bucket_seconds())
            .bind(window.minutes() as i32)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        rows.into_iter()
            .map(|row| {
                Ok(QueueDepthBucket {
                    bucket: row
                        .try_get::<DateTime<Utc>, _>("bucket")
                        .map_err(AppError::from)?,
                    open: row.try_get("open_count").map_err(AppError::from)?,
                })
            })
            .collect()
    }

    async fn retry_rate(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<Vec<RetryRateBucket>> {
        let rows = sqlx::query(&*RETRY_RATE_SQL)
            .bind(company)
            .bind(window.bucket_seconds())
            .bind(window.minutes() as i32)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        rows.into_iter()
            .map(|row| {
                Ok(RetryRateBucket {
                    bucket: row
                        .try_get::<DateTime<Utc>, _>("bucket")
                        .map_err(AppError::from)?,
                    attempts: row.try_get("attempts").map_err(AppError::from)?,
                    retries: row.try_get("retries").map_err(AppError::from)?,
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
            retries: row.try_get("retries").map_err(AppError::from)?,
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
    use crate::entities::task::NewTask;
    use crate::entities::task::{
        TaskAttemptOutcome, TaskAttemptRef, TaskAttemptStatus, TaskStopReason, TokenUsage,
    };
    use crate::use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    };

    async fn test_persistence() -> Option<PostgresPersistence> {
        Some(PostgresPersistence::new(test_pool().await?))
    }

    /// A company and a task of this test's own, for the attempt-ledger tests below.
    ///
    /// `task_attempts.task_id` is a foreign key, so those tests need a task that exists. They used
    /// to take whichever task happened to already be in the database and return early when there
    /// was none — which meant they reported success while asserting nothing the moment the table
    /// was empty, and the table is empty at the start of every run now. Owning the fixture keeps
    /// them honest and scopes them to a company nothing else touches.
    async fn task_fixture(persistence: &PostgresPersistence, label: &str) -> (Uuid, Uuid) {
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("{label}_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .expect("the fixture user is created");
        let owner = UserPersistence::get_by_email(persistence, &email)
            .await
            .expect("the fixture user is readable")
            .expect("the fixture user was just created");
        let company = CompanyPersistence::create(
            persistence,
            owner.id,
            CompanyWrite {
                name: "Dashboard Test".to_string(),
                slug: format!("{label}-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .expect("the fixture company is created");
        let channel = ChannelPersistence::create(
            persistence,
            company.id,
            ChannelWrite {
                name: "Dashboard".into(),
                slug: "dashboard".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .expect("the fixture channel is created");
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "dashboard-probe",
                serde_json::json!({}),
            ))
            .await
            .expect("the fixture task is queued");

        (task.id, company.id)
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
        // Gap-filled, so "owns nothing" reads as a full series of zeroes rather than no series at
        // all: `SLOTS_CTE` generates one bucket per slice of the window from `CURRENT_TIMESTAMP`
        // alone, and never sees `$1`. An emptiness check here would assert the chart has no x-axis.
        assert_eq!(snapshot.throughput_total(), 0, "{:?}", snapshot.throughput);
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

        let (task_id, company) = task_fixture(&persistence, "latency").await;

        let attempt = TaskAttemptRef {
            task_id,
            attempt_number: 9_998,
            execution_generation: Uuid::new_v4(),
        };
        persistence
            .begin_task_attempt(attempt)
            .await
            .expect("the ledger row opens");
        persistence
            .finish_task_attempt(&TaskAttemptOutcome {
                attempt,
                status: TaskAttemptStatus::Completed,
                stop_reason: TaskStopReason::Completed,
                error: None,
                tokens: Some(TokenUsage::new(3, 5)),
            })
            .await
            .expect("the ledger row closes");

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

        CompanyPersistence::delete(&persistence, company)
            .await
            .expect("the fixture company is removed");
    }

    #[tokio::test]
    async fn retry_rate_counts_only_attempt_numbers_above_one_and_stays_company_scoped() {
        let Some(persistence) = test_persistence().await else {
            return;
        };
        let (task_id, company) = task_fixture(&persistence, "retry-rate").await;

        for attempt_number in [1, 2] {
            persistence
                .begin_task_attempt(TaskAttemptRef {
                    task_id,
                    attempt_number,
                    execution_generation: Uuid::new_v4(),
                })
                .await
                .expect("the attempt starts");
        }

        for window in DashboardWindow::PRESETS {
            let snapshot = persistence
                .dashboard_snapshot(Some(company), window)
                .await
                .expect("the scoped attempt aggregates run");
            assert_eq!(snapshot.attempts.attempts, 2);
            assert_eq!(snapshot.attempts.retries, 1);
            assert_eq!(snapshot.attempts.retry_rate_percent(), Some(50.0));
            assert_eq!(snapshot.retry_rate.len() as i64, window.bucket_count());
            assert!(snapshot.retry_rate.iter().any(|bucket| {
                bucket.attempts == 2 && bucket.retries == 1 && bucket.rate_percent() == Some(50.0)
            }));
        }

        let unrelated = persistence
            .dashboard_snapshot(Some(Uuid::new_v4()), DashboardWindow::last_hour())
            .await
            .expect("an unrelated company scope runs");
        assert_eq!(unrelated.attempts, AttemptStats::default());
        assert!(
            unrelated
                .retry_rate
                .iter()
                .all(|bucket| bucket.attempts == 0)
        );

        CompanyPersistence::delete(&persistence, company)
            .await
            .expect("the fixture company is removed");
    }

    #[tokio::test]
    async fn an_attempt_is_ledgered_and_can_be_reopened_by_a_re_run() {
        let Some(persistence) = test_persistence().await else {
            return;
        };

        let (task_id, company) = task_fixture(&persistence, "ledger").await;

        // A number far above any real retry count, so this test cannot collide with live rows.
        let attempt = TaskAttemptRef {
            task_id,
            attempt_number: 9_999,
            execution_generation: Uuid::new_v4(),
        };

        persistence
            .begin_task_attempt(attempt)
            .await
            .expect("the ledger row opens");

        let outcome = TaskAttemptOutcome {
            attempt,
            status: TaskAttemptStatus::Completed,
            stop_reason: TaskStopReason::Completed,
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
        let replacement = TaskAttemptRef {
            execution_generation: Uuid::new_v4(),
            ..attempt
        };
        persistence
            .begin_task_attempt(replacement)
            .await
            .expect("a re-run reopens the same attempt rather than colliding with it");

        assert!(
            !persistence
                .finish_task_attempt(&outcome)
                .await
                .expect("the stale execution can report without writing"),
            "the execution replaced during reclaim must not finish the new ledger row"
        );
        assert!(
            persistence
                .finish_task_attempt(&TaskAttemptOutcome {
                    attempt: replacement,
                    ..outcome.clone()
                })
                .await
                .expect("the replacement execution closes"),
            "the current execution generation must still be able to finish"
        );

        let reopened: (String, Option<i32>, Option<String>) = sqlx::query_as(
            "SELECT status, prompt_tokens, stop_reason FROM task_attempts WHERE task_id = $1 AND attempt_number = $2",
        )
        .bind(task_id)
        .bind(9_999_i32)
        .fetch_one(persistence.pool())
        .await
        .expect("the reopened row is readable");

        assert_eq!(
            reopened.0,
            TaskAttemptStatus::Completed.as_str(),
            "the replacement run finished"
        );
        assert_eq!(reopened.1, Some(11));
        assert_eq!(
            reopened.2.as_deref(),
            Some(TaskStopReason::Completed.as_str())
        );

        CompanyPersistence::delete(&persistence, company)
            .await
            .expect("the fixture company is removed");
    }
}
