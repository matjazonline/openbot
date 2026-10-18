//! Durable internal event/timer transitions; human decisions have a separate authority.
use super::{
    activation::*,
    batch::{PureCompletion, validate_completion},
    completion::CommittedWorkflowStep,
    publication::PublishedBundle,
};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{CompanyId, RouteSelection, RunId, WaitId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowEventId(pub Uuid);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowEventName(pub String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowEventCorrelation(pub String);
/// A correlated event addressed to a run, never to an execution, so it can arrive before the
/// waiting step exists or parks (05-D1 §11, 05-D2 §11).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowEvent {
    pub company: CompanyId,
    pub run: RunId,
    /// Producer deduplication identity.
    pub id: WorkflowEventId,
    pub name: WorkflowEventName,
    pub correlation: WorkflowEventCorrelation,
    pub payload: Value,
}
/// `Unmatched` tells an ingress adapter to treat the message as ordinary ingress. A reused id
/// with different content, or a full inbox, is `Conflict`; an invalid payload is `BadRequest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventIngress {
    Recorded,
    Duplicate,
    Unmatched,
}
/// Unconsumed events one run may hold; producers serialize per run, so the bound is exact.
pub const MAX_UNCONSUMED_EVENTS: i64 = 128;
#[derive(Debug, Clone, PartialEq)]
pub struct ParkedWorkflow {
    pub wait: WaitId,
    pub deadline: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum WaitProgress {
    Pending,
    Completed(Box<CommittedWorkflowStep>),
    Expired(super::completion::CommitDisposition),
    Failed {
        disposition: super::completion::CommitDisposition,
        reason: WaitFailureReason,
    },
    Refused,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitFailureReason {
    ActivationLimit,
    InvalidOutput,
    /// A finality breach vetoed automatic continuation of a message step (04.11-D4 §M).
    EvidenceConflict,
}

#[async_trait]
pub trait WorkflowWaits: Send + Sync {
    async fn park_wait(&self, scope: ActivationRequest) -> AppResult<Option<ParkedWorkflow>>;
    /// Internal producer port. Never matches a human decision or a protected action
    /// (BRIEF-03.6), and authorizes no public ingress.
    async fn record_event(&self, event: WorkflowEvent) -> AppResult<EventIngress>;
    async fn resume_wait(&self, scope: ActivationRequest) -> AppResult<WaitProgress>;
    /// Bounded database scan; transitions each selected run independently in run-first order.
    async fn sweep_waits(&self, limit: u16) -> AppResult<u16>;
    /// Aggregate the execution's message deliveries into action states and receipts, one
    /// action per transaction, at most `MESSAGE_RECEIPT_PAGE` actions per call.
    async fn consume_message_receipts(
        &self,
        scope: crate::application::workflow::activation::ActivationRequest,
        policy: crate::application::workflow::lease::LeasePolicy,
    ) -> AppResult<u16>;
}

pub(crate) struct WaitSpecification {
    pub timer: bool,
    pub deadline: DateTime<Utc>,
    pub name: Option<String>,
    pub correlation: Option<String>,
}
pub(crate) fn specification(
    bundle: &PublishedBundle,
    activation: &ActivatedExecution,
) -> AppResult<WaitSpecification> {
    let step = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&activation.step)
        .ok_or_else(invalid)?;
    let timer = match step.step_type.as_str() {
        "wait.timer" => true,
        "wait.event" => false,
        _ => return Err(invalid()),
    };
    let text = |key| {
        activation
            .inputs
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(invalid)
    };
    let deadline = DateTime::parse_from_rfc3339(text("deadline")?)
        .map_err(|_| invalid())?
        .with_timezone(&Utc);
    Ok(WaitSpecification {
        timer,
        deadline,
        name: if timer {
            None
        } else {
            Some(text("event")?.to_owned())
        },
        correlation: if timer {
            None
        } else {
            Some(text("correlation")?.to_owned())
        },
    })
}
pub(crate) fn prepare(
    bundle: &PublishedBundle,
    activation: &ActivatedExecution,
    output: Value,
) -> AppResult<PureCompletion> {
    specification(bundle, activation)?;
    validate_completion(bundle, activation, output, RouteSelection::Success)
}
fn invalid() -> AppError {
    AppError::BadRequest("Invalid workflow wait".into())
}
