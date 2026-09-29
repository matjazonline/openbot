//! Exact execution ownership. A successful handler result is not commit authorization:
//! the result transaction must recheck this fence and close the same attempt.
use super::activation::{ActivatedExecution, ActivationRequest};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{RetrySafety, StepFailure};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowWorkerId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowGeneration(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowAttempt(pub i32);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowFence {
    pub scope: ActivationRequest,
    pub worker: WorkflowWorkerId,
    pub generation: WorkflowGeneration,
    pub attempt: WorkflowAttempt,
}

#[derive(Debug, Clone, Copy)]
pub struct LeasePolicy {
    duration: Duration,
}
impl LeasePolicy {
    pub fn new(duration: Duration) -> AppResult<Self> {
        if !(Duration::from_secs(3)..=Duration::from_secs(60)).contains(&duration) {
            return Err(AppError::BadRequest(
                "Workflow lease must be 3–60 seconds".into(),
            ));
        }
        Ok(Self { duration })
    }
    pub fn duration(self) -> Duration {
        self.duration
    }
    pub fn heartbeat(self) -> Duration {
        self.duration / 3
    }
    pub fn persistence_timeout(self) -> Duration {
        Duration::from_secs(1)
    }
    pub fn retry_delay(self) -> Duration {
        Duration::from_secs(2)
    }
}

/// Conservative monotonic deadlines derived from persistence time, measured from
/// BEFORE the persistence request. Network/commit latency never extends ownership.
#[derive(Debug, Clone, Copy)]
pub struct LeaseWindow {
    pub expires: Instant,
    pub run_deadline: Instant,
}
#[derive(Debug, Clone)]
pub struct ClaimedWorkflow {
    pub fence: WorkflowFence,
    pub activation: ActivatedExecution,
    pub window: LeaseWindow,
    pub max_result_bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowFailure {
    pub failure: StepFailure,
    pub safety: RetrySafety,
}
pub type WorkflowHandlerResult = Result<Value, WorkflowFailure>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseReleaseCause {
    LocalInterruption,
    Deadline,
    LeaseLost,
    StorageFailure,
    InvalidResult,
    ActivationLimit,
    HandlerFailure,
    Classified(WorkflowFailure),
}

#[async_trait]
pub trait WorkflowLeases: Send + Sync {
    async fn claim_io(
        &self,
        scope: ActivationRequest,
        worker: WorkflowWorkerId,
        policy: LeasePolicy,
    ) -> AppResult<Option<ClaimedWorkflow>>;
    async fn renew_io(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
    ) -> AppResult<Option<LeaseWindow>>;
    async fn validate_io(&self, fence: WorkflowFence, policy: LeasePolicy) -> AppResult<bool>;
    async fn release_io(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
        cause: LeaseReleaseCause,
    ) -> AppResult<bool>;
}

#[derive(Debug)]
pub struct FencedWorkflowResult {
    pub fence: WorkflowFence,
    pub output: Value,
}

/// Retire spent ownership without granting a new attempt. Discovery can be stale,
/// and a worker may no longer provide the handler that originally claimed it.
#[async_trait]
pub trait WorkflowLeaseRecovery: Send + Sync {
    /// Retire already-debited pending work without creating another attempt.
    async fn retire_exhausted_work(
        &self,
        scope: ActivationRequest,
        policy: LeasePolicy,
    ) -> AppResult<bool>;
    async fn retire_expired_io(
        &self,
        scope: ActivationRequest,
        policy: LeasePolicy,
    ) -> AppResult<bool>;
}
