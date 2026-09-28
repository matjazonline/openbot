use super::*;
use crate::{
    app_error::AppError,
    application::workflow::{CompanyId, activation::*},
    domain::workflow::{ExecutionId, RunId, StepId},
};
use async_trait::async_trait;
use std::{
    pin::Pin,
    sync::{
        Arc,
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
    calls: AtomicUsize,
    dropped: Arc<AtomicBool>,
    valid: bool,
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.renewal {
            Renewal::Success => Ok(Some(LeaseWindow {
                expires: Instant::now() + policy.duration(),
                run_deadline: Instant::now() + Duration::from_secs(30),
            })),
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
        Ok(true)
    }
}
struct Handler {
    timer: Pin<Box<tokio::time::Sleep>>,
    dropped: Arc<AtomicBool>,
}
impl Future for Handler {
    type Output = AppResult<Value>;
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
            calls: AtomicUsize::new(0),
            dropped: dropped.clone(),
            valid: true,
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
