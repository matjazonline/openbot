use std::fmt;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaskWorkerRecommendationPolicy {
    pub window_hours: i32,
    pub minimum_coverage_minutes: i32,
    pub minimum_samples_per_level: i64,
    pub cpu_p95_percent: f64,
    pub cpu_pressure_p95_percent: f64,
    pub rss_p95_percent: f64,
    pub database_acquire_p95_ms: f64,
    pub reserved_pool_connections: i32,
}

pub const TASK_WORKER_RECOMMENDATION_POLICY: TaskWorkerRecommendationPolicy =
    TaskWorkerRecommendationPolicy {
        window_hours: 24,
        // One ten-minute deploy or connectivity gap should not restart a full day of evidence.
        minimum_coverage_minutes: 23 * 60 + 50,
        minimum_samples_per_level: 100,
        cpu_p95_percent: 80.0,
        cpu_pressure_p95_percent: 5.0,
        rss_p95_percent: 80.0,
        database_acquire_p95_ms: 100.0,
        reserved_pool_connections: 2,
    };

/// Stable for one Fly Machine, and boot-local elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MachineId(String);

impl MachineId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MachineId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Fly region when available. Local development has no region to invent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MachineRegion(String);

impl MachineRegion {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MachineRegion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineIdentity {
    pub id: MachineId,
    pub region: Option<MachineRegion>,
}

impl MachineIdentity {
    /// Fly exposes a unique id per Machine. Outside Fly a UUID deliberately lasts only for this
    /// boot, so samples from separate local runs are never presented as one continuous process.
    pub fn from_runtime_environment() -> Self {
        let id = std::env::var("FLY_MACHINE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(MachineId::new)
            .unwrap_or_else(|| MachineId::new(format!("local-{}", uuid::Uuid::new_v4())));
        let region = std::env::var("FLY_REGION")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(MachineRegion::new);

        Self { id, region }
    }
}

/// Memory provider calls that completed on this machine during one sampling interval.
///
/// The aggregate spans every configured provider. The `hydradb_` column names it persists into
/// predate the second provider and are kept deliberately — see the schema comment.
///
/// Counted rather than probed: the interval reports the memory calls the application actually
/// made, so an idle machine costs the provider nothing and the latency shown is the latency
/// recall and ingestion really paid.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MemoryProviderInterval {
    pub calls: i32,
    pub failures: i32,
    pub total_duration_ms: f64,
}

impl MemoryProviderInterval {
    /// `None` for an interval with no calls — an idle interval has no latency to average.
    pub fn mean_duration_ms(&self) -> Option<f64> {
        (self.calls > 0).then(|| self.total_duration_ms / f64::from(self.calls))
    }
}

/// Host/process values observed before the database acquisition probe begins.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeMetricObservation {
    pub identity: MachineIdentity,
    pub sampled_at: DateTime<Utc>,
    pub process_rss_bytes: Option<i64>,
    pub memory_limit_bytes: Option<i64>,
    pub cpu_utilization_percent: Option<f64>,
    pub cpu_steal_percent: Option<f64>,
    pub cpu_throttle_percent: Option<f64>,
    pub active_task_executions: i32,
    pub task_worker_concurrency_limit: i32,
    pub hydradb: MemoryProviderInterval,
}

/// One durable ten-second reading from one serving machine.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeMetricSample {
    pub identity: MachineIdentity,
    pub sampled_at: DateTime<Utc>,
    pub process_rss_bytes: Option<i64>,
    pub memory_limit_bytes: Option<i64>,
    pub cpu_utilization_percent: Option<f64>,
    pub cpu_steal_percent: Option<f64>,
    pub cpu_throttle_percent: Option<f64>,
    pub active_task_executions: i32,
    pub task_worker_concurrency_limit: i32,
    pub database_acquire_duration_ms: f64,
    pub database_acquire_succeeded: bool,
    pub pool_size: i32,
    pub pool_idle: i32,
    pub pool_active: i32,
    pub hydradb: MemoryProviderInterval,
}

/// Aggregates rendered as one point in the selected dashboard range.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeMetricBucket {
    pub bucket: DateTime<Utc>,
    pub cpu_utilization_percent: Option<f64>,
    pub cpu_steal_percent: Option<f64>,
    pub cpu_throttle_percent: Option<f64>,
    pub database_acquire_p50_ms: Option<f64>,
    pub database_acquire_p95_ms: Option<f64>,
    /// `None` where no sample landed in the bucket at all; `Some(0)` where the machine was up
    /// and made no memory provider calls. The two are a gap and a quiet period, not the same thing.
    pub hydradb_calls: Option<i64>,
    pub hydradb_failures: Option<i64>,
    pub hydradb_mean_ms: Option<f64>,
}

/// Highest concurrency level the serving machine has actually demonstrated within the resource
/// criteria. This is advice for configuration, never an automatic runtime adjustment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskWorkerConcurrencyRecommendation {
    pub maximum: i32,
    pub supporting_samples: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuntimeMetricSnapshot {
    pub current: Option<RuntimeMetricSample>,
    pub peak_rss_bytes: Option<i64>,
    pub buckets: Vec<RuntimeMetricBucket>,
    pub suggested_task_worker_concurrency: Option<TaskWorkerConcurrencyRecommendation>,
}
