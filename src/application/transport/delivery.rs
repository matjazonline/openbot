//! Delivery intent, rendered parts, and what a provider said about one send.
//!
//! A delivery is a durable attempt to expose one canonical message through one destination. This
//! module holds the vocabulary for planning them (which is pure policy), for freezing what will be
//! sent (which is deterministic rendering), and for recording what came back (which is where
//! ambiguity has to be preserved rather than collapsed into success or failure).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use uuid::Uuid;

use crate::{
    entities::{
        correlation::CorrelationId,
        message::CanonicalMessageId,
        transport::{
            ChannelBinding, ChannelBindingId, DeliveryId, DeliveryPartStatus, DeliveryPurpose,
            ExternalDestination, ExternalMessageKey, FailureClass, TransportKind, bounded_string,
        },
        value_objects::{EmailAddress, MessageId},
    },
    transport::{bounded::BoundsError, ingress::CanonicalContent},
};

/// The most parts one logical delivery may be split into. Email renders one; a chat provider with
/// a per-message length limit renders several, and an unbounded split is a way to turn one large
/// message into a rate-limit outage.
pub const MAX_DELIVERY_PARTS: usize = 50;

/// Long enough to hold a purpose, a UUID and a full RFC-length address, because an explicitly
/// named destination is part of the key -- see [`DeliveryIntent::stable_key`].
pub const MAX_DELIVERY_KEY_BYTES: usize = 512;
pub const MAX_PART_KEY_BYTES: usize = 200;
pub const MAX_FAILURE_DETAIL_BYTES: usize = 512;
pub const MAX_CONTENT_DIGEST_BYTES: usize = 128;

/// The largest rendered payload one part may carry, serialized.
pub const MAX_PART_PAYLOAD_BYTES: usize = 256 * 1024;

/// The version of the envelope shape handed to a transport adapter.
pub const DELIVERY_ENVELOPE_VERSION: u8 = 1;

bounded_string!(DeliveryKey, MAX_DELIVERY_KEY_BYTES);
bounded_string!(PartKey, MAX_PART_KEY_BYTES);
bounded_string!(FailureDetail, MAX_FAILURE_DETAIL_BYTES);
bounded_string!(ContentDigest, MAX_CONTENT_DIGEST_BYTES);

impl ContentDigest {
    /// The digest a reconciliation lookup matches on. Content, not credentials: this value is
    /// allowed to travel in provider metadata, so it must be derived from the rendered body alone.
    pub fn sha256_of(bytes: &[u8]) -> Self {
        let digest = format!("{:x}", Sha256::digest(bytes));
        Self::parse(digest).expect("a hex sha256 digest is within its bound")
    }
}

/// Where a delivery is going.
///
/// Either a binding this platform owns -- which is how a message reaches a channel's own interface
/// -- or an address named by the command that created it, which is how outreach reaches someone
/// with no binding at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeliveryDestination {
    Binding(ChannelBindingId),
    External(ExternalDestination),
}

impl DeliveryDestination {
    /// The destination as one stable, comparable string.
    ///
    /// Case-folded for the transports whose addressing is case-insensitive, so the same recipient
    /// written two ways derives one key rather than two deliveries.
    pub(crate) fn key_fragment(&self) -> String {
        match self {
            Self::Binding(binding_id) => format!("binding:{binding_id}"),
            Self::External(ExternalDestination::Email(address)) => {
                format!("email:{}", address.trim().to_ascii_lowercase())
            }
        }
    }
}

/// One durable intention to deliver one canonical message to one destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryIntent {
    pub message_id: CanonicalMessageId,
    /// The binding the message arrived on, or that the producing channel speaks through. Recorded
    /// so fan-out can exclude it: delivering a message back to its own source is an echo.
    pub source_binding_id: ChannelBindingId,
    pub destination: DeliveryDestination,
    pub purpose: DeliveryPurpose,
    /// Stable within the destination binding. Two workers computing this for the same logical
    /// delivery compute the same string, which is what makes queue creation idempotent.
    pub key: DeliveryKey,
}

