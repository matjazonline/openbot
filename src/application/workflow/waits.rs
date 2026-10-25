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

/// An accepted message that directly answers a message a workflow run published (p08 D1.5).
/// Ingress asks the run whether one of its `message.reply` waits awaits exactly this answer.
#[derive(Debug, Clone, PartialEq)]
pub struct IngressReply {
    pub company: CompanyId,
    /// The thread the reply was accepted into; the answered message must have been published there.
    pub thread: Uuid,
    /// The workflow-published message the reply answers (its resolved direct parent).
    pub answers: crate::domain::entities::message::CanonicalMessageId,
    /// The stored reply's author principal.
    pub responder: Option<Uuid>,
    /// Posted in-app by a member (`IngressOrigin::TrustedApplication`), not received as mail.
    pub in_app: bool,
    /// Producer identity, stable across redelivery of the same intent.
    pub event: WorkflowEventId,
    /// The reply as `MessageTriggerInput`.
    pub payload: Value,
    /// Until when a not-yet-parked wait may still be awaited (event before park).
    pub park_grace_until: DateTime<Utc>,
}

/// What became of a [`IngressReply`]. Only `Recorded`/`Duplicate` consume the message: the
/// run resumes and ingress admits nothing for it. `NotYet` asks to try again shortly.
/// `Undeliverable` is permanent and `Unmatched` is ordinary ingress; an `Err` from the port is
/// always worth retrying, including running out of time (p08 D1.5.5, F028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyIngress {
    Recorded(RunId),
    Duplicate(RunId),
    NotYet,
    Unmatched,
    /// The run awaits this answer but can never take it: its event id was reused for different
    /// content, the run's event inbox is full, or the payload fails the wait's schema. The reply
    /// is ordinary ingress, and the cause is logged.
    Undeliverable(&'static str),
}

/// The `message.reply` waits that can receive answers to messages `publishing` sent: those whose
/// correlation references that step's output (the registry admits no other shape).
pub fn awaited_reply_steps(
    definition: &crate::domain::workflow::WorkflowDefinition,
    publishing: &crate::domain::workflow::StepId,
) -> Vec<crate::domain::workflow::StepId> {
    use crate::application::workflow::triggers::MESSAGE_REPLY_EVENT;
    use crate::domain::workflow::Binding;
    let reply = Value::String(MESSAGE_REPLY_EVENT.into());
    definition
        .steps
        .iter()
        .filter(|(_, step)| {
            step.step_type.as_str() == "wait.event"
                && matches!(step.inputs.get("event"), Some(Binding::Literal(event)) if *event == reply)
                && matches!(step.inputs.get("correlation"),
                    Some(Binding::Reference(reference)) if reference.step_id() == Some(publishing))
        })
        .map(|(id, _)| id.clone())
        .collect()
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
    /// A saved model tool call's delivery settled; its parked agent job was reopened.
    Continued,
    /// A `flow.repeat` round ended without exit: the next round's child and wait now exist.
    NextRound,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitFailureReason {
    ActivationLimit,
    InvalidOutput,
    /// A finality breach vetoed automatic continuation of a message step (04.11-D4 §M).
    EvidenceConflict,
    /// The shared root budget refused the next `flow.repeat` round (07-D1 §D1.6).
    RootBudgetExhausted,
}

#[async_trait]
pub trait WorkflowWaits: Send + Sync {
    async fn park_wait(&self, scope: ActivationRequest) -> AppResult<Option<ParkedWorkflow>>;
    /// Internal producer port. Never matches a human decision or a protected action
    /// (BRIEF-03.6), and authorizes no public ingress.
    async fn record_event(&self, event: WorkflowEvent) -> AppResult<EventIngress>;
    /// Ingress's reply producer (p08 D1.5): records the reply only for an open `message.reply`
    /// wait on its correlation whose expected responder sent it, one response per wait.
    async fn deliver_reply(&self, delivery: IngressReply) -> AppResult<ReplyIngress>;
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
