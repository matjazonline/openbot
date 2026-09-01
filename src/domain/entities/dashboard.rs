//! What `/ui/dashboard` reports: the state of both queues right now, and how they have been
//! performing over a trailing window.
//!
//! Every figure here is derived from the tables the queues already write, so the numbers survive a
//! restart and mean the same thing on every machine — unlike the process-local counters behind
//! [`crate::domain::monitoring::MonitoringService`], which reset on deploy and which the page shows
//! alongside these as clearly-labelled since-boot gauges.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::entities::{outbox::OutboxStatus, task::TaskStatus};

/// The trailing period the rate panels cover, and how finely it is sliced.
///
/// Held as a value rather than two loose `i64`s so a caller cannot pass the bucket where the window
/// belongs — they are both minute counts, which is exactly the swap the compiler could not catch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardWindow {
    minutes: i64,
    bucket_minutes: i64,
}

impl DashboardWindow {
    /// Every window a reader can pick, in the order the selector offers them.
    pub const PRESETS: [Self; 3] = [Self::last_hour(), Self::last_six_hours(), Self::last_day()];

    /// The default view: the last hour, in five-minute buckets.
    pub const fn last_hour() -> Self {
        Self {
            minutes: 60,
            bucket_minutes: 5,
        }
    }

    /// A morning's worth, in quarter-hours.
    pub const fn last_six_hours() -> Self {
        Self {
            minutes: 360,
            bucket_minutes: 15,
        }
    }

    /// A full day, hour by hour.
    pub const fn last_day() -> Self {
        Self {
            minutes: 1440,
            bucket_minutes: 60,
        }
    }

    pub const fn minutes(&self) -> i64 {
        self.minutes
    }

    /// Bucket width in *seconds*, which is the unit the epoch-flooring in the SQL works in.
    pub const fn bucket_seconds(&self) -> f64 {
        (self.bucket_minutes * 60) as f64
    }

    /// How many slices the window is cut into -- the row count every bucketed query must return
    /// once it is gap-filled, and the number of points every chart draws.
    pub const fn bucket_count(&self) -> i64 {
        self.minutes / self.bucket_minutes
    }

    /// The window's name in a URL. Short enough to type, stable enough to bookmark.
    pub const fn slug(&self) -> &'static str {
        match self.minutes {
            360 => "6h",
            1440 => "24h",
            _ => "1h",
        }
    }

    /// The inverse of [`Self::slug`]. `None` for anything else, which callers read as "use the
    /// default" rather than as an error -- a stale bookmark should show a dashboard, not a 400.
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::PRESETS
            .into_iter()
            .find(|window| window.slug() == slug)
    }

    /// How the heading says it. `minutes` alone would render the day as "Last 1440 minutes".
    pub const fn label(&self) -> &'static str {
        match self.minutes {
            360 => "Last 6 hours",
            1440 => "Last 24 hours",
            _ => "Last hour",
        }
    }

    /// How a bucket's timestamp is labelled on the x-axis: a day-long window crosses midnight, so
    /// bare `%H:%M` would repeat itself and the reader could not tell which day a spike sat in.
    pub const fn tick_format(&self) -> &'static str {
        if self.minutes >= 1440 {
            "%b %-d %H:%M"
        } else {
            "%H:%M"
        }
    }
}

impl Default for DashboardWindow {
    fn default() -> Self {
        Self::last_hour()
    }
}

/// How many tasks sit in one status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatusCount {
    pub status: TaskStatus,
    pub count: i64,
}

/// How many queued emails sit in one status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxStatusCount {
    pub status: OutboxStatus,
    pub count: i64,
}

/// The background task queue as it stands.
#[derive(Debug, Clone, Default)]
pub struct TaskQueueHealth {
    pub by_status: Vec<TaskStatusCount>,
    /// Claimed by a worker whose lease has lapsed — nothing is renewing it, so the row will be
    /// re-claimed and the agent will run a second time. The number that should be zero.
    pub stalled: i64,
    /// Pending *and* already due: the backlog actually waiting on a worker, as opposed to tasks
    /// deliberately scheduled for later by retry backoff.
    pub due_now: i64,
}

impl TaskQueueHealth {
    pub fn total(&self) -> i64 {
        self.by_status.iter().map(|entry| entry.count).sum()
    }

    pub fn count_of(&self, status: TaskStatus) -> i64 {
        self.by_status
            .iter()
            .find(|entry| entry.status == status)
            .map_or(0, |entry| entry.count)
    }
}

