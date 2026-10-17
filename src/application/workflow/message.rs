//! Provider-neutral `message.send` / `message.reply` (04.11-D2/D3/D4).
//!
//! One canonical message per destination; the publication transaction writes the `message`
//! marker first, then every thread, message, delivery, linkage row and (for a step that has
//! email) its effect wait, and closes the step's attempt in the same commit. Provider I/O
//! belongs to the delivery engine after commit. Handlers never see SMTP or SQL.
use crate::app_error::{AppError, AppResult};
use crate::application::transport::queue::NewDelivery;
use crate::application::transport::{BoundsError, CanonicalContent};
use crate::application::use_cases::thread::MessageWrite;
use crate::application::workflow::actions::*;
use crate::application::workflow::lease::{ClaimedWorkflow, WorkflowFence};
use crate::application::workflow::polling::WorkflowStepKind;
use crate::application::workflow::publication::{
    ActionEffect, ActionRecovery, ApprovedActionPolicy, ToolSnapshot,
};
use crate::application::workflow::registry::{MAX_MESSAGE_DESTINATIONS, ToolContract};
use crate::domain::workflow::{CompanyId, TypeName, WaitId};
use crate::entities::correlation::CorrelationId;
use crate::entities::message::CanonicalMessageId;
use crate::entities::value_objects::{ChannelSlug, CompanySlug, EmailAddress, MessageId};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

#[path = "message_runtime.rs"]
mod runtime;
pub use runtime::*;

/// The most message markers one execution may hold: the `workflow_action_retry_safe` and
/// `workflow_action_not_applied_available` proof ceiling, so messaging never truncates them.
/// The SQL backstop `workflow_message_admission_guard` names this constant.
pub const MAX_MESSAGE_ACTIONS_PER_EXECUTION: usize = 128;

/// Names the workflow catalogue reserves for the built-in messaging step types.
pub fn is_builtin_message_tool(name: &str) -> bool {
    MessageKind::parse(name).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Send,
    Reply,
}
impl MessageKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "message.send" => Some(Self::Send),
            "message.reply" => Some(Self::Reply),
            _ => None,
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Send => "message.send",
            Self::Reply => "message.reply",
        }
    }
}

/// The trusted built-in tool snapshot for one messaging kind. Fixed schemas, write effect,
/// reconcile recovery and no connection slot: publication verifies snapshots against this,
/// never against an HTTP registration.
pub fn builtin_message_snapshot(company: CompanyId, kind: MessageKind) -> AppResult<ToolSnapshot> {
    let (input_schema, output_schema) =
        crate::application::workflow::registry::message_tool_schemas(kind.name())
            .ok_or_else(|| AppError::Internal("Unknown built-in message kind".into()))?;
    let name = TypeName::parse(kind.name()).map_err(|_| invalid_message())?;
    Ok(ToolSnapshot {
        company_id: company,
        contract: ToolContract {
            name: name.clone(),
            connection: None,
            input_schema,
            output_schema,
        },
        policy: ApprovedActionPolicy {
            capability: name,
            policy_revision: 1,
            effect: ActionEffect::Write,
            recovery: ActionRecovery::Reconcile,
        },
    })
}

