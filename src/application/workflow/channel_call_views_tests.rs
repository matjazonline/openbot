use super::*;
use serde_json::json;

fn facts() -> CallPhaseFacts {
    CallPhaseFacts {
        stored: None,
        wait: CallWait::Waiting,
        child: CallRunState::Running,
        child_waiting_reason: None,
        final_review: None,
        child_stalled: false,
        caller: CallRunState::Waiting,
        caller_execution_completed: false,
        caller_consumed: false,
        caller_stalled: false,
    }
}

const WAITS: [CallWait; 5] = [
    CallWait::Waiting,
    CallWait::Completed,
    CallWait::Expired,
    CallWait::Failed,
    CallWait::Cancelled,
];
const STATES: [CallRunState; 6] = [
    CallRunState::Queued,
    CallRunState::Running,
    CallRunState::Waiting,
    CallRunState::Succeeded,
    CallRunState::Failed,
    CallRunState::Cancelled,
];

/// The phase the gate's rules 1–4 assign, written independently of `call_phase`.
fn expected(f: &CallPhaseFacts) -> CallPhase {
    if let Some(stored) = &f.stored {
        return CallPhase::Settled {
            closure: stored.closure.clone(),
            code: stored.code.clone(),
        };
    }
    if f.wait == CallWait::Waiting {
        return match (f.child, f.final_review) {
            (CallRunState::Queued | CallRunState::Running, _) => CallPhase::ChildRunning {
                stalled: f.child_stalled,
            },
            (CallRunState::Waiting, Some(decision)) => CallPhase::AwaitingFinalReview { decision },
            (CallRunState::Waiting, None) => CallPhase::ChildWaiting {
                reason: f.child_waiting_reason.clone(),
            },
            (CallRunState::Succeeded, _) => CallPhase::ResponseReady,
            (state, _) => CallPhase::ChildEnded { state },
        };
    }
    let caller_open = !matches!(
        f.caller,
        CallRunState::Succeeded | CallRunState::Failed | CallRunState::Cancelled
    );
    if f.wait == CallWait::Completed
        && !f.caller_consumed
        && !f.caller_execution_completed
        && caller_open
    {
        return CallPhase::AwaitingCaller {
            stalled: f.caller_stalled,
        };
    }
    CallPhase::ClosedWithoutResult {
        wait: f.wait,
        caller: f.caller,
    }
}

/// V1: every combination of result row × wait state × child state and decision kind × caller
/// execution, consumption and run state × stalled maps by rules 1–4.
#[test]
fn channel_call_views_phase() {
    let review = Uuid::new_v4();
    let mut seen = std::collections::HashSet::new();
    for stored in [
        None,
        Some(StoredCallResult {
            closure: "denied".into(),
            code: Some("channel.denied".into()),
        }),
    ] {
        for wait in WAITS {
            for child in STATES {
                for final_review in [None, Some(review)] {
                    for caller in STATES {
                        for bits in 0..16_u8 {
                            let f = CallPhaseFacts {
                                stored: stored.clone(),
                                wait,
                                child,
                                child_waiting_reason: Some("decision".into()),
                                final_review,
                                child_stalled: bits & 1 != 0,
                                caller,
                                caller_execution_completed: bits & 2 != 0,
                                caller_consumed: bits & 4 != 0,
                                caller_stalled: bits & 8 != 0,
                            };
                            let phase = call_phase(&f);
                            assert_eq!(phase, expected(&f), "{f:?}");
                            seen.insert(std::mem::discriminant(&phase));
                            assert!(!phase.label().is_empty());
                        }
                    }
                }
            }
        }
    }
    assert_eq!(seen.len(), 8, "every phase is reachable");
}

/// The named cases the gate calls out.
#[test]
fn channel_call_views_phase_named_cases() {
    // A child parked on a business `decision.human` step is not a final review (F2).
    let business = CallPhaseFacts {
        child: CallRunState::Waiting,
        child_waiting_reason: Some("decision".into()),
        ..facts()
    };
    assert_eq!(
        call_phase(&business),
        CallPhase::ChildWaiting {
            reason: Some("decision".into())
        }
    );
    // Revoked: the wait completed, the caller consumed the answer, no row was stored (F8).
    let revoked = CallPhaseFacts {
        wait: CallWait::Completed,
        child: CallRunState::Succeeded,
        caller: CallRunState::Running,
        caller_consumed: true,
        ..facts()
    };
    assert_eq!(
        call_phase(&revoked),
        CallPhase::ClosedWithoutResult {
            wait: CallWait::Completed,
            caller: CallRunState::Running
        }
    );
    // Resumed, not yet consumed, the caller's job lease expired (F3).
    let resumed = CallPhaseFacts {
        caller_consumed: false,
        caller_stalled: true,
        ..revoked.clone()
    };
    assert_eq!(
        call_phase(&resumed),
        CallPhase::AwaitingCaller { stalled: true }
    );
    // The caller was cancelled after its child succeeded.
    let cancelled = CallPhaseFacts {
        wait: CallWait::Cancelled,
        child: CallRunState::Succeeded,
        caller: CallRunState::Cancelled,
        ..facts()
    };
    assert_eq!(
        call_phase(&cancelled).label(),
        "Closed without a stored result (wait cancelled, caller cancelled)"
    );
    // A child whose job lease expired differs from one awaiting review.
    let stalled = CallPhaseFacts {
        child_stalled: true,
        ..facts()
    };
    assert_eq!(
        call_phase(&stalled).label(),
        "Child stalled: its job has not progressed"
    );
}

/// The counter's labels are bounded and classify refusals apart from later errors.
#[test]
fn channel_call_views_outcome_labels() {
    use crate::application::workflow::channel_call::{REVOKED, TARGET_UNAVAILABLE};
    use crate::application::workflow::tools::tool_error;
    let label = |value| outcome_label(&Ok(AgentToolObservation::Completed(value)));
    assert_eq!(label(json!({"closure":"done"})), "done");
    assert_eq!(label(json!({"closure":"no_reply"})), "no_reply");
    assert_eq!(label(tool_error(TARGET_UNAVAILABLE, "no")), "refused");
    assert_eq!(label(tool_error(REVOKED, "no")), "error");
    assert_eq!(outcome_label(&Ok(AgentToolObservation::Uncertain)), "error");
}
