use std::{fmt, sync::OnceLock};

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkflowSlotsRecommendationPolicy {
    pub window_hours: i32,
    pub minimum_coverage_minutes: i32,
    pub minimum_samples_per_level: i64,
    pub cpu_p95_percent: f64,
    pub cpu_pressure_p95_percent: f64,
    pub rss_p95_percent: f64,
    pub database_acquire_p95_ms: f64,
    pub reserved_pool_connections: i32,
}

pub const WORKFLOW_SLOTS_RECOMMENDATION_POLICY: WorkflowSlotsRecommendationPolicy =
    WorkflowSlotsRecommendationPolicy {
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

    /// The identity of *this* process, resolved once.
    ///
    /// Off Fly [`Self::from_runtime_environment`] mints a fresh `local-<uuid>` on every call, so
    /// two callers asking separately would invent two machines and report the same process under
    /// both. Everything that names the running process -- the metrics sampler and the task
    /// attempt ledger -- goes through here so they cannot disagree.
    pub fn process() -> &'static Self {
        static PROCESS: OnceLock<MachineIdentity> = OnceLock::new();
        PROCESS.get_or_init(Self::from_runtime_environment)
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
    pub active_workflow_jobs: i32,
    pub workflow_worker_slots: i32,
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
    pub active_workflow_jobs: i32,
    pub workflow_worker_slots: i32,
    pub database_acquire_duration_ms: f64,
    pub database_acquire_succeeded: bool,
    pub pool_size: i32,
    pub pool_idle: i32,
    pub pool_active: i32,
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
}

/// The most concurrent workflow jobs the serving machine has actually run within the resource
/// criteria: advice for `WORKFLOW_WORKER_SLOTS`, never an automatic runtime adjustment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowWorkerSlotsRecommendation {
    pub maximum: i32,
    pub supporting_samples: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuntimeMetricSnapshot {
    pub current: Option<RuntimeMetricSample>,
    pub peak_rss_bytes: Option<i64>,
    pub buckets: Vec<RuntimeMetricBucket>,
    pub suggested_workflow_worker_slots: Option<WorkflowWorkerSlotsRecommendation>,
}