/// Whether a bundle snapshot is exactly the trusted built-in for its name.
pub fn is_trusted_message_snapshot(snapshot: &ToolSnapshot) -> AppResult<bool> {
    let Some(kind) = MessageKind::parse(snapshot.contract.name.as_str()) else {
        return Ok(false);
    };
    let trusted = builtin_message_snapshot(snapshot.company_id, kind)?;
    Ok(
        serde_json::to_value(snapshot).map_err(|_| invalid_message())?
            == serde_json::to_value(&trusted).map_err(|_| invalid_message())?,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DestinationKind {
    Channel,
    User,
    Reply,
}
impl DestinationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::User => "user",
            Self::Reply => "reply",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageDestinationInput {
    pub kind: DestinationKind,
    pub id: Uuid,
}

/// Frozen arguments, re-validated at runtime against the canonical owners' bounds.
#[derive(Debug, Clone)]
pub enum MessageArguments {
    Send {
        content: CanonicalContent,
        destinations: Vec<MessageDestinationInput>,
    },
    Reply {
        source: CanonicalMessageId,
        content: CanonicalContent,
    },
}
impl MessageArguments {
    pub fn parse(kind: MessageKind, arguments: &Value) -> AppResult<Self> {
        let content = parse_content(arguments.get("content"))?;
        match kind {
            MessageKind::Send => Ok(Self::Send {
                content,
                destinations: parse_destinations(arguments.get("destinations"))?,
            }),
            MessageKind::Reply => {
                let source = arguments
                    .get("source_message")
                    .and_then(Value::as_str)
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .ok_or_else(invalid_message)?;
                Ok(Self::Reply {
                    source: CanonicalMessageId::new(source),
                    content,
                })
            }
        }
    }
    pub fn content(&self) -> &CanonicalContent {
        match self {
            Self::Send { content, .. } | Self::Reply { content, .. } => content,
        }
    }
}

fn parse_content(content: Option<&Value>) -> AppResult<CanonicalContent> {
    let content = content
        .and_then(Value::as_object)
        .ok_or_else(invalid_message)?;
    let subject = match content.get("subject") {
        None => "",
        Some(value) => value.as_str().ok_or_else(invalid_message)?,
    };
    let body = content
        .get("body")
        .and_then(Value::as_str)
        .ok_or_else(invalid_message)?;
    let parsed = CanonicalContent::parse(subject, body).map_err(bounds)?;
    if parsed.body_text().trim().is_empty() {
        return Err(AppError::BadRequest(
            "A message body must contain text after sanitization".into(),
        ));
    }
    Ok(parsed)
}

fn parse_destinations(value: Option<&Value>) -> AppResult<Vec<MessageDestinationInput>> {
    let items = value
        .and_then(Value::as_array)
        .ok_or_else(invalid_message)?;
    if items.is_empty() || items.len() > MAX_MESSAGE_DESTINATIONS {
        return Err(AppError::BadRequest(format!(
            "A message names between 1 and {MAX_MESSAGE_DESTINATIONS} destinations"
        )));
    }
    let mut seen = BTreeSet::new();
    items
        .iter()
        .map(|item| {
            let kind = match item.get("kind").and_then(Value::as_str) {
                Some("user") => DestinationKind::User,
                Some("channel") => DestinationKind::Channel,
                _ => {
                    return Err(AppError::BadRequest(
                        "Message destinations are users or channels".into(),
                    ));
                }
            };
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or_else(invalid_message)?;
            if !seen.insert((kind, id)) {
                return Err(AppError::BadRequest(
                    "A destination may appear only once".into(),
                ));
            }
            Ok(MessageDestinationInput { kind, id })
        })
        .collect()
}

fn bounds(error: BoundsError) -> AppError {
    AppError::BadRequest(format!("Message content exceeds its bound: {error}"))
}
pub(crate) fn invalid_message() -> AppError {
    AppError::BadRequest("Invalid workflow message request".into())
}

/// A channel this message may be sent through or published into, as persistence read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRoute {
    pub id: Uuid,
    pub slug: ChannelSlug,
    pub name: String,
}

/// Facts persistence resolved for one invocation. Not a grant: publication re-reads and
/// locks every one of them before writing.
#[derive(Debug, Clone)]
pub struct MessageRoutes {
    pub company_slug: CompanySlug,
    pub correlation_id: CorrelationId,
    pub targets: Vec<RouteTarget>,
}

#[derive(Debug, Clone)]
pub enum RouteTarget {
    Channel {
        channel: Uuid,
    },
    User {
        user: Uuid,
        principal: Uuid,
        email: EmailAddress,
        sender: ChannelRoute,
    },
    ReplyEmail {
        source: CanonicalMessageId,
        thread: Uuid,
        sender: ChannelRoute,
        recipient: EmailAddress,
        in_reply_to: MessageId,
        references: Vec<MessageId>,
    },
    ReplyInternal {
        source: CanonicalMessageId,
        thread: Uuid,
        channel: Uuid,
    },
}

/// What publication must find unchanged under its locks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteCheck {
    Channel {
        channel: Uuid,
    },
    Member {
        user: Uuid,
        principal: Uuid,
        email: EmailAddress,
        sender: Uuid,
    },
    Source {
        source: CanonicalMessageId,
        thread: Uuid,
        channel: Uuid,
    },
}