impl DeliveryIntent {
    /// The key one logical delivery of `message_id` to `destination` for `purpose` always has.
    ///
    /// Derived rather than random on purpose: a retry of the planning step must produce the key
    /// that already exists, so the unique index absorbs it instead of enqueuing a second send.
    ///
    /// The destination is part of the key, not just of the row. Deduplication is by
    /// `(destination_binding_id, key)`, and an explicitly named external destination has no
    /// binding id at all -- so without it, two different outreach recipients on one message would
    /// share a key and one of them would silently never be written.
    pub fn stable_key(
        purpose: DeliveryPurpose,
        message_id: CanonicalMessageId,
        destination: &DeliveryDestination,
    ) -> DeliveryKey {
        super::compose::delivery_key(purpose, &format!("message:{message_id}"), destination)
    }

    /// The destination binding, when this delivery goes through one.
    pub const fn destination_binding(&self) -> Option<ChannelBindingId> {
        match &self.destination {
            DeliveryDestination::Binding(binding_id) => Some(*binding_id),
            DeliveryDestination::External(_) => None,
        }
    }
}

/// One binding offered to the planner, with the one fact about its installation that fan-out needs.
///
/// The usability of the provider account is passed in rather than looked up, because the planner is
/// pure: it decides, and the caller -- which already joined the installation to load the binding --
/// supplies the facts.
#[derive(Debug, Clone, Copy)]
pub struct DeliveryCandidate<'a> {
    pub binding: &'a ChannelBinding,
    /// Whether the provider account behind this binding can currently be used. Always `true` for a
    /// transport that needs no installation.
    pub installation_usable: bool,
}

impl<'a> DeliveryCandidate<'a> {
    /// A binding on a transport that needs no provider account: email.
    pub const fn deployment(binding: &'a ChannelBinding) -> Self {
        Self {
            binding,
            installation_usable: true,
        }
    }
}

/// What the planner is being asked to fan out.
#[derive(Debug, Clone, Copy)]
pub struct DeliveryPlanRequest<'a> {
    pub message_id: CanonicalMessageId,
    pub source_binding_id: ChannelBindingId,
    pub purpose: DeliveryPurpose,
    /// Every binding that could conceivably receive this message. The planner removes the ones
    /// policy excludes; it never widens the set.
    pub candidates: &'a [DeliveryCandidate<'a>],
    /// Destinations the command named outright -- an outreach recipient. These are retained
    /// regardless of binding policy, because they are the point of the command rather than a
    /// policy-driven mirror.
    pub explicit: &'a [ExternalDestination],
}

/// The application's own fan-out rule, as a pure function of the request.
///
/// Every exclusion here is a decision `docs/transport_architecture.md` records:
///
/// - the source binding never receives its own message (that is an echo);
/// - a binding that is not active, or whose installation is not usable, receives nothing;
/// - a binding whose delivery policy is reply-only receives no conversation-initiating purpose;
/// - explicitly named destinations survive all of the above.
pub fn plan_deliveries(request: &DeliveryPlanRequest<'_>) -> Vec<DeliveryIntent> {
    let intent = |destination: DeliveryDestination| DeliveryIntent {
        message_id: request.message_id,
        source_binding_id: request.source_binding_id,
        key: DeliveryIntent::stable_key(request.purpose, request.message_id, &destination),
        destination,
        purpose: request.purpose,
    };

    let mut intents: Vec<DeliveryIntent> = request
        .candidates
        .iter()
        .filter(|candidate| eligible(request, candidate))
        .map(|candidate| intent(DeliveryDestination::Binding(candidate.binding.id)))
        .collect();

    intents.extend(
        request
            .explicit
            .iter()
            .cloned()
            .map(|destination| intent(DeliveryDestination::External(destination))),
    );
    intents
}

fn eligible(request: &DeliveryPlanRequest<'_>, candidate: &DeliveryCandidate<'_>) -> bool {
    candidate.binding.id != request.source_binding_id
        && candidate.binding.is_active()
        && candidate.installation_usable
        && request
            .purpose
            .permitted_by(candidate.binding.delivery_policy)
}

/// Which zero-based part of a multi-part delivery this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PartIndex(u16);

