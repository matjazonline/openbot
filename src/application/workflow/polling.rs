//! Discovery is a hint, never ownership. Every transition rechecks its durable scope.
use super::{activation::ActivationRequest, lease::WorkflowWorkerId};
use crate::application::app_error::{AppError, AppResult};
use async_trait::async_trait;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollCursor(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollWork {
    Job,
    ExpiredLease,
    ExpiredRun,
    ExhaustedPending,
    Wait,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollCandidate {
    pub scope: ActivationRequest,
    pub work: PollWork,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStepKind(pub String);
#[derive(Debug)]
pub struct PollPage {
    pub candidates: Vec<PollCandidate>,
    pub next: Option<PollCursor>,
}
#[derive(Debug, Clone, Copy)]
pub struct PollPolicy {
    interval: Duration,
    page_size: u16,
}
impl PollPolicy {
    pub fn new(interval: Duration, page_size: u16) -> AppResult<Self> {
        if !(Duration::from_millis(50)..=Duration::from_secs(30)).contains(&interval)
            || !(1..=128).contains(&page_size)
        {
            return Err(AppError::BadRequest("Invalid workflow poll policy".into()));
        }
        Ok(Self {
            interval,
            page_size,
        })
    }
    pub fn interval(self) -> Duration {
        self.interval
    }
    pub fn page_size(self) -> u16 {
        self.page_size
    }
}
#[async_trait]
pub trait WorkflowPolling: Send + Sync {
    /// Diagnostic/recovery scan, not the worker scheduling policy.
    /// Scan strictly after the cursor; an empty final page resets it. A restarted
    /// worker starts at None, so losing the cursor never acknowledges durable work.
    async fn poll_work(&self, after: Option<PollCursor>, limit: u16) -> AppResult<PollPage>;
    /// Atomically retire an overdue active run and its owned work.
    async fn expire_run(&self, scope: ActivationRequest) -> AppResult<bool>;
    /// Resolve from the admitted immutable bundle, not mutable registry selection.
    async fn step_kind(&self, scope: ActivationRequest) -> AppResult<WorkflowStepKind>;
}

/// Durable fair discovery; each worker identity has an independent traversal.
#[async_trait]
pub trait WorkflowFairPolling: Send + Sync {
    /// At most one company's bounded page, or this worker's outstanding demand.
    /// Implementations finish within two seconds, including pool acquisition.
    async fn poll_fair(&self, worker: WorkflowWorkerId, limit: u16) -> AppResult<FairPollPage>;
    /// Withdraw only this requester's matching hint (e.g. local capability loss).
    async fn withdraw_demand(
        &self,
        worker: WorkflowWorkerId,
        ticket: DemandTicket,
    ) -> AppResult<()>;
}

/// Poll <=30s + discovery <=2s + kind lookup <=2s + claim <=1s, plus10s
/// responsiveness margin. Only FREE-capacity service starts this clock.
pub const FAIR_OPPORTUNITY: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DemandTicket(pub i64);
#[derive(Debug)]
pub struct FairPollPage {
    pub candidates: Vec<PollCandidate>,
    pub demand: Option<DemandTicket>,
}
