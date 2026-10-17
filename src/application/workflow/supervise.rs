//! Own the actual future, including while persistence is slow or unavailable.
use super::lease::*;
use std::future::Future;
use tokio::time::{Instant, sleep_until, timeout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Cancelled,
    Deadline,
    LeaseLost,
    Persistence,
    OversizedResult,
    HandlerFailed,
}
#[derive(Debug)]
pub enum SupervisedResult {
    Ready(FencedWorkflowResult),
    /// The handler's own commit parked the step; the port confirmed the exact close.
    Suspended(crate::domain::workflow::WaitId),
    Stopped(StopReason),
}

/// `handler` must be the operation itself, never a detached task's JoinHandle.
/// Call only with a committed claim. No transaction is held while polling it.
pub async fn supervise_io<P, H, C, O>(
    port: &P,
    claim: ClaimedWorkflow,
    policy: LeasePolicy,
    operation_deadline: Instant,
    handler: H,
    cancel: C,
) -> SupervisedResult
where
    P: WorkflowLeases,
    H: Future<Output = O>,
    O: Into<WorkflowHandlerOutcome>,
    C: Future<Output = ()>,
{
    let fence = claim.fence;
    // Own the allocation, so dropping it destroys the actual future BEFORE cleanup.
    let mut handler = Box::pin(handler);
    let mut cancel = Box::pin(cancel);
    let deadline = operation_deadline.min(claim.window.run_deadline);
    let mut expires = claim.window.expires;
    let mut heartbeat = next_heartbeat(Instant::now(), claim.window, policy);
    let mut failure = None;
    let outcome = loop {
        tokio::select! { biased;
            _ = &mut cancel => break SupervisedResult::Stopped(StopReason::Cancelled),
            _ = sleep_until(deadline) => break SupervisedResult::Stopped(StopReason::Deadline),
            _ = sleep_until(expires) => break SupervisedResult::Stopped(StopReason::LeaseLost),
            _ = sleep_until(heartbeat.unwrap_or(expires)), if heartbeat.is_some() => {
                let renewed = tokio::select! { biased;
                    _ = &mut cancel => break SupervisedResult::Stopped(StopReason::Cancelled),
                    _ = sleep_until(deadline) => break SupervisedResult::Stopped(StopReason::Deadline),
                    _ = sleep_until(expires) => break SupervisedResult::Stopped(StopReason::LeaseLost),
                    result = timeout(policy.supervision_guard(),port.renew_io(fence,policy)) => result,
                };
                match renewed {
                    Ok(Ok(Some(window))) => {
                        expires = window.expires;
                        heartbeat = next_heartbeat(Instant::now(), window, policy);
                    }
                    Ok(Ok(None)) => break SupervisedResult::Stopped(StopReason::LeaseLost),
                    _ => break SupervisedResult::Stopped(StopReason::Persistence),
                }
            }
            output = &mut handler => {
                let output = match output.into() {
                    WorkflowHandlerOutcome::Completed(output) => output,
                    WorkflowHandlerOutcome::Suspended(suspended) => {
                        let confirmed = tokio::select! { biased;
                            _ = &mut cancel => break SupervisedResult::Stopped(StopReason::Cancelled),
                            result = timeout(policy.supervision_guard(),port.confirm_suspension(fence,suspended.wait)) => result,
                        };
                        // Never complete or release a parked attempt: its job is already closed.
                        break match confirmed {
                            Ok(Ok(true)) => SupervisedResult::Suspended(suspended.wait),
                            Ok(Ok(false)) => SupervisedResult::Stopped(StopReason::LeaseLost),
                            _ => SupervisedResult::Stopped(StopReason::Persistence),
                        };
                    }
                };
                let output = match output { Ok(output) => output, Err(error) => { failure = Some(error); break SupervisedResult::Stopped(StopReason::HandlerFailed); } };
                if serde_json::to_vec(&output).map_or(true, |bytes| bytes.len()>claim.max_result_bytes) {
                    break SupervisedResult::Stopped(StopReason::OversizedResult);
                }
                let valid = tokio::select! { biased;
                    _ = &mut cancel => break SupervisedResult::Stopped(StopReason::Cancelled),
                    _ = sleep_until(deadline) => break SupervisedResult::Stopped(StopReason::Deadline),
                    _ = sleep_until(expires) => break SupervisedResult::Stopped(StopReason::LeaseLost),
                    result = timeout(policy.supervision_guard(),port.validate_io(fence,policy)) => result,
                };
                break match valid {
                    Ok(Ok(true)) if Instant::now()<expires && Instant::now()<deadline => SupervisedResult::Ready(FencedWorkflowResult { fence, output, policy }),
                    Ok(Ok(_)) => SupervisedResult::Stopped(StopReason::LeaseLost),
                    _ => SupervisedResult::Stopped(StopReason::Persistence),
                };
            }
        }
    };
    drop(handler);
    if let SupervisedResult::Stopped(reason) = outcome {
        // Cleanup is best effort and bounded; expired ownership is charged by the
        // exact-job recovery transition if storage refuses or cannot be reached.
        let _ = timeout(
            policy.supervision_guard(),
            port.release_io(
                fence,
                policy,
                failure
                    .map(LeaseReleaseCause::Classified)
                    .unwrap_or_else(|| reason.release_cause()),
            ),
        )
        .await;
    }
    outcome
}

/// Windows are anchored at the operation's start, but this runs after it returned. So
/// renew no later than one full persistence deadline before expiry: a renewal may then
/// take its whole bound and still land. A window capped at the run deadline cannot be
/// extended; renewing it again would only loop on the global capacity lock, so stop and
/// let the deadline or expiry end supervision.
fn next_heartbeat(now: Instant, window: LeaseWindow, policy: LeasePolicy) -> Option<Instant> {
    if window.expires >= window.run_deadline {
        return None;
    }
    let latest = window
        .expires
        .checked_sub(policy.persistence_deadline())
        .unwrap_or(now);
    Some((now + policy.heartbeat()).min(latest).max(now))
}

#[cfg(test)]
#[path = "supervise_tests.rs"]
mod tests;

impl StopReason {
    fn release_cause(self) -> LeaseReleaseCause {
        match self {
            Self::Cancelled => LeaseReleaseCause::LocalInterruption,
            Self::Deadline => LeaseReleaseCause::Deadline,
            Self::LeaseLost => LeaseReleaseCause::LeaseLost,
            Self::Persistence => LeaseReleaseCause::StorageFailure,
            Self::OversizedResult => LeaseReleaseCause::InvalidResult,
            Self::HandlerFailed => LeaseReleaseCause::HandlerFailure,
        }
    }
}
