use super::*;
use crate::{
    app_error::{AppError, AppResult},
    application::workflow::{CompanyId, activation::*},
    domain::workflow::{ExecutionId, RunId, StepId},
};
use async_trait::async_trait;
use serde_json::Value;
use std::{
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use uuid::Uuid;

#[derive(Clone, Copy)]
enum Renewal {
    Success,
    Refuse,
    Error,
    Hang,
}
struct Port {
    renewal: Renewal,
    /// How long a successful renewal takes, and the run deadline that caps its window.
    delay: Duration,
    run_deadline: Option<Instant>,
    renewals: Mutex<Vec<Instant>>,
    calls: AtomicUsize,
    dropped: Arc<AtomicBool>,
    valid: bool,
    /// What `confirm_suspension` proves; `None` is a persistence error.
    suspension: Option<bool>,
    released: AtomicUsize,
}
#[async_trait]
impl WorkflowLeases for Port {
    async fn claim_io(
        &self,
        _: ActivationRequest,
        _: WorkflowWorkerId,
        _: LeasePolicy,
    ) -> AppResult<Option<ClaimedWorkflow>> {
        unreachable!()
    }
    async fn renew_io(
        &self,
        _: WorkflowFence,
        policy: LeasePolicy,
    ) -> AppResult<Option<LeaseWindow>> {
        let started = Instant::now();
        self.renewals.lock().unwrap().push(started);
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.renewal {
            Renewal::Success => {
                tokio::time::sleep(self.delay).await;
                let run_deadline = self
                    .run_deadline
                    .unwrap_or(started + Duration::from_secs(30));
                Ok(Some(LeaseWindow {
                    expires: (started + policy.duration()).min(run_deadline),
                    run_deadline,
                }))
            }
            Renewal::Refuse => Ok(None),
            Renewal::Error => Err(AppError::Database("injected".into())),
            Renewal::Hang => std::future::pending().await,
        }
    }
    async fn validate_io(&self, _: WorkflowFence, _: LeasePolicy) -> AppResult<bool> {
        Ok(self.valid)
    }
    async fn release_io(
        &self,
        _: WorkflowFence,
        _: LeasePolicy,
        _: LeaseReleaseCause,
    ) -> AppResult<bool> {
        assert!(
            self.dropped.load(Ordering::SeqCst),
            "actual future must be gone BEFORE cleanup"
        );
        self.released.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }
    async fn confirm_suspension(
        &self,
        _: WorkflowFence,
        _: crate::domain::workflow::WaitId,
    ) -> AppResult<bool> {
        self.suspension
            .ok_or_else(|| AppError::Database("injected".into()))
    }
}
struct Handler {
    timer: Pin<Box<tokio::time::Sleep>>,
    dropped: Arc<AtomicBool>,
}
impl Future for Handler {
    type Output = WorkflowHandlerResult;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.timer
            .as_mut()
            .poll(cx)
            .map(|_| Ok(serde_json::json!({"ok":true})))
    }
}
impl Drop for Handler {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}
fn fixture(renewal: Renewal, duration: u64) -> (Port, ClaimedWorkflow, Handler) {
    let dropped = Arc::new(AtomicBool::new(false));
    let scope = ActivationRequest {
        company: CompanyId::new(Uuid::new_v4()),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
        job: WorkflowJobId(Uuid::new_v4()),
    };
    let claim = ClaimedWorkflow {
        fence: WorkflowFence {
            scope,
            worker: WorkflowWorkerId(Uuid::new_v4()),
            generation: WorkflowGeneration(Uuid::new_v4()),
            attempt: WorkflowAttempt(1),
        },
        activation: ActivatedExecution {
            execution: scope.execution,
            step: StepId::parse("start").unwrap(),
            ordinal: 1,
            inputs: Value::Null,
            choice: None,
        },
        window: LeaseWindow {
            expires: Instant::now() + Duration::from_secs(3),
            run_deadline: Instant::now() + Duration::from_secs(30),
        },
        max_result_bytes: 100,
    };
    (
        Port {
            renewal,
            delay: Duration::ZERO,
            run_deadline: None,
            renewals: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
            dropped: dropped.clone(),
            valid: true,
            suspension: Some(true),
            released: AtomicUsize::new(0),
        },
        claim,
        Handler {
            timer: Box::pin(tokio::time::sleep(Duration::from_secs(duration))),
            dropped,
        },
    )
}
fn policy() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(3)).unwrap()
}
#[tokio::test(start_paused = true)]
async fn workflow_io_supervision_drops_actual_future_on_every_stop() {
    for (renewal, cancel, deadline, expires, reason) in [
        (Renewal::Success, 500, 20000, 3000, StopReason::Cancelled),
        (Renewal::Hang, 1500, 20000, 3000, StopReason::Cancelled),
        (Renewal::Refuse, 20000, 20000, 3000, StopReason::LeaseLost),
        (Renewal::Error, 20000, 20000, 3000, StopReason::Persistence),
        (Renewal::Hang, 20000, 20000, 3000, StopReason::Persistence),
        (Renewal::Hang, 20000, 20000, 1500, StopReason::LeaseLost),
        (Renewal::Success, 20000, 500, 3000, StopReason::Deadline),
    ] {
        let (port, mut claim, handler) = fixture(renewal, 10);
        claim.window.expires = Instant::now() + Duration::from_millis(expires);
        let result = supervise_io(
            &port,
            claim,
            policy(),
            Instant::now() + Duration::from_millis(deadline),
            handler,
            tokio::time::sleep(Duration::from_millis(cancel)),
        )
        .await;
        assert!(
            matches!(result,SupervisedResult::Stopped(actual) if actual==reason),
            "{result:?}"
        );
        assert!(port.dropped.load(Ordering::SeqCst));
    }
}
#[tokio::test(start_paused = true)]
async fn workflow_io_supervision_heartbeats_and_returns_only_fenced_bounded_result() {
    let (port, claim, handler) = fixture(Renewal::Success, 10);
    let fence = claim.fence;
    let result = supervise_io(
        &port,
        claim,
        policy(),
        Instant::now() + Duration::from_secs(20),
        handler,
        std::future::pending(),
    )
    .await;
    assert!(matches!(result,SupervisedResult::Ready(result) if result.fence==fence));
    assert!(port.calls.load(Ordering::SeqCst) >= 5);
    assert!(port.dropped.load(Ordering::SeqCst));
    for (valid, limit, reason) in [
        (false, 100, StopReason::LeaseLost),
        (true, 1, StopReason::OversizedResult),
    ] {
        let (mut port, mut claim, handler) = fixture(Renewal::Success, 0);
        port.valid = valid;
        claim.max_result_bytes = limit;
        let result = supervise_io(
            &port,
            claim,
            policy(),
            Instant::now() + Duration::from_secs(20),
            handler,
            std::future::pending(),
        )
        .await;
        assert!(matches!(result,SupervisedResult::Stopped(actual) if actual==reason));
    }
}
#[tokio::test(start_paused = true)]
async fn workflow_io_supervision_ready_result_loses_to_cancel_expiry_and_run_deadline() {
    for kind in 0..3 {
        let (port, mut claim, handler) = fixture(Renewal::Success, 0);
        if kind == 1 {
            claim.window.expires = Instant::now();
        }
        if kind == 2 {
            claim.window.run_deadline = Instant::now();
        }
        let cancel = async move {
            if kind == 0 {
                return;
            }
            std::future::pending::<()>().await
        };
        let result = supervise_io(
            &port,
            claim,
            policy(),
            Instant::now() + Duration::from_secs(20),
            handler,
            cancel,
        )
        .await;
        assert!(matches!(result, SupervisedResult::Stopped(_)));
    }
}

