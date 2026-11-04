//! A workflow database guard that refuses the worker's own write twice in a row is a
//! disagreement between Rust and SQL, not an outage: the run fails, naming the guard, instead of
//! being retried until its deadline (fix-app 03 batch 1). Guards tripped inside a handler's own
//! transaction stay the handler's typed, retryable step failure (`*.guard_violation`).
use super::{activation::ActivationRequest, lease::WorkflowFence};
use crate::application::app_error::{AppResult, WorkflowGuardViolation};
use async_trait::async_trait;

/// Which worker operation the violation came from. Decides which time races the failure
/// transaction leaves to their existing owner, so it never defers to the operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardSite {
    /// A job step, a wait resume or a receipt consumption: run expiry and expired-lease
    /// retirement still own their races.
    Step,
    /// `expire_run`: the deadline has passed, so nothing else will settle the run.
    RunExpiry,
    /// `retire_expired_io` / `retire_exhausted_work`: the expired-lease owner itself failed.
    LeaseExpiry,
}

impl GuardSite {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::RunExpiry => "run_expiry",
            Self::LeaseExpiry => "lease_expiry",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GuardFailure {
    pub scope: ActivationRequest,
    pub violation: WorkflowGuardViolation,
    pub site: GuardSite,
    /// The IO claim whose completion was refused; `None` for operations that hold no claim.
    pub fence: Option<WorkflowFence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardFailureOutcome {
    /// The run failed with the guard's name; its jobs, waits, deliveries and live children ended.
    Failed,
    /// Another owner settles this state (run expiry, lease retirement, a competing claim, or a
    /// completed execution); nothing was written.
    Deferred,
    /// The run had already ended; nothing was written.
    Inactive,
}

impl GuardFailureOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Deferred => "deferred",
            Self::Inactive => "inactive",
        }
    }
}

#[async_trait]
pub trait WorkflowGuardFailure: Send + Sync {
    /// One transaction under the run lock. Writes only when the run is live, its scope execution
    /// is incomplete and no other owner holds or settled the scope job.
    async fn fail_run_on_guard(&self, failure: GuardFailure) -> AppResult<GuardFailureOutcome>;
}
