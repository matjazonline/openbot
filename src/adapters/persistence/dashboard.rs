//! The read side of `/ui/dashboard`: aggregates over the tables the queues already write.
//!
//! Kept out of [`super::task`] deliberately — that module is the queue's own read/write path and is
//! already long; this one is reporting, never writes, and its queries are shaped by what a panel
//! needs rather than by what a worker does.
//!
//! How often these run is not decided here. `services::dashboard_snapshot` shares one reading per
//! view across every tab for a tick, and this adapter answers each question it is asked.
//!
//! # Scope
//!
//! Every aggregate exists twice. The company form filters on `company_id = $n`. The global form has
//! no company predicate at all, for the operator's cross-company rollup. [`ScopedSql`] builds both
//! from one template, and Rust chooses between them on `company: Option<Uuid>`.
//!
//! This used to be one statement per aggregate, filtered with `($1::uuid IS NULL OR company_id =
//! $1)`. That is correct under a custom plan, where Postgres folds the `IS NULL` away and the
//! predicate reaches the index. Under a generic plan the parameter is unknown when the plan is
//! built, so the `OR` survives as a filter applied after the scan. A company's dashboard then reads
//! every company's rows and discards the rest. sqlx prepares these statements, and a five-second
//! tick reaches Postgres's switch to a generic plan within a minute, so this was a real risk.
//!
//! The single statement guarded against the scoping rule being written in two places, and one copy
//! drifting open. The shared template now gives that guarantee instead: both forms are the same
//! text, and each aggregate states its company predicate exactly once.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, postgres::PgArguments, query::Query};
use std::{str::FromStr, sync::LazyLock};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        dashboard::{
            AttemptStats, DashboardSnapshot, DashboardWindow, DeliveryHealth, DeliveryStatusCount,
            LatencyBucket, OUTSTANDING_LIMIT, OutstandingTask, QueueDepthBucket, RetryRateBucket,
            TaskQueueHealth, TaskStatusCount, ThroughputBucket,
        },
        task::TaskStatus,
        transport::DeliveryStatus,
    },
    services::dashboard_snapshot::DashboardPersistence,
};

type PgQuery<'q> = Query<'q, Postgres, PgArguments>;

/// Where a template takes its company predicate.
const SCOPE: &str = "{scope}";

/// One aggregate as the pair of statements it really is: one company's rows, or every company's.
///
/// Built from a template that carries [`SCOPE`] exactly once. The company form puts its predicate
/// there and the global form puts nothing, so the two differ by that one clause and nothing else.
///
/// The predicate always binds the statement's *last* parameter. The global form can then leave
/// that parameter out, instead of carrying one it never reads.
struct ScopedSql {
    company: String,
    global: String,
}

impl ScopedSql {
    /// `predicate` is the whole clause the company form adds, with its own leading `WHERE` or `AND`.
    fn new(template: &str, predicate: &str) -> Self {
        assert_eq!(
            template.matches(SCOPE).count(),
            1,
            "a scoped template marks exactly one place for its company predicate"
        );
        Self {
            company: template.replacen(SCOPE, predicate, 1),
            global: template.replacen(SCOPE, "", 1),
        }
    }

    /// The statement for `company`, for a template with no parameters of its own.
    fn query(&self, company: Option<Uuid>) -> PgQuery<'_> {
        self.query_with(company, |query| query)
    }

    /// The statement for `company`, with `bind` applying every parameter except the company. The
    /// company is bound last in the company form and not at all in the global form.
    fn query_with<'q>(
        &'q self,
        company: Option<Uuid>,
        bind: impl FnOnce(PgQuery<'q>) -> PgQuery<'q>,
    ) -> PgQuery<'q> {
        match company {
            Some(company) => bind(sqlx::query(&self.company)).bind(company),
            None => bind(sqlx::query(&self.global)),
        }
    }
}