fn lease(seconds: u64) -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(seconds)).unwrap()
}

/// (k) A renewal that settles just inside its persistence deadline keeps the lease.
#[tokio::test(start_paused = true)]
async fn workflow_io_supervision_renewal_settling_inside_its_deadline_continues() {
    let policy = lease(3);
    let (mut port, claim, handler) = fixture(Renewal::Success, 5);
    port.delay = policy.persistence_deadline() - Duration::from_millis(1);
    let fence = claim.fence;
    let result = supervise_io(
        &port,
        claim,
        policy,
        Instant::now() + Duration::from_secs(20),
        handler,
        std::future::pending(),
    )
    .await;
    assert!(matches!(result, SupervisedResult::Ready(result) if result.fence == fence));
    assert!(port.calls.load(Ordering::SeqCst) >= 3);
}

/// (n) Windows are anchored at the operation's start: a claim that returns a full
/// deadline late renews at once, a renewal inside the remaining window keeps the lease,
/// and one slower than it ends as LeaseLost, never as a late Ready. T = d/2 for 3–6 s.
#[tokio::test(start_paused = true)]
async fn workflow_io_supervision_renews_at_once_after_a_full_deadline_claim() {
    for seconds in [3, 6] {
        let policy = lease(seconds);
        let deadline = policy.persistence_deadline();
        assert_eq!(deadline, policy.duration() / 2);
        for (delay, landed) in [(policy.attempt_bound(), true), (deadline, false)] {
            let (mut port, mut claim, handler) = fixture(Renewal::Success, 1);
            port.delay = delay;
            let entered = Instant::now();
            claim.window.expires = entered + (policy.duration() - deadline);
            let result = supervise_io(
                &port,
                claim,
                policy,
                entered + Duration::from_secs(20),
                handler,
                std::future::pending(),
            )
            .await;
            assert_eq!(
                port.renewals.lock().unwrap().first(),
                Some(&entered),
                "{seconds}s"
            );
            if landed {
                assert!(
                    matches!(result, SupervisedResult::Ready(_)),
                    "{seconds}s {result:?}"
                );
            } else {
                assert!(
                    matches!(result, SupervisedResult::Stopped(StopReason::LeaseLost)),
                    "{seconds}s {result:?}"
                );
            }
        }
    }
}