impl PartIndex {
    pub const fn new(index: u16) -> Self {
        Self(index)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A rendered payload, bounded and versioned, whose shape only its own adapter understands.
///
/// The application never looks inside. What it guarantees is that the value is small enough to
/// store, carries the transport and version it was written for, and comes back out through a
/// fallible decode -- so a payload written by a newer renderer is an error at the seam rather than
/// a misread field halfway through a provider call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransportPayload {
    transport: TransportKind,
    version: u16,
    body: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PayloadError {
    #[error("could not encode a {transport} delivery payload: {detail}")]
    Encode {
        transport: TransportKind,
        detail: String,
    },
    #[error(
        "a {transport} delivery payload of version {found} cannot be read as version {expected}"
    )]
    UnknownVersion {
        transport: TransportKind,
        expected: u16,
        found: u16,
    },
    #[error("a {expected} renderer cannot read a {found} delivery payload")]
    WrongTransport {
        expected: TransportKind,
        found: TransportKind,
    },
    #[error("could not decode a {transport} delivery payload: {detail}")]
    Decode {
        transport: TransportKind,
        detail: String,
    },
    #[error(transparent)]
    Bounds(#[from] BoundsError),
}

impl TransportPayload {
    pub fn encode<T: Serialize>(
        transport: TransportKind,
        version: u16,
        value: &T,
    ) -> Result<Self, PayloadError> {
        let body = serde_json::to_value(value).map_err(|error| PayloadError::Encode {
            transport,
            detail: error.to_string(),
        })?;
        Self::from_value(transport, version, body)
    }

    fn from_value(
        transport: TransportKind,
        version: u16,
        body: serde_json::Value,
    ) -> Result<Self, PayloadError> {
        let encoded = serde_json::to_vec(&body).map_err(|error| PayloadError::Encode {
            transport,
            detail: error.to_string(),
        })?;
        if encoded.len() > MAX_PART_PAYLOAD_BYTES {
            return Err(BoundsError::TooLarge {
                field: "delivery part payload",
                actual: encoded.len(),
                max: MAX_PART_PAYLOAD_BYTES,
            }
            .into());
        }
        Ok(Self {
            transport,
            version,
            body,
        })
    }

    /// Reads the payload back as the adapter's own type, refusing every mismatch it can detect.
    pub fn decode<T: serde::de::DeserializeOwned>(
        &self,
        transport: TransportKind,
        version: u16,
    ) -> Result<T, PayloadError> {
        if self.transport != transport {
            return Err(PayloadError::WrongTransport {
                expected: transport,
                found: self.transport,
            });
        }
        if self.version != version {
            return Err(PayloadError::UnknownVersion {
                transport,
                expected: version,
                found: self.version,
            });
        }
        serde_json::from_value(self.body.clone()).map_err(|error| PayloadError::Decode {
            transport,
            detail: error.to_string(),
        })
    }

    pub const fn transport(&self) -> TransportKind {
        self.transport
    }

    pub const fn version(&self) -> u16 {
        self.version
    }
}

/// Stored payloads are untrusted on the way back in: the bound is applied again rather than
/// assumed of whatever wrote the row.
impl<'de> Deserialize<'de> for TransportPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            transport: TransportKind,
            version: u16,
            body: serde_json::Value,
        }

        let raw = Raw::deserialize(deserializer)?;
        Self::from_value(raw.transport, raw.version, raw.body).map_err(serde::de::Error::custom)
    }
}

/// One frozen piece of a delivery: what will be sent, under which stable key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedPart {
    pub index: PartIndex,
    /// Stable across re-renders of the same delivery, so a resumed send addresses the same part
    /// rather than creating a second one.
    pub key: PartKey,
    pub payload: TransportPayload,
    /// What a reconciliation lookup compares against when a provider outcome was ambiguous.
    pub digest: ContentDigest,
}

/// The transport-specific facts a renderer needs on top of the canonical content.
///
/// An enum rather than a bag of optional fields, for the same reason
/// [`crate::transport::IngressPolicyFacts`] is one: there is no way to spell "this Slack post has
/// a Cc line", because there is no way to attach mail addressing to anything but the email
/// variant. Each arm is owned by the application because the delivery worker assembles it; the
/// adapter that consumes it reads its own arm and nothing else.
///
/// Deliberately *not* persisted. Parts are rendered and frozen before the first provider call, so
/// this exists only between resolving a destination and freezing what will be sent -- which is
/// what keeps the delivery row down to identifiers and the part payload down to wire content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryContext {
    Email(EmailDeliveryContext),
}

impl DeliveryContext {
    /// The transport this context can be rendered for. A renderer compares it against its own
    /// kind rather than assuming the worker handed it the right arm.
    pub const fn transport(&self) -> TransportKind {
        match self {
            Self::Email(_) => TransportKind::Email,
        }
    }