/// Counts of `background_tasks` grouped by status, plus the two states no grouping shows.
const TASK_QUEUE_SQL: &str = r#"
    SELECT status,
           COUNT(*)::bigint AS count
      FROM background_tasks
     {scope}
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
     {scope}"#;

const DELIVERY_QUEUE_SQL: &str = r#"
    SELECT status,
           COUNT(*)::bigint AS count
      FROM message_deliveries
     {scope}
     GROUP BY status
     ORDER BY status"#;

/// Deliveries claimed under a lease that has already run out, and the ready-to-send backlog.
///
/// `expired_leases` counts rows the next maintenance sweep will charge an attempt. `due_now` is
/// the backlog actually waiting on a worker: both claimable statuses, and only the rows whose
/// backoff has elapsed.
const DELIVERY_PRESSURE_SQL: &str = r#"
    SELECT COUNT(*) FILTER (
               WHERE status = 'sending'
                 AND (execution_id IS NULL OR owner_worker_id IS NULL OR locked_at IS NULL
                      OR lock_expires_at IS NULL
                      OR lock_expires_at <= locked_at
                      OR lock_expires_at <= CURRENT_TIMESTAMP)
           )::bigint AS expired_leases,
           COUNT(*) FILTER (
               WHERE status IN ('pending', 'retryable') AND available_at <= CURRENT_TIMESTAMP
           )::bigint AS due_now
      FROM message_deliveries
     {scope}"#;

