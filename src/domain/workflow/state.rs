use super::{DurableWaitRequest, EngineDisposition, TransitionTarget};
use chrono::{DateTime, Utc};
use thiserror::Error;

/// Classification of a parked run. The associated decision, event, timer,
/// child, effect, or reconciliation record is owned by later runtime work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitingReason {
    Decision,
    Event,
    Timer,
    ChildRun,
    Effect,
    Reconciliation,
}

/// Authoritative logical run state. `Running` includes queued successors and
/// retries; it does not assert that a worker currently holds a lease.
/// `Succeeded` means execution reached its declared end, including a business
/// rejection or handled error route; it does not mean business approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Queued,
    Running,
    Waiting(WaitingReason),
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RunStateError {
    #[error("cannot {action} a run in state {from:?}")]
    InvalidTransition {
        from: RunState,
        action: &'static str,
    },
    #[error("wait deadline must be in the future")]
    ExpiredDeadline,
}

impl RunState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    pub fn start(self) -> Result<Self, RunStateError> {
        match self {
            Self::Queued => Ok(Self::Running),
            _ => Err(self.invalid("start")),
        }
    }

    /// A durable resumer must atomically match company, run, wait ID, deadline
    /// policy and consumed wake condition before accepting this pure transition.
    pub fn resume(self) -> Result<Self, RunStateError> {
        match self {
            Self::Waiting(_) => Ok(Self::Running),
            _ => Err(self.invalid("resume")),
        }
    }

    /// Rechecks the deadline even if the request was valid when constructed.
    /// Sweepers, run caps and deadline policy are added with durable waits.
    pub fn park(
        self,
        request: &DurableWaitRequest,
        now: DateTime<Utc>,
    ) -> Result<Self, RunStateError> {
        if self != Self::Running {
            return Err(self.invalid("park"));
        }
        if request.deadline() <= now {
            return Err(RunStateError::ExpiredDeadline);
        }
        Ok(Self::Waiting(request.reason()))
    }

    /// Applies an already resolved engine disposition. This does not persist
    /// the execution, authorize it, prove ownership or schedule another job.
    /// The caller needs a live revision/fence and an atomic commit of the
    /// execution result, run progress, audit, and wait plus notification or
    /// successor. A failed execution with an error route may still end the run
    /// successfully after that route reaches `End`.
    pub fn apply(
        self,
        disposition: &EngineDisposition<'_>,
        now: DateTime<Utc>,
    ) -> Result<Self, RunStateError> {
        if self != Self::Running {
            return Err(self.invalid("apply outcome"));
        }
        match disposition {
            EngineDisposition::Advance { target, .. } => Ok(Self::routed(target)),
            EngineDisposition::Park { request } => self.park(request, now),
            EngineDisposition::Retry { .. } => Ok(Self::Running),
            EngineDisposition::Terminal { target, .. } => {
                Ok(target.map_or(Self::Failed, Self::routed))
            }
        }
    }

    /// Engine terminal failure, such as an exhausted run budget. Its reason
    /// and durable accounting are owned by the runtime.
    pub fn fail_terminal(self) -> Result<Self, RunStateError> {
        if self.is_terminal() {
            return Err(self.invalid("fail"));
        }
        Ok(Self::Failed)
    }

    /// Cancellation leaves terminal states unchanged. Durable cancellation
    /// must revoke pending ownership atomically and cannot erase committed
    /// outputs, receipts, or effects already made.
    pub fn cancel(self) -> Self {
        if self.is_terminal() {
            self
        } else {
            Self::Cancelled
        }
    }

    fn routed(target: &TransitionTarget) -> Self {
        match target {
            TransitionTarget::Step(_) => Self::Running,
            TransitionTarget::End => Self::Succeeded,
        }
    }

    fn invalid(self, action: &'static str) -> RunStateError {
        RunStateError::InvalidTransition { from: self, action }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::workflow::{
        CompletedStep, CompletionRoute, ContextLimits, FailureClass, FailureCode, StepFailure,
        StepId, WaitId,
    };
    use chrono::Duration;
    use serde_json::json;
    use uuid::Uuid;

    fn wait(reason: WaitingReason, now: DateTime<Utc>) -> DurableWaitRequest {
        DurableWaitRequest::new(
            WaitId::new(Uuid::new_v4()),
            reason,
            now + Duration::seconds(30),
            now,
        )
        .unwrap()
    }

    #[test]
    fn every_wait_reason_survives_request_and_park() {
        let now = Utc::now();
        for reason in [
            WaitingReason::Decision,
            WaitingReason::Event,
            WaitingReason::Timer,
            WaitingReason::ChildRun,
            WaitingReason::Effect,
            WaitingReason::Reconciliation,
        ] {
            let request = wait(reason, now);
            assert_eq!(request.reason(), reason);
            assert_eq!(
                RunState::Running.park(&request, now),
                Ok(RunState::Waiting(reason))
            );
            assert_eq!(RunState::Waiting(reason).resume(), Ok(RunState::Running));
        }
    }

    #[test]
    fn wait_rechecks_deadline_and_invalid_origins_do_not_transition() {
        let now = Utc::now();
        let request = wait(WaitingReason::Timer, now);
        for expired in [
            request.deadline(),
            request.deadline() + Duration::seconds(1),
        ] {
            assert_eq!(
                RunState::Running.park(&request, expired),
                Err(RunStateError::ExpiredDeadline)
            );
            let disposition = EngineDisposition::Park { request: &request };
            assert_eq!(
                RunState::Running.apply(&disposition, expired),
                Err(RunStateError::ExpiredDeadline)
            );
        }
        let states = [
            RunState::Queued,
            RunState::Running,
            RunState::Waiting(WaitingReason::Event),
            RunState::Succeeded,
            RunState::Failed,
            RunState::Cancelled,
        ];
        for state in states {
            assert_eq!(state.start().is_ok(), state == RunState::Queued);
            assert_eq!(
                state.resume().is_ok(),
                matches!(state, RunState::Waiting(_))
            );
            assert_eq!(
                state.park(&request, now).is_ok(),
                state == RunState::Running
            );
            let disposition = EngineDisposition::Park { request: &request };
            assert_eq!(
                state.apply(&disposition, now).is_ok(),
                state == RunState::Running
            );
        }
    }

    #[test]
    fn outcome_mapping_reuses_resolved_targets_and_preserves_business_result() {
        let now = Utc::now();
        let rejected = CompletedStep::new(
            json!({"approved": false}),
            CompletionRoute::Success,
            ContextLimits::default(),
        )
        .unwrap();
        let end = TransitionTarget::End;
        let next = TransitionTarget::Step(StepId::parse("next").unwrap());
        let advance = EngineDisposition::Advance {
            completed: &rejected,
            target: &end,
        };
        assert_eq!(
            RunState::Running.apply(&advance, now),
            Ok(RunState::Succeeded)
        );
        assert_eq!(rejected.output(), &json!({"approved": false}));
        let advance = EngineDisposition::Advance {
            completed: &rejected,
            target: &next,
        };
        assert_eq!(
            RunState::Running.apply(&advance, now),
            Ok(RunState::Running)
        );
        let failure = StepFailure::new(
            FailureClass::Retryable,
            FailureCode::parse("provider.timeout").unwrap(),
            None,
        )
        .unwrap();
        let retry = EngineDisposition::Retry { failure: &failure };
        assert_eq!(RunState::Running.apply(&retry, now), Ok(RunState::Running));
        for (target, expected) in [
            (Some(&next), RunState::Running),
            (Some(&end), RunState::Succeeded),
            (None, RunState::Failed),
        ] {
            let disposition = EngineDisposition::Terminal {
                failure: &failure,
                target,
            };
            assert_eq!(RunState::Running.apply(&disposition, now), Ok(expected));
        }
    }

    #[test]
    fn terminal_states_are_immutable_and_cancellation_covers_nonterminal() {
        for state in [
            RunState::Queued,
            RunState::Running,
            RunState::Waiting(WaitingReason::Decision),
        ] {
            assert_eq!(state.cancel(), RunState::Cancelled);
            assert_eq!(state.fail_terminal(), Ok(RunState::Failed));
        }
        for state in [RunState::Succeeded, RunState::Failed, RunState::Cancelled] {
            assert_eq!(state.cancel(), state);
            assert!(state.start().is_err());
            assert!(state.resume().is_err());
            assert!(state.fail_terminal().is_err());
        }
    }
}
