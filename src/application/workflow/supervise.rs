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
    Stopped(StopReason),
}

/// `handler` must be the operation itself, never a detached task's JoinHandle.
/// Call only with a committed claim. No transaction is held while polling it.
pub async fn supervise_io<P, H, C>(
    port: &P,
    claim: ClaimedWorkflow,
    policy: LeasePolicy,
    operation_deadline: Instant,
    handler: H,
    cancel: C,
) -> SupervisedResult
where
    P: WorkflowLeases,
    H: Future<Output = WorkflowHandlerResult>,
    C: Future<Output = ()>,
{
    let fence = claim.fence;
    // Own the allocation, so dropping it destroys the actual future BEFORE cleanup.
    let mut handler = Box::pin(handler);
    let mut cancel = Box::pin(cancel);
    let deadline = operation_deadline.min(claim.window.run_deadline);
    let mut expires = claim.window.expires;
    let mut heartbeat = Instant::now() + policy.heartbeat();
    let mut failure = None;
    let outcome = loop {
        tokio::select! { biased;
            _ = &mut cancel => break SupervisedResult::Stopped(StopReason::Cancelled),
            _ = sleep_until(deadline) => break SupervisedResult::Stopped(StopReason::Deadline),
            _ = sleep_until(expires) => break SupervisedResult::Stopped(StopReason::LeaseLost),
            _ = sleep_until(heartbeat) => {
                let renewed = tokio::select! { biased;
                    _ = &mut cancel => break SupervisedResult::Stopped(StopReason::Cancelled),
                    _ = sleep_until(deadline) => break SupervisedResult::Stopped(StopReason::Deadline),
                    _ = sleep_until(expires) => break SupervisedResult::Stopped(StopReason::LeaseLost),
                    result = timeout(policy.persistence_timeout(),port.renew_io(fence,policy)) => result,
                };
                match renewed {
                    Ok(Ok(Some(window))) => expires=window.expires,
                    Ok(Ok(None)) => break SupervisedResult::Stopped(StopReason::LeaseLost),
                    _ => break SupervisedResult::Stopped(StopReason::Persistence),
                }
                heartbeat = Instant::now()+policy.heartbeat();
            }
            output = &mut handler => {
                let output = match output { Ok(output) => output, Err(error) => { failure = Some(error); break SupervisedResult::Stopped(StopReason::HandlerFailed); } };
                if serde_json::to_vec(&output).map_or(true, |bytes| bytes.len()>claim.max_result_bytes) {
                    break SupervisedResult::Stopped(StopReason::OversizedResult);
                }
                let valid = tokio::select! { biased;
                    _ = &mut cancel => break SupervisedResult::Stopped(StopReason::Cancelled),
                    _ = sleep_until(deadline) => break SupervisedResult::Stopped(StopReason::Deadline),
                    _ = sleep_until(expires) => break SupervisedResult::Stopped(StopReason::LeaseLost),
                    result = timeout(policy.persistence_timeout(),port.validate_io(fence,policy)) => result,
                };
                break match valid {
                    Ok(Ok(true)) if Instant::now()<expires && Instant::now()<deadline => SupervisedResult::Ready(FencedWorkflowResult { fence, output }),
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
            policy.persistence_timeout(),
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