/// Every bucket boundary in the window, whether or not anything happened in it.
///
/// Prepended to each bucketed query so they all return exactly `window.bucket_count()` rows in
/// ascending order. Without it the aggregates are *sparse* — a quiet bucket produces no row at all —
/// which the old bar strip got away with because it drew whatever it was given, but which a chart
/// with a time axis cannot: the gap closes up and the axis then claims two adjacent columns were
/// five minutes apart when they were an hour apart.
///
/// The slots are floored on the same epoch grid as the data (`floor(epoch / $1) * $1`), because the
/// join is on equality: a boundary derived any other way would miss every bucket by a fraction of a
/// second and every row would come back zero.
///
/// The span reads `$2 * 60 - $1` rather than a bucket count because it is the same statement: from
/// the newest boundary, step back the whole window and forward one bucket, so the newest boundary is
/// included and the count comes out at `minutes / bucket_minutes` exactly.
///
/// `$1` and `$2` rather than anything later because the company, when there is one, has to be the
/// last parameter — see [`ScopedSql`]. Every bucketed statement binds them through
/// [`bucketed`], so the numbering is stated once.
///
/// Shared as one string rather than copied into each query — three copies of this arithmetic is
/// three chances for one chart's x-axis to silently disagree with the others'.
const SLOTS_CTE: &str = r#"
    WITH slots AS (
        SELECT generate_series(
                   to_timestamp(floor(extract(epoch FROM CURRENT_TIMESTAMP) / $1) * $1)
                       - make_interval(secs => $2 * 60 - $1),
                   to_timestamp(floor(extract(epoch FROM CURRENT_TIMESTAMP) / $1) * $1),
                   make_interval(secs => $1)
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
        SELECT to_timestamp(floor(extract(epoch FROM updated_at) / $1) * $1) AS bucket,
               COUNT(*) FILTER (WHERE status = 'completed')::bigint AS completed,
               COUNT(*) FILTER (WHERE status IN ('failed', 'dead_letter'))::bigint AS failed
          FROM background_tasks
         WHERE status IN ('completed', 'failed', 'dead_letter')
           AND updated_at >= CURRENT_TIMESTAMP - make_interval(mins => $2)
           {scope}
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
        SELECT to_timestamp(floor(extract(epoch FROM attempt.started_at) / $1) * $1) AS bucket,
               percentile_disc(0.5) WITHIN GROUP (
                   ORDER BY (extract(epoch FROM (attempt.finished_at - attempt.started_at))
                             * 1000)::double precision
               ) AS p50_ms,
               percentile_disc(0.95) WITHIN GROUP (
                   ORDER BY (extract(epoch FROM (attempt.finished_at - attempt.started_at))
                             * 1000)::double precision
               ) AS p95_ms
          FROM task_attempts AS attempt
          JOIN background_tasks AS task ON task.id = attempt.task_id
         WHERE attempt.started_at >= CURRENT_TIMESTAMP - make_interval(mins => $2)
           {scope}
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
/// `(company_id, status, created_at DESC, id DESC)` index is for in the company form.
const QUEUE_DEPTH_BODY: &str = r#",
    open_tasks AS (
        SELECT created_at, updated_at, status
          FROM background_tasks
         WHERE (status IN ('pending', 'processing', 'pending_approval',
                           'waiting_for_third_party_reply')
                OR updated_at >= CURRENT_TIMESTAMP - make_interval(mins => $2))
           {scope}
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
     WHERE attempt.started_at >= CURRENT_TIMESTAMP - make_interval(mins => $1)
       {scope}"#;

/// Retry share per bucket, including empty buckets as `attempts = 0` so the chart keeps its time
/// axis without claiming that an idle interval had a zero-percent retry rate.
const RETRY_RATE_BODY: &str = r#",
    measured AS (
        SELECT to_timestamp(floor(extract(epoch FROM attempt.started_at) / $1) * $1) AS bucket,
               COUNT(*)::bigint AS attempts,
               COUNT(*) FILTER (WHERE attempt.attempt_number > 1)::bigint AS retries
          FROM task_attempts AS attempt
          JOIN background_tasks AS task ON task.id = attempt.task_id
         WHERE attempt.started_at >= CURRENT_TIMESTAMP - make_interval(mins => $2)
           {scope}
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
     WHERE (
             task.status IN ('processing', 'pending_approval',
                             'waiting_for_third_party_reply', 'dead_letter')
             OR (task.status = 'pending' AND task.run_at <= CURRENT_TIMESTAMP)
           )
       {scope}
     ORDER BY (task.status = 'dead_letter'
                OR (task.status = 'processing'
                    AND (task.lock_expires_at IS NULL
                         OR task.lock_expires_at <= CURRENT_TIMESTAMP))) DESC,
              task.updated_at DESC,
              task.id DESC
     LIMIT $1"#;

/// Each aggregate's company and global forms, assembled once at first use.
///
/// The predicate is the only text the two forms do not share. Its placeholder is one past the
/// template's own parameters: `$1` for the four unparameterised counts, `$3` after [`SLOTS_CTE`]'s
/// two, and `$2` after the window of [`ATTEMPT_STATS_SQL`] or the limit of [`OUTSTANDING_SQL`].
///
/// Assembled at first use rather than on every read. A reading is taken at most once per tick per
/// view, and there is still no reason to rebuild twenty strings each time.
static TASK_QUEUE: LazyLock<ScopedSql> =
    LazyLock::new(|| ScopedSql::new(TASK_QUEUE_SQL, "WHERE company_id = $1"));
static TASK_PRESSURE: LazyLock<ScopedSql> =
    LazyLock::new(|| ScopedSql::new(TASK_PRESSURE_SQL, "WHERE company_id = $1"));
static DELIVERY_QUEUE: LazyLock<ScopedSql> =
    LazyLock::new(|| ScopedSql::new(DELIVERY_QUEUE_SQL, "WHERE company_id = $1"));
static DELIVERY_PRESSURE: LazyLock<ScopedSql> =
    LazyLock::new(|| ScopedSql::new(DELIVERY_PRESSURE_SQL, "WHERE company_id = $1"));
static THROUGHPUT: LazyLock<ScopedSql> = LazyLock::new(|| {
    ScopedSql::new(
        &format!("{SLOTS_CTE}{THROUGHPUT_BODY}"),
        "AND company_id = $3",
    )
});
static LATENCY: LazyLock<ScopedSql> = LazyLock::new(|| {
    ScopedSql::new(
        &format!("{SLOTS_CTE}{LATENCY_BODY}"),
        "AND task.company_id = $3",
    )
});
static QUEUE_DEPTH: LazyLock<ScopedSql> = LazyLock::new(|| {
    ScopedSql::new(
        &format!("{SLOTS_CTE}{QUEUE_DEPTH_BODY}"),
        "AND company_id = $3",
    )
});
static RETRY_RATE: LazyLock<ScopedSql> = LazyLock::new(|| {
    ScopedSql::new(
        &format!("{SLOTS_CTE}{RETRY_RATE_BODY}"),
        "AND task.company_id = $3",
    )
});
static ATTEMPT_STATS: LazyLock<ScopedSql> =
    LazyLock::new(|| ScopedSql::new(ATTEMPT_STATS_SQL, "AND task.company_id = $2"));
static OUTSTANDING: LazyLock<ScopedSql> =
    LazyLock::new(|| ScopedSql::new(OUTSTANDING_SQL, "AND task.company_id = $2"));

/// A bucketed statement with its parameters bound: the bucket width and the window span as
/// [`SLOTS_CTE`]'s `$1` and `$2`, then the company, if there is one, as `$3`.
fn bucketed(statement: &ScopedSql, company: Option<Uuid>, window: DashboardWindow) -> PgQuery<'_> {
    statement.query_with(company, |query| {
        query
            .bind(window.bucket_seconds())
            .bind(window.minutes() as i32)
    })
}

#[async_trait]
impl DashboardPersistence for PostgresPersistence {
    async fn dashboard_snapshot(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<DashboardSnapshot> {
        // Sequential rather than joined: these are eight unrelated aggregates over two tables, and
        // a single query producing all of them would be a cross join nobody could read or index.
        //
        // Sequential rather than concurrent, too. The snapshot cache runs this once per view per
        // tick, however many tabs are open. Saving seven round trips for that one caller is not
        // worth taking eight pool connections at once away from the workers that need them.
        Ok(DashboardSnapshot {
            tasks: self.task_queue_health(company).await?,
            deliveries: self.delivery_health(company).await?,
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
        let rows = TASK_QUEUE
            .query(company)
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

        let pressure = TASK_PRESSURE
            .query(company)
            .fetch_one(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(TaskQueueHealth {
            by_status,
            stalled: pressure.try_get("stalled").map_err(AppError::from)?,
            due_now: pressure.try_get("due_now").map_err(AppError::from)?,
        })
    }

    async fn delivery_health(&self, company: Option<Uuid>) -> AppResult<DeliveryHealth> {
        let rows = DELIVERY_QUEUE
            .query(company)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        let mut by_status = Vec::with_capacity(rows.len());
        for row in rows {
            let raw: String = row.try_get("status").map_err(AppError::from)?;
            by_status.push(DeliveryStatusCount {
                status: DeliveryStatus::from_str(&raw)
                    .map_err(|err| AppError::Internal(err.to_string()))?,
                count: row.try_get("count").map_err(AppError::from)?,
            });
        }

        let pressure = DELIVERY_PRESSURE
            .query(company)
            .fetch_one(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(DeliveryHealth {
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
        let rows = bucketed(&THROUGHPUT, company, window)
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
        let rows = bucketed(&LATENCY, company, window)
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
        let rows = bucketed(&QUEUE_DEPTH, company, window)
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
        let rows = bucketed(&RETRY_RATE, company, window)
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
        let rows = OUTSTANDING
            .query_with(company, |query| query.bind(OUTSTANDING_LIMIT))
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
        let row = ATTEMPT_STATS
            .query_with(company, |query| query.bind(window.minutes() as i32))
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
#[path = "dashboard_tests.rs"]
mod tests;