/// (p) A window capped at the run deadline cannot be extended: renew at most once in
/// the final stretch instead of looping on the global capacity lock.
#[tokio::test(start_paused = true)]
async fn workflow_io_supervision_stops_renewing_a_deadline_capped_window() {
    let policy = lease(3);
    let (mut port, mut claim, handler) = fixture(Renewal::Success, 10);
    let start = Instant::now();
    let run_deadline = start + Duration::from_secs(4);
    port.run_deadline = Some(run_deadline);
    claim.window.run_deadline = run_deadline;
    let result = supervise_io(
        &port,
        claim,
        policy,
        start + Duration::from_secs(20),
        handler,
        std::future::pending(),
    )
    .await;
    assert!(
        matches!(result, SupervisedResult::Stopped(StopReason::Deadline)),
        "{result:?}"
    );
    let renewals = port.renewals.lock().unwrap();
    let final_stretch = run_deadline - policy.persistence_deadline();
    assert!(
        renewals.iter().filter(|at| **at >= final_stretch).count() <= 1,
        "{renewals:?}"
    );
    assert_eq!(renewals.len(), 1, "{renewals:?}");
}

/// A handler whose own commit parked the step (04.11-D2 §3): the supervisor proves the exact
/// close and never completes or releases the parked attempt.
#[tokio::test(start_paused = true)]
async fn workflow_message_wait_supervisor_confirms_suspension_and_never_releases() {
    let wait = crate::domain::workflow::WaitId::new(Uuid::new_v4());
    for (suspension, expected) in [
        (Some(true), None),
        (Some(false), Some(StopReason::LeaseLost)),
        (None, Some(StopReason::Persistence)),
    ] {
        let (mut port, claim, _) = fixture(Renewal::Success, 1);
        port.suspension = suspension;
        port.dropped.store(true, Ordering::SeqCst);
        let parked = async move {
            WorkflowHandlerOutcome::Suspended(crate::application::workflow::lease::SuspendedWorkflow {
                wait,
            })
        };
        let result = supervise_io(
            &port,
            claim,
            policy(),
            Instant::now() + Duration::from_secs(20),
            parked,
            std::future::pending(),
        )
        .await;
        match expected {
            None => assert!(
                matches!(result, SupervisedResult::Suspended(actual) if actual == wait),
                "{result:?}"
            ),
            Some(reason) => assert!(
                matches!(result, SupervisedResult::Stopped(actual) if actual == reason),
                "{result:?}"
            ),
        }
        assert_eq!(port.released.load(Ordering::SeqCst), 0);
    }
}
