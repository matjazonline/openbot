//! Reusable single-slot worker. Production handler wiring belongs to the owning
//! action/agent subsystems; notification delivery is never a prerequisite.
use super::{
    activation::*, batch::*, children::*, completion::*, decisions::*, guard::*, lease::*,
    polling::*, supervise::*, waits::*,
};
use crate::application::app_error::{AppError, AppResult};
use crate::application::services::runtime_metrics::ActiveWorkflowJobs;
use crate::domain::monitoring::MonitoringService;
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
    /// A handler for an action-owned kind (`actions::marker_owned_effect_kind`) must
    /// perform effects only after a committed record of them: ActionService's marker-first
    /// dispatch, or the kind's documented equivalent there (`memory.save`'s intent committed
    /// with its park). Retirement treats "no marker" as "no effect" for those kinds.
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome;
}

pub struct WorkflowWorker<'a, P, H> {
    pub port: &'a P,
    pub handler: &'a H,
    /// Renders `decision.human` notices inside the park transaction. Without it the worker
    /// leaves decision steps unsupported.
    pub notices: Option<&'a DecisionNotices>,
    pub worker: WorkflowWorkerId,
    pub lease: LeasePolicy,
    pub poll: PollPolicy,
    /// Counts this slot in the runtime panel's active jobs while it executes one. `None` where
    /// nothing samples the process, as in tests.
    pub active_jobs: Option<&'a ActiveWorkflowJobs>,
    /// Counts confirmed guard violations (`workflow_guard_violations_total`). `None` in tests
    /// that do not observe metrics.
    pub monitoring: Option<&'a dyn MonitoringService>,
}
// Candidate operations may be confirmed once. Errors after a handler has run must
// propagate without re-entering that candidate, including errors from compensation.
enum ProcessFailure {
    Candidate(AppError),
    AfterHandler(AppError),
}

impl From<AppError> for ProcessFailure {
    fn from(error: AppError) -> Self {
        Self::Candidate(error)
    }
}

impl ProcessFailure {
    fn into_error(self) -> AppError {
        match self {
            Self::Candidate(error) | Self::AfterHandler(error) => error,
        }
    }
}