/// The delivery queue as it stands.
#[derive(Debug, Clone, Default)]
pub struct OutboxHealth {
    pub by_status: Vec<OutboxStatusCount>,
    /// Being sent under a lease that has already expired. The maintenance pass fails these with
    /// `'Delivery lease expired without a result'`, so a non-zero count means deliveries are being
    /// reaped mid-flight.
    pub expired_leases: i64,
    pub due_now: i64,
}

impl OutboxHealth {
    pub fn total(&self) -> i64 {
        self.by_status.iter().map(|entry| entry.count).sum()
    }

    pub fn count_of(&self, status: OutboxStatus) -> i64 {
        self.by_status
            .iter()
            .find(|entry| entry.status == status)
            .map_or(0, |entry| entry.count)
    }
}

/// Terminal task outcomes in one slice of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThroughputBucket {
    pub bucket: DateTime<Utc>,
    pub completed: i64,
    pub failed: i64,
}

impl ThroughputBucket {
    pub fn total(&self) -> i64 {
        self.completed + self.failed
    }
}

/// Attempt duration percentiles in one slice of the window.
///
/// Both percentiles are `None` for a bucket in which no attempt *finished*. That is deliberate and
/// the charts rely on it: a quiet minute is not a zero-millisecond minute, so the line breaks there
/// rather than diving to the floor and inventing a latency nobody measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyBucket {
    pub bucket: DateTime<Utc>,
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
}

/// Retry attempts among all attempts started in one slice of the selected window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryRateBucket {
    pub bucket: DateTime<Utc>,
    pub attempts: i64,
    pub retries: i64,
}

impl RetryRateBucket {
    pub fn rate_percent(&self) -> Option<f64> {
        (self.attempts > 0).then(|| self.retries as f64 * 100.0 / self.attempts as f64)
    }
}

/// How many tasks were still open at the end of one slice of the window.
///
/// Reconstructed from the task rows themselves rather than sampled: nothing records queue depth as
/// it happens, but `background_tasks` is never pruned, so "created by then and not yet finished by
/// then" is answerable after the fact. See the query for what that reconstruction does and does not
/// preserve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueDepthBucket {
    pub bucket: DateTime<Utc>,
    pub open: i64,
}

/// Per-attempt cost and duration over the window, from `task_attempts`.
///
/// Latencies are `None` until at least one attempt has *finished* inside the window — an empty
/// window and a window of still-running work are not the same thing, and a `0` would claim they
/// were.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttemptStats {
    pub attempts: i64,
    pub retries: i64,
    pub failed: i64,
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

impl AttemptStats {
    pub fn total_tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }

    /// Share of attempts that failed, as a percentage, or `None` when nothing ran.
    pub fn failure_rate_percent(&self) -> Option<f64> {
        (self.attempts > 0).then(|| self.failed as f64 * 100.0 / self.attempts as f64)
    }

    /// Share of starts that are retries. The attempt ledger's first run is numbered one; every
    /// larger attempt number is a retry regardless of its final status.
    pub fn retry_rate_percent(&self) -> Option<f64> {
        (self.attempts > 0).then(|| self.retries as f64 * 100.0 / self.attempts as f64)
    }
}

/// How many outstanding tasks the dashboard lists before deferring to `/ui/tasks`.
///
/// Enough to cover a normal bad morning, short enough that the panel stays a summary rather than
/// becoming a second task monitor with none of its filtering.
pub const OUTSTANDING_LIMIT: i64 = 20;

/// One task the dashboard can hand you a link to.
///
/// The counts say three things are stalled; this says *which* three, and carries enough to reach
/// each one. `company_id` travels with the row rather than being taken from the page's scope
/// because the operator rollup spans companies — the link would otherwise point into whichever
/// company happened to be on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutstandingTask {
    pub id: Uuid,
    pub company_id: Uuid,
    pub company_name: String,
    pub channel_id: Uuid,
    pub channel_name: String,
    /// `None` for a task queued outside a conversation; those link to the task itself instead.
    pub thread_id: Option<Uuid>,
    pub task_type: String,
    pub status: TaskStatus,
    /// Claimed, but its lease has lapsed. Derived rather than stored — there is no `stalled`
    /// status, because from the queue's point of view the row is still `processing`.
    pub stalled: bool,
    pub retry_count: i32,
    pub last_error: Option<String>,
    /// When the task last changed state, which is what "stuck for 20 minutes" is measured from.
    pub since: DateTime<Utc>,
}

