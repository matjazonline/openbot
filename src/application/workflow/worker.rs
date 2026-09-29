//! Reusable single-slot worker. Production handler wiring belongs to the owning
//! action/agent subsystems; notification delivery is never a prerequisite.
use super::{activation::*, batch::*, completion::*, lease::*, polling::*, supervise::*, waits::*};
use crate::application::app_error::AppResult;
use async_trait::async_trait;
use std::time::Duration;
use tokio::{
    sync::Notify,
    time::{Instant, sleep_until},
};
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait WorkflowHandler: Send + Sync {
    /// Pure, nonblocking capability decision. Missing support spends no attempt.
    fn supports(&self, kind: &WorkflowStepKind) -> bool;
    /// Effect permission, receipts and replay safety remain the handler's contract.
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerResult;
}

pub struct WorkflowWorker<'a, P, H> {
    pub port: &'a P,
    pub handler: &'a H,
    pub worker: WorkflowWorkerId,
    pub lease: LeasePolicy,
    pub poll: PollPolicy,
}
impl<P, H> WorkflowWorker<'_, P, H>
where
    P: WorkflowPolling
        + WorkflowFairPolling
        + WorkflowBatch
        + WorkflowWaits
        + WorkflowLeases
        + WorkflowLeaseRecovery
        + WorkflowCompletion,
    H: WorkflowHandler,
{
    /// Own one actual operation at a time. Dropping the loop drops its in-flight
    /// future; an interrupted committed claim expires through the existing ledger.
    pub async fn run(&self, wakeup: &Notify, shutdown: &CancellationToken) {
        loop {
            let page = tokio::select! { biased;
                _ = shutdown.cancelled() => return,
                result = self.port.poll_fair(self.worker, self.poll.page_size()) => result,
            };
            match page {
                Ok(page) => {
                    for candidate in page.candidates {
                        let result = tokio::select! { biased;
                            _ = shutdown.cancelled() => return,
                            result = Box::pin(self.process(candidate, shutdown)) => result,
                        };
                        if matches!(result, Ok(WorkDisposition::Unsupported))
                            && let Some(ticket) = page.demand
                            && let Err(error) = self.port.withdraw_demand(self.worker, ticket).await
                        {
                            tracing::warn!(error = %error, "Workflow demand withdrawal failed");
                        }
                        if matches!(result, Ok(WorkDisposition::Deferred)) {
                            break; // Outstanding supported demand must get the very next retry.
                        }
                        if let Err(error) = result {
                            tracing::warn!(company_id = %candidate.scope.company.as_uuid(),
                                run_id = %candidate.scope.run.as_uuid(), error = %error,
                                "Workflow poll candidate failed");
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "Workflow poll failed; periodic retry remains active");
                }
            }
            // Even a full unchanged page pays a positive delay. Hints coalesce,
            // and a storm cannot exceed one scan per 50ms. No detached listener.
            let next = Instant::now() + self.poll.interval();
            let earliest = Instant::now() + Duration::from_millis(50);
            tokio::select! { biased;
                _ = shutdown.cancelled() => return,
                _ = sleep_until(next) => {},
                _ = wakeup.notified() => {
                    tokio::select! { biased;
                        _ = shutdown.cancelled() => return,
                        _ = sleep_until(earliest) => {},
                    }
                }
            }
        }
    }

    pub async fn process(
        &self,
        candidate: PollCandidate,
        shutdown: &CancellationToken,
    ) -> AppResult<WorkDisposition> {
        let scope = candidate.scope;
        match candidate.work {
            PollWork::ExpiredRun => {
                self.port.expire_run(scope).await?;
                return Ok(WorkDisposition::Done);
            }
            PollWork::Wait => {
                self.port.resume_wait(scope).await?;
                return Ok(WorkDisposition::Done);
            }
            PollWork::ExpiredLease => {
                self.port.retire_expired_io(scope, self.lease).await?;
                return Ok(WorkDisposition::Done);
            }
            PollWork::ExhaustedPending => {
                self.port.retire_exhausted_work(scope, self.lease).await?;
                return Ok(WorkDisposition::Done);
            }
            PollWork::Job => {}
        }
        let kind = self.port.step_kind(scope).await?;
        match kind.0.as_str() {
            "data.map" | "decision.rule" => {
                self.port
                    .advance_pure(scope, BatchBudget::new(16, Duration::from_millis(100))?)
                    .await?;
            }
            "wait.event" | "wait.timer" => {
                if self.port.park_wait(scope).await?.is_some() {
                    self.port.resume_wait(scope).await?;
                }
            }
            _ if is_io_kind(&kind.0) && self.handler.supports(&kind) => {
                return self.execute_io(scope, &kind, shutdown).await;
            }
            _ => return Ok(WorkDisposition::Unsupported),
        }
        Ok(WorkDisposition::Done)
    }

    async fn execute_io(
        &self,
        scope: ActivationRequest,
        kind: &WorkflowStepKind,
        shutdown: &CancellationToken,
    ) -> AppResult<WorkDisposition> {
        let Some(claim) = self.port.claim_io(scope, self.worker, self.lease).await? else {
            return Ok(WorkDisposition::Deferred);
        };
        // Box the external boundary: stock-stack tests must remain safe as real
        // action/agent implementations replace the scripted handler.
        let handler = Box::pin(self.handler.execute(kind, &claim));
        let result = supervise_io(
            self.port,
            claim.clone(),
            self.lease,
            Instant::now() + Duration::from_secs(60),
            handler,
            shutdown.cancelled(),
        )
        .await;
        if let SupervisedResult::Ready(result) = result {
            self.port.complete_io(result).await?;
        }
        Ok(WorkDisposition::Done)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkDisposition {
    Done,
    Deferred,
    Unsupported,
}
