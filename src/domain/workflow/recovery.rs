//! Pure failure decisions. Persistence supplies a locked snapshot and atomically
//! applies the decision; constructing one grants neither ownership nor effect permission.
use super::{FailureClass, RetryEligibility, StepFailure};
use chrono::{DateTime, TimeDelta, Utc};
use std::time::Duration;
use thiserror::Error;

const MAX_BACKOFF: Duration = Duration::from_secs(3600);

/// Evidence from the action/handler owner, never inferred from an error class,
/// HTTP method, timeout, or lease expiry. Safe retries still recheck authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrySafety {
    SafeToRetry,
    EffectOutcomeUnknown,
}

/// Includes the failure being decided. A lost lease consumes the same allowance
/// as a returned failure; operator retry cannot replace this with a fresh budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptBudget {
    consumed: u32,
    limit: u32,
}
impl AttemptBudget {
    pub fn new(consumed: u32, limit: u32) -> Result<Self, RecoveryPolicyError> {
        if consumed == 0 || limit == 0 || consumed > limit {
            return Err(RecoveryPolicyError::InvalidAttempts);
        }
        Ok(Self { consumed, limit })
    }
    pub fn consumed(self) -> u32 {
        self.consumed
    }
    pub fn limit(self) -> u32 {
        self.limit
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RecoverySnapshot {
    pub attempts: AttemptBudget,
    /// Remaining step/model/repetition/root allowance, after charging this work.
    /// This is supplied by its durable owner, not by the failing handler.
    pub work_budget: RetryEligibility,
    pub now: DateTime<Utc>,
    pub deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryLimit {
    Deadline,
    WorkBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryDecision {
    RetryAt(DateTime<Utc>),
    /// Use the existing resolve_outcome with RetryEligibility::Exhausted to
    /// select the frozen final_error route (or fail when no route was declared).
    FinalError,
    /// No route or successor may bypass the run's deadline or work allowance.
    FailRun(RecoveryLimit),
    /// Do not retry or take a success/final_error route. Preserve the unknown
    /// effect for phase04 reconciliation, even when the run must also expire.
    /// This decision never extends the run deadline or creates an immortal wait.
    Reconcile,
}

#[derive(Debug, Clone, Copy)]
pub struct RecoveryPolicy {
    initial: Duration,
    maximum: Duration,
}
impl RecoveryPolicy {
    pub fn new(initial: Duration, maximum: Duration) -> Result<Self, RecoveryPolicyError> {
        if initial < Duration::from_millis(1) || maximum < initial || maximum > MAX_BACKOFF {
            return Err(RecoveryPolicyError::InvalidBackoff);
        }
        Ok(Self { initial, maximum })
    }

    pub fn decide(
        self,
        failure: &StepFailure,
        safety: RetrySafety,
        snapshot: RecoverySnapshot,
    ) -> RecoveryDecision {
        if safety == RetrySafety::EffectOutcomeUnknown {
            return RecoveryDecision::Reconcile;
        }
        if snapshot.deadline <= snapshot.now {
            return RecoveryDecision::FailRun(RecoveryLimit::Deadline);
        }
        if snapshot.work_budget == RetryEligibility::Exhausted {
            return RecoveryDecision::FailRun(RecoveryLimit::WorkBudget);
        }
        if failure.class() == FailureClass::Terminal
            || snapshot.attempts.consumed == snapshot.attempts.limit
        {
            return RecoveryDecision::FinalError;
        }
        // Clamp the exponent before shifting; even extreme persisted counters
        // take constant work. Saturation cannot wrap a large delay back to zero.
        let multiplier = 1_u32 << (snapshot.attempts.consumed - 1).min(31);
        let delay = self.initial.saturating_mul(multiplier).min(self.maximum);
        // Validated maximum fits chrono. checked_add also handles extreme dates.
        let Some(retry_at) = TimeDelta::from_std(delay)
            .ok()
            .and_then(|delay| snapshot.now.checked_add_signed(delay))
        else {
            return RecoveryDecision::FailRun(RecoveryLimit::Deadline);
        };
        if retry_at >= snapshot.deadline {
            return RecoveryDecision::FailRun(RecoveryLimit::Deadline);
        }
        RecoveryDecision::RetryAt(retry_at)
    }
}
impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(2),
            maximum: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RecoveryPolicyError {
    #[error("failure attempts must be positive and within the original allowance")]
    InvalidAttempts,
    #[error("retry backoff must be between one millisecond and one hour, in ascending order")]
    InvalidBackoff,
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
