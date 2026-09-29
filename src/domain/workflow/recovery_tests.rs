use super::*;
use crate::domain::workflow::FailureCode;

fn failure(class: FailureClass) -> StepFailure {
    StepFailure::new(class, FailureCode::parse("provider.failure").unwrap(), None).unwrap()
}

fn snapshot(consumed: u32, limit: u32) -> RecoverySnapshot {
    let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    RecoverySnapshot {
        attempts: AttemptBudget::new(consumed, limit).unwrap(),
        work_budget: RetryEligibility::Available,
        now,
        deadline: now + TimeDelta::hours(1),
    }
}

#[test]
fn workflow_recovery_backoff_is_positive_capped_and_deterministic() {
    let policy = RecoveryPolicy::default();
    let failed = failure(FailureClass::Retryable);
    for (consumed, seconds) in [(1, 2), (2, 4), (3, 8), (8, 256), (9, 300), (999, 300)] {
        let state = snapshot(consumed, 1000);
        let expected = RecoveryDecision::RetryAt(state.now + TimeDelta::seconds(seconds));
        assert_eq!(
            policy.decide(&failed, RetrySafety::SafeToRetry, state),
            expected
        );
        assert_eq!(
            policy.decide(&failed, RetrySafety::SafeToRetry, state),
            expected
        );
        let RecoveryDecision::RetryAt(at) = expected else {
            unreachable!()
        };
        assert!(
            at > state.now,
            "a second poll at the same time cannot reclaim"
        );
    }
    // Extreme counters must not overflow or turn into unbounded work.
    let state = snapshot(u32::MAX - 1, u32::MAX);
    assert_eq!(
        policy.decide(&failed, RetrySafety::SafeToRetry, state),
        RecoveryDecision::RetryAt(state.now + TimeDelta::seconds(300))
    );
}

#[test]
fn workflow_recovery_terminal_and_spent_attempts_use_final_error() {
    let policy = RecoveryPolicy::default();
    for (class, consumed) in [(FailureClass::Terminal, 1), (FailureClass::Retryable, 3)] {
        let state = snapshot(consumed, 3);
        assert_eq!(
            policy.decide(&failure(class), RetrySafety::SafeToRetry, state),
            RecoveryDecision::FinalError
        );
        assert_eq!(state.attempts.consumed(), consumed);
        assert_eq!(state.attempts.limit(), 3);
    }
    assert_eq!(
        policy.decide(
            &failure(FailureClass::Retryable),
            RetrySafety::SafeToRetry,
            snapshot(2, 3)
        ),
        RecoveryDecision::RetryAt(snapshot(2, 3).now + TimeDelta::seconds(4))
    );
}

#[test]
fn workflow_recovery_limits_cannot_be_bypassed_by_retry_or_error_route() {
    let policy = RecoveryPolicy::default();
    for class in [FailureClass::Retryable, FailureClass::Terminal] {
        let mut state = snapshot(1, 3);
        state.work_budget = RetryEligibility::Exhausted;
        assert_eq!(
            policy.decide(&failure(class), RetrySafety::SafeToRetry, state),
            RecoveryDecision::FailRun(RecoveryLimit::WorkBudget)
        );
        state.deadline = state.now;
        assert_eq!(
            policy.decide(&failure(class), RetrySafety::SafeToRetry, state),
            RecoveryDecision::FailRun(RecoveryLimit::Deadline)
        );
    }
    let mut state = snapshot(1, 3);
    state.deadline = state.now + TimeDelta::seconds(2);
    assert_eq!(
        policy.decide(
            &failure(FailureClass::Retryable),
            RetrySafety::SafeToRetry,
            state
        ),
        RecoveryDecision::FailRun(RecoveryLimit::Deadline)
    );
    state.deadline += TimeDelta::microseconds(1);
    assert!(matches!(
        policy.decide(
            &failure(FailureClass::Retryable),
            RetrySafety::SafeToRetry,
            state
        ),
        RecoveryDecision::RetryAt(_)
    ));
    state.now = DateTime::<Utc>::MAX_UTC - TimeDelta::seconds(1);
    state.deadline = DateTime::<Utc>::MAX_UTC;
    assert_eq!(
        policy.decide(
            &failure(FailureClass::Retryable),
            RetrySafety::SafeToRetry,
            state
        ),
        RecoveryDecision::FailRun(RecoveryLimit::Deadline)
    );
}

#[test]
fn workflow_recovery_unknown_effect_never_becomes_retry_or_final_error() {
    for class in [FailureClass::Retryable, FailureClass::Terminal] {
        for consumed in [1, 3] {
            for expired in [false, true] {
                let mut state = snapshot(consumed, 3);
                if expired {
                    state.deadline = state.now;
                    state.work_budget = RetryEligibility::Exhausted;
                }
                assert_eq!(
                    RecoveryPolicy::default().decide(
                        &failure(class),
                        RetrySafety::EffectOutcomeUnknown,
                        state
                    ),
                    RecoveryDecision::Reconcile
                );
            }
        }
    }
}

#[test]
fn workflow_recovery_configuration_and_attempt_boundaries() {
    for (consumed, limit) in [(0, 3), (1, 0), (4, 3)] {
        assert_eq!(
            AttemptBudget::new(consumed, limit),
            Err(RecoveryPolicyError::InvalidAttempts)
        );
    }
    assert!(AttemptBudget::new(1, 1).is_ok());
    for (initial, maximum) in [
        (Duration::ZERO, Duration::from_secs(1)),
        (Duration::from_nanos(1), Duration::from_secs(1)),
        (Duration::from_secs(2), Duration::from_secs(1)),
        (
            Duration::from_secs(1),
            MAX_BACKOFF + Duration::from_nanos(1),
        ),
    ] {
        assert!(matches!(
            RecoveryPolicy::new(initial, maximum),
            Err(RecoveryPolicyError::InvalidBackoff)
        ));
    }
    for delay in [Duration::from_millis(1), MAX_BACKOFF] {
        let policy = RecoveryPolicy::new(delay, delay).unwrap();
        let mut state = snapshot(1, 2);
        state.deadline += TimeDelta::hours(1);
        assert_eq!(
            policy.decide(
                &failure(FailureClass::Retryable),
                RetrySafety::SafeToRetry,
                state
            ),
            RecoveryDecision::RetryAt(state.now + TimeDelta::from_std(delay).unwrap())
        );
    }
}