    pub const fn email(&self) -> Option<&EmailDeliveryContext> {
        match self {
            Self::Email(context) => Some(context),
        }
    }
}

/// Everything mail addressing needs that a canonical message does not carry.
///
/// The `From` mailbox arrives already resolved from the sending interface. The adapter this
/// replaces rebuilt it from a channel slug, a company slug and the configured domain at send time,
/// which meant a renderer could invent an address for a channel that had none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailDeliveryContext {
    pub from: EmailAddress,
    /// The display name on the `From:` line: the channel's name, or the platform's for a notice.
    pub from_name: Option<String>,
    pub recipient_to: EmailAddress,
    pub recipients_cc: Vec<EmailAddress>,
    pub in_reply_to: Option<MessageId>,
    pub references: Vec<MessageId>,
    /// Loop control for mail that one channel's agent sends, absent for a platform notice.
    ///
    /// `None` is what makes a bounce or a stop notice unanswerable: without it the renderer emits
    /// no `X-MailAgents-*` headers, so the receiving side has no hop count to continue and the
    /// notice ends the chain instead of extending it.
    pub relay: Option<EmailRelayTrace>,
}

/// The inter-channel hop budget one piece of mail carries on the wire.
///
/// A struct rather than three loose fields because `hop_count` and the channel ids travel
/// together: a trace without its hop count is a loop with no bound on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailRelayTrace {
    /// The channel this mail goes out as, which is also the channel a recipient must not be.
    pub source_channel_id: Uuid,
    /// Hops already taken. The renderer stamps `hop_count + 1`; ingress refuses beyond
    /// [`crate::transport::MAX_INGRESS_HOPS`].
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
}

/// A delivery with its destination resolved, ready for one transport's renderer.
///
/// Produced only after destination resolution: the point of the split is that a renderer is handed
/// a real endpoint rather than being left to invent a channel name or an address, which is what the
/// adapter this replaces did. It carries no parts -- rendering them is what it is *for*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryEnvelope {
    pub version: u8,
    pub delivery_id: DeliveryId,
    pub intent: DeliveryIntent,
    pub transport: TransportKind,
    /// The chain this delivery belongs to, inherited from whatever produced it and stamped onto
    /// the wire where the protocol permits, so a recipient stays on the same trail.
    pub correlation_id: CorrelationId,
    pub content: CanonicalContentV1,
    pub context: DeliveryContext,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a {transport} renderer cannot read a {context} delivery context")]
pub struct ContextMismatch {
    pub transport: TransportKind,
    pub context: TransportKind,
}

impl DeliveryEnvelope {
    /// Refuses an envelope whose context does not match the transport it names, rather than
    /// leaving the renderer to discover it: the two arrive from different places -- the transport
    /// from the stored row, the context from whatever resolved the destination -- and a mismatch
    /// means the worker paired the wrong adapter with the wrong endpoint.
    pub fn new(
        delivery_id: DeliveryId,
        intent: DeliveryIntent,
        transport: TransportKind,
        correlation_id: CorrelationId,
        content: &CanonicalContent,
        context: DeliveryContext,
    ) -> Result<Self, ContextMismatch> {
        if context.transport() != transport {
            return Err(ContextMismatch {
                transport,
                context: context.transport(),
            });
        }
        Ok(Self {
            version: DELIVERY_ENVELOPE_VERSION,
            delivery_id,
            intent,
            transport,
            correlation_id,
            content: CanonicalContentV1::from(content),
            context,
        })
    }
}

/// The protocol-neutral content an envelope carries, in its stored shape.
///
/// A separate type from [`CanonicalContent`] because this one is written down: it is versioned so
/// a later shape change is a decision rather than a silent reinterpretation of stored rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalContentV1 {
    pub version: u8,
    pub subject: String,
    pub body_text: String,
}

impl From<&CanonicalContent> for CanonicalContentV1 {
    fn from(content: &CanonicalContent) -> Self {
        Self {
            version: 1,
            subject: content.subject().to_string(),
            body_text: content.body_text().to_string(),
        }
    }
}

