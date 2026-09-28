//! Durable internal event/timer transitions; human decisions have a separate authority.
use super::{
    activation::*,
    batch::{PureCompletion, validate_completion},
    completion::CommittedWorkflowStep,
    publication::PublishedBundle,
};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{RouteSelection, WaitId};
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
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowSignal {
    pub scope: ActivationRequest,
    pub id: WorkflowEventId,
    pub name: WorkflowEventName,
    pub correlation: WorkflowEventCorrelation,
    pub payload: Value,
}
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
}

#[async_trait]
pub trait WorkflowWaits: Send + Sync {
    async fn park_wait(&self, scope: ActivationRequest) -> AppResult<Option<ParkedWorkflow>>;
    /// Internal producer port. Does not authorize a human decision or public ingress.
    async fn record_signal(&self, signal: WorkflowSignal) -> AppResult<bool>;
    async fn resume_wait(&self, scope: ActivationRequest) -> AppResult<WaitProgress>;
    /// Bounded database scan; transitions each selected run independently in run-first order.
    async fn sweep_waits(&self, limit: u16) -> AppResult<u16>;
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
