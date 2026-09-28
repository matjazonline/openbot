//! Discovery is a hint, never ownership. Every transition rechecks its durable scope.
use super::activation::ActivationRequest;
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
    /// Scan strictly after the cursor; an empty final page resets it. A restarted
    /// worker starts at None, so losing the cursor never acknowledges durable work.
    async fn poll_work(&self, after: Option<PollCursor>, limit: u16) -> AppResult<PollPage>;
    /// Resolve from the admitted immutable bundle, not mutable registry selection.
    async fn step_kind(&self, scope: ActivationRequest) -> AppResult<WorkflowStepKind>;
}