#[derive(Debug, Clone)]
pub struct NewThread {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct PlannedDestination {
    pub kind: DestinationKind,
    pub destination_id: Uuid,
    pub new_thread: Option<NewThread>,
    pub thread_id: Uuid,
    pub message: MessageWrite,
    pub delivery: Option<NewDelivery>,
    pub principal_id: Option<Uuid>,
    pub recipient: Option<EmailAddress>,
    pub check: RouteCheck,
}

/// Everything one publication writes, composed without provider I/O.
#[derive(Debug, Clone)]
pub struct MessagePlan {
    pub destinations: Vec<PlannedDestination>,
}
impl MessagePlan {
    pub fn has_delivery(&self) -> bool {
        self.destinations
            .iter()
            .any(|destination| destination.delivery.is_some())
    }
    /// The receipt of an all-in-app publication; SQL recomputes and compares it exactly.
    pub fn accepted_result(&self) -> Value {
        json!({
            "status": "accepted",
            "messages": self.destinations.iter().map(|destination| json!({
                "destination": {"kind": destination.kind.as_str(), "id": destination.destination_id.to_string()},
                "message_id": destination.message.id.to_string(),
                "status": "accepted",
            })).collect::<Vec<_>>(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageOrigin {
    Step,
    ModelTool,
}
impl MessageOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::ModelTool => "model_tool",
        }
    }
}

/// The durable state of one invocation after (or instead of) publication.
pub enum MessagePublication {
    /// A receipt exists: all destinations in-app at commit, or a later derived receipt.
    Committed(ActionReceipt),
    /// The message step parked in its effect wait in the publication transaction.
    Suspended(WaitId),
    /// Deliveries are queued or in flight; no provider acceptance is claimed.
    Pending,
    /// Every destination is terminal and at least one outcome is unknown.
    Uncertain,
    ApprovalRequired,
    /// `MAX_MESSAGE_ACTIONS_PER_EXECUTION` reached; nothing was written.
    ActionLimit,
    /// A step re-entered an unresolved marker it cannot replay.
    PossibleDispatchExists,
}

#[async_trait]
pub trait MessageActionPreparation: Send + Sync {
    async fn message_step_request(
        &self,
        claim: &ClaimedWorkflow,
        kind: &WorkflowStepKind,
    ) -> AppResult<ActionRequest>;
    async fn message_tool_request(
        &self,
        scope: ActionScope,
        kind: MessageKind,
        arguments: Value,
    ) -> AppResult<ActionRequest>;
    /// Read-only resolution of destinations and the reply source. Errors are refusals.
    async fn message_routes(
        &self,
        request: &ActionRequest,
        arguments: &MessageArguments,
    ) -> AppResult<MessageRoutes>;
}

#[async_trait]
pub trait MessageDispatch: Send + Sync {
    /// Existing durable state under current authority, or None when nothing was published.
    async fn observe(
        &self,
        request: &ActionDispatchRequest,
        origin: MessageOrigin,
    ) -> AppResult<Option<MessagePublication>>;
    async fn publish(
        &self,
        request: &ActionDispatchRequest,
        origin: MessageOrigin,
        plan: &MessagePlan,
    ) -> AppResult<MessagePublication>;
}

/// A direct agent message tool call (port seam; the agent host is phase 06/08).
pub struct MessageToolCall {
    pub kind: MessageKind,
    pub arguments: Value,
    pub call: ModelToolCallId,
}

/// What a direct tool observes. Pending never claims provider acceptance.
pub enum MessageObservation {
    Committed(ActionReceipt),
    Pending,
    Uncertain,
    ApprovalRequired,
    ActionLimit,
}

pub(crate) fn tool_scope(fence: WorkflowFence) -> ActionScope {
    ActionScope {
        company: fence.scope.company,
        run: fence.scope.run,
        execution: fence.scope.execution,
    }
}