impl<P, H> WorkflowWorker<'_, P, H>
where
    P: WorkflowPolling
        + WorkflowFairPolling
        + WorkflowBatch
        + WorkflowWaits
        + WorkflowChildren
        + WorkflowDecisions
        + WorkflowLeases
        + WorkflowLeaseRecovery
        + WorkflowCompletion
        + super::channel_reply::WorkflowFinalizations
        + WorkflowGuardFailure,
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

    /// A guard violation from the worker's own write is acted on only if an immediate re-run
    /// trips a guard again: guards compare the clock at COMMIT, and a re-run that sees the moved
    /// clock takes its owned path (expiry, lease retirement, wait timeout) instead. A violation
    /// that repeats is a disagreement between Rust and SQL, and fails the run.
    pub async fn process(
        &self,
        candidate: PollCandidate,
        shutdown: &CancellationToken,
    ) -> AppResult<WorkDisposition> {
        let first = Box::pin(self.process_once(candidate, shutdown)).await;
        if !matches!(
            first,
            Err(ProcessFailure::Candidate(AppError::WorkflowGuard(_)))
        ) {
            return first.map_err(ProcessFailure::into_error);
        }
        match Box::pin(self.process_once(candidate, shutdown)).await {
            Err(ProcessFailure::Candidate(AppError::WorkflowGuard(violation))) => {
                let site = match candidate.work {
                    PollWork::ExpiredRun => GuardSite::RunExpiry,
                    PollWork::ExpiredLease | PollWork::ExhaustedPending => GuardSite::LeaseExpiry,
                    PollWork::Job | PollWork::Wait | PollWork::MessageReceipt => GuardSite::Step,
                };
                Box::pin(self.fail_on_guard(GuardFailure {
                    scope: candidate.scope,
                    violation,
                    site,
                    fence: None,
                }))
                .await
            }
            other => other.map_err(ProcessFailure::into_error),
        }
    }

    async fn process_once(
        &self,
        candidate: PollCandidate,
        shutdown: &CancellationToken,
    ) -> Result<WorkDisposition, ProcessFailure> {
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
            PollWork::MessageReceipt => {
                self.port
                    .consume_message_receipts(scope, self.lease)
                    .await?;
                return Ok(WorkDisposition::Done);
            }
            PollWork::Job => {}
        }
        // Held across the job and dropped on every way out of it, cancellation included.
        let _active = self.active_jobs.map(ActiveWorkflowJobs::enter);
        if !matches!(
            self.port.resume_finalization(scope).await?,
            super::channel_reply::FinalizationProgress::Unstaged
        ) {
            return Ok(WorkDisposition::Done);
        }
        let kind = self.port.step_kind(scope).await?;
        match kind.0.as_str() {
            "data.map" | "decision.rule" | "channel.no_reply" => {
                self.port
                    .advance_pure(scope, BatchBudget::new(16, Duration::from_millis(100))?)
                    .await?;
            }
            "wait.event" | "wait.timer" => {
                if self.port.park_wait(scope).await?.is_some() {
                    self.port.resume_wait(scope).await?;
                }
            }
            // The parent parks on its child; no worker waits for the child (07-D1 §D1.2).
            "workflow.call" | "flow.repeat" => {
                if self.port.park_child(scope).await?.is_some() {
                    self.port.resume_wait(scope).await?;
                }
            }
            "decision.human" if self.notices.is_some() => {
                let notices = self.notices.ok_or_else(|| {
                    crate::app_error::AppError::Internal("Decision notices unavailable".into())
                })?;
                if self.port.park_decision(scope, notices).await?.is_some() {
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
    ) -> Result<WorkDisposition, ProcessFailure> {
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
            return self
                .complete_confirmed(result)
                .await
                .map_err(ProcessFailure::AfterHandler);
        }
        Ok(WorkDisposition::Done)
    }

    /// The handler already ran, so a refused completion is confirmed here, under the claim's
    /// fence, and never re-raised: re-running the candidate would only find its own claim.
    async fn complete_confirmed(&self, result: FencedWorkflowResult) -> AppResult<WorkDisposition> {
        let fence = result.fence;
        let retry = result.clone();
        let first = self.port.complete_io(result).await;
        if !matches!(first, Err(AppError::WorkflowGuard(_))) {
            // Any other outcome, error included, propagates exactly as before.
            return first.map(|_| WorkDisposition::Done);
        }
        match self.port.complete_io(retry).await {
            Err(AppError::WorkflowGuard(violation)) => {
                Box::pin(self.fail_on_guard(GuardFailure {
                    scope: fence.scope,
                    violation,
                    site: GuardSite::Step,
                    fence: Some(fence),
                }))
                .await
            }
            other => other.map(|_| WorkDisposition::Done),
        }
    }

    async fn fail_on_guard(&self, failure: GuardFailure) -> AppResult<WorkDisposition> {
        let (scope, site) = (failure.scope, failure.site);
        let guard = failure.violation.guard().to_owned();
        let message = failure.violation.message().to_owned();
        let outcome = self.port.fail_run_on_guard(failure).await;
        let label = outcome.as_ref().map_or("error", |outcome| outcome.as_str());
        if let Some(monitoring) = self.monitoring {
            monitoring.increment_counter(
                "workflow_guard_violations_total",
                1,
                &[
                    ("guard", &guard),
                    ("site", site.as_str()),
                    ("outcome", label),
                ],
            );
        }
        match outcome {
            Ok(_) => {
                tracing::warn!(company_id = %scope.company.as_uuid(), run_id = %scope.run.as_uuid(),
                    guard = %guard, site = site.as_str(), outcome = label, error = %message,
                    "Workflow guard refused the worker's write twice");
                Ok(WorkDisposition::Done)
            }
            Err(error) => {
                tracing::error!(company_id = %scope.company.as_uuid(), run_id = %scope.run.as_uuid(),
                    guard = %guard, site = site.as_str(), error = %error, refused = %message,
                    "Workflow guard violation could not fail its run");
                Err(error)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkDisposition {
    Done,
    Deferred,
    Unsupported,
}