/// What one external request produced.
///
/// This is the whole reason [`crate::transport::TransportSender`] does not return `AppResult`: a
/// bare `Err` erases the difference between "the provider definitely refused this", "the provider
/// asked us to wait 30 seconds" and "the connection dropped after the request went out, so it may
/// well have been accepted". The third case must never be retried automatically, and a `Result`
/// gives a caller no way to tell it from the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderSendOutcome {
    /// The provider accepted it, and said so.
    Delivered {
        /// The provider's own key for what it stored, when it returned one.
        provider_key: Option<ExternalMessageKey>,
    },
    /// Refused with an explicit wait. The delay is the provider's, not a guess.
    RetryAfter {
        retry_after: Duration,
        class: FailureClass,
        detail: FailureDetail,
    },
    /// Definitely not accepted, and worth trying again later.
    Retryable {
        class: FailureClass,
        detail: FailureDetail,
    },
    /// The request may or may not have been accepted. Reconcile; never blind-retry.
    OutcomeUnknown {
        class: FailureClass,
        detail: FailureDetail,
    },
    /// Definitively rejected. Re-sending the same payload cannot succeed.
    Terminal {
        class: FailureClass,
        detail: FailureDetail,
    },
}

impl ProviderSendOutcome {
    /// The wait the provider asked for, if it asked for one.
    pub const fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RetryAfter { retry_after, .. } => Some(*retry_after),
            _ => None,
        }
    }

    /// Whether this outcome may be sent again without risking a duplicate.
    ///
    /// False for [`Self::OutcomeUnknown`] specifically: that is the case a duplicate comes from.
    pub const fn is_safely_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. } | Self::RetryAfter { .. })
    }

    pub const fn class(&self) -> Option<FailureClass> {
        match self {
            Self::Delivered { .. } => None,
            Self::RetryAfter { class, .. }
            | Self::Retryable { class, .. }
            | Self::OutcomeUnknown { class, .. }
            | Self::Terminal { class, .. } => Some(*class),
        }
    }
}

/// How one provider outcome moves the part it answers, and what that costs the delivery.
///
/// The mapping is stated here, once, rather than inside the SQL that applies it: which outcomes
/// are safe to send again, which are terminal, and which cost an attempt is a policy decision, and
/// a policy decision spread across four `UPDATE` statements is one that drifts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartTransition {
    pub status: DeliveryPartStatus,
    /// The provider's key for what it stored, when the outcome carried one.
    pub provider_key: Option<ExternalMessageKey>,
    pub class: Option<FailureClass>,
    pub detail: Option<FailureDetail>,
    /// Whether this outcome spends one of the delivery's attempts.
    ///
    /// A provider that asked us to wait does not: it refused to act, named its own deadline, and
    /// charging the delivery for having been rate-limited is how a busy hour exhausts a retry
    /// budget that was meant for real failures.
    pub consumes_attempt: bool,
    /// The provider's own deadline, when it named one. Overrides the computed backoff, because a
    /// `Retry-After` is an instruction rather than a hint.
    pub retry_after: Option<Duration>,
}

impl PartTransition {
    pub fn of(outcome: &ProviderSendOutcome) -> Self {
        match outcome {
            ProviderSendOutcome::Delivered { provider_key } => Self {
                status: DeliveryPartStatus::Delivered,
                provider_key: provider_key.clone(),
                class: None,
                detail: None,
                consumes_attempt: false,
                retry_after: None,
            },
            ProviderSendOutcome::RetryAfter {
                retry_after,
                class,
                detail,
            } => Self {
                status: DeliveryPartStatus::Retryable,
                provider_key: None,
                class: Some(*class),
                detail: Some(detail.clone()),
                consumes_attempt: false,
                retry_after: Some(*retry_after),
            },
            ProviderSendOutcome::Retryable { class, detail } => Self {
                status: DeliveryPartStatus::Retryable,
                provider_key: None,
                class: Some(*class),
                detail: Some(detail.clone()),
                consumes_attempt: true,
                retry_after: None,
            },
            // Never retryable, and never terminal either: the provider may hold this part, so it
            // waits for a reconciler rather than being re-sent or written off.
            ProviderSendOutcome::OutcomeUnknown { class, detail } => Self {
                status: DeliveryPartStatus::OutcomeUnknown,
                provider_key: None,
                class: Some(*class),
                detail: Some(detail.clone()),
                consumes_attempt: true,
                retry_after: None,
            },
            ProviderSendOutcome::Terminal { class, detail } => Self {
                status: DeliveryPartStatus::Dead,
                provider_key: None,
                class: Some(*class),
                detail: Some(detail.clone()),
                consumes_attempt: true,
                retry_after: None,
            },
        }
    }
}