impl OutstandingTask {
    /// Is somebody expected to do something about this?
    ///
    /// A stalled or dead-lettered task is a problem. A parked one is waiting on a human or a third
    /// party by design, and a running one is simply running. One rule, so the ordering, the icon
    /// and the tint cannot disagree about what counts as trouble.
    pub fn needs_attention(&self) -> bool {
        self.stalled || self.status == TaskStatus::DeadLetter
    }
}

/// The since-boot counters of *this* process, read out of
/// [`MonitoringService::get_stats_json`](crate::domain::monitoring::MonitoringService::get_stats_json).
///
/// Shown apart from everything else on the page and labelled as such, because they mean something
/// different: they are per-process and reset on deploy. Unlike the queue figures, SMTP intake and
/// the offset classes PostgreSQL normalizes away are recorded nowhere else, so this is the only
/// place they can be seen at all. Active SSE connections are a current process-local gauge.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessGauges {
    pub smtp_total: u64,
    pub smtp_accepted: u64,
    pub ai_total: u64,
    pub ai_failed: u64,
    pub ai_total_tokens: u64,
    pub ai_avg_latency_ms: u64,
    pub deep_task_pagination: u64,
    pub deep_outbox_pagination: u64,
    pub active_dashboard_sse_connections: u64,
}

impl ProcessGauges {
    /// Read the gauges out of the monitoring service's JSON.
    ///
    /// Missing or malformed fields read as zero rather than failing: this is display-only
    /// telemetry, and a monitor that reports a shape we don't recognise should cost a panel, not
    /// the whole page.
    pub fn from_stats_json(stats: &serde_json::Value) -> Self {
        let count = |group: &str, key: &str| {
            stats
                .get(group)
                .and_then(|group| group.get(key))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        };
        let rounded = |group: &str, key: &str| {
            stats
                .get(group)
                .and_then(|group| group.get(key))
                .and_then(serde_json::Value::as_f64)
                .map_or(0, |value| value.round() as u64)
        };
        let custom = |key: &str| {
            stats
                .get("custom_counters")
                .and_then(|counters| counters.get(key))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        };
        let gauge = |key: &str| {
            stats
                .get("gauges")
                .and_then(|gauges| gauges.get(key))
                .and_then(serde_json::Value::as_f64)
                .map_or(0, |value| value.max(0.0).round() as u64)
        };

        Self {
            smtp_total: count("smtp_connections", "total"),
            smtp_accepted: count("smtp_connections", "accepted"),
            ai_total: count("ai_executions", "total"),
            ai_failed: count("ai_executions", "failed"),
            ai_total_tokens: count("ai_executions", "total_tokens"),
            ai_avg_latency_ms: rounded("ai_executions", "avg_latency_ms"),
            deep_task_pagination: custom("deep_pagination_observed{endpoint=tasks}"),
            deep_outbox_pagination: custom("deep_pagination_observed{endpoint=outbox}"),
            active_dashboard_sse_connections: gauge("active_dashboard_sse_connections"),
        }
    }

    /// SMTP connections turned away for any reason — the complement of the accepted count.
    pub fn smtp_rejected(&self) -> u64 {
        self.smtp_total.saturating_sub(self.smtp_accepted)
    }
}

/// One complete reading of the system, which is what a page render and an SSE tick each emit.
#[derive(Debug, Clone, Default)]
pub struct DashboardSnapshot {
    pub tasks: TaskQueueHealth,
    pub outbox: OutboxHealth,
    pub throughput: Vec<ThroughputBucket>,
    pub latency: Vec<LatencyBucket>,
    pub retry_rate: Vec<RetryRateBucket>,
    pub queue_depth: Vec<QueueDepthBucket>,
    pub attempts: AttemptStats,
    /// Capped at [`OUTSTANDING_LIMIT`]; the page links to the task monitor for the rest.
    pub outstanding: Vec<OutstandingTask>,
}

impl DashboardSnapshot {
    /// Completed and failed across the whole window, for the headline rate.
    pub fn throughput_total(&self) -> i64 {
        self.throughput.iter().map(ThroughputBucket::total).sum()
    }

    /// The busiest bucket, which sets the scale every bar in the sparkline is drawn against.
    pub fn throughput_peak(&self) -> i64 {
        self.throughput
            .iter()
            .map(ThroughputBucket::total)
            .max()
            .unwrap_or(0)
    }
}
