//! The ports the application needs from the outside world in order to move messages.
//!
//! Each one is a single cohesive job: commit an inbound message, resolve a provider key, plan a
//! fan-out, render parts, make one provider request. None of them has a defaulted method -- a test
//! double that silently succeeds at a durable write or an authorization check is a double that
//! makes the test prove nothing, which is what `src/AGENTS.md` forbids for correctness operations.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        correlation::CorrelationId,
        message::CanonicalMessageId,
        transport::{
            ChannelBindingId, ExternalDestination, ExternalMessageKey, ExternalThreadKey,
            TransportKind, UnsupportedTransport,
        },
        value_objects::{EmailAddress, MessageId},
    },
    transport::{
        delivery::{
            DeliveryEnvelope, DeliveryIntent, DeliveryPlanRequest, ProviderSendOutcome,
            RenderedPart, StandaloneDeliveryEnvelope,
        },
        ingress::{InboundCommitOutcome, InboundCommitRequest},
        queue::DeliveryRecord,
    },
};

/// The one transaction an accepted inbound message goes through.
///
/// Deliberately a single method taking a single request. The rows it writes -- identities, thread
/// mappings, the canonical message, its associations, the provider mappings, the task, the
/// delivery intents, and the completion of any claimed inbound event -- become visible together or
/// not at all. Splitting them across several port calls is how a message ends up stored without
/// the mapping that would deduplicate its redelivery.
#[async_trait]
pub trait InboundMessageCommitter: Send + Sync {
    async fn commit_inbound(
        &self,
        request: InboundCommitRequest,
    ) -> AppResult<InboundCommitOutcome>;
}

/// Read-only resolution of provider keys, for the decisions that must happen before the commit.
///
/// Everything correctness-critical belongs inside [`InboundMessageCommitter::commit_inbound`].
/// This exists for the reads a policy phase needs in order to *decide* -- "is this a reply to a
/// conversation we already know?" -- where a stale answer costs an extra thread rather than a lost
/// or duplicated message.
#[async_trait]
pub trait ExternalCorrelationStore: Send + Sync {
    /// The internal thread one of these provider conversation keys maps to, within one binding.
    ///
    /// Ordered, and a list rather than a single key, because a transport may offer more than one
    /// candidate root for the same conversation and the nearest one wins.
    async fn thread_for_thread_keys(
        &self,
        binding_id: ChannelBindingId,
        thread_keys: &[ExternalThreadKey],
    ) -> AppResult<Option<Uuid>>;

    /// The internal thread reached through any of these provider message keys, nearest candidate
    /// first. Ordered because a reply names its parent before it names the conversation root.
    async fn thread_for_message_keys(
        &self,
        binding_id: ChannelBindingId,
        message_keys: &[ExternalMessageKey],
    ) -> AppResult<Option<Uuid>>;

    /// The canonical message a provider key already maps to, which is how a redelivery is
    /// recognised before any work is repeated.
    async fn message_for_external_key(
        &self,
        binding_id: ChannelBindingId,
        message_key: &ExternalMessageKey,
    ) -> AppResult<Option<CanonicalMessageId>>;
}

/// Pure binding and policy fan-out: which destinations one message is owed.
///
/// Synchronous and free of I/O by design. Everything it needs -- the candidate bindings and
/// whether their installations are usable -- is passed in, so the rule can be unit-tested against
/// a table of cases with no database and no mocks.
pub trait DeliveryPlanner: Send + Sync {
    fn plan(&self, request: &DeliveryPlanRequest<'_>) -> Vec<DeliveryIntent>;
}

/// The application's own planner: [`crate::transport::plan_deliveries`] behind the port.
#[derive(Debug, Clone, Copy, Default)]
pub struct PolicyDeliveryPlanner;

impl DeliveryPlanner for PolicyDeliveryPlanner {
    fn plan(&self, request: &DeliveryPlanRequest<'_>) -> Vec<DeliveryIntent> {
        super::delivery::plan_deliveries(request)
    }
}

/// Turns a resolved delivery into the bounded parts that will actually be sent.
///
/// Deterministic and synchronous: the same envelope renders the same parts with the same keys, so
/// a resumed delivery addresses the parts it already froze instead of creating new ones. A
/// renderer that needed to await something would be making a decision that belongs upstream of the
/// freeze.
pub trait TransportRenderer: Send + Sync {
    fn transport(&self) -> TransportKind;

    /// Parse one explicitly external destination using this transport's own syntax.
    ///
    /// The application needs this before it can authorize an outreach target, but must not import
    /// a protocol parser or its framework types to do it. Keeping the classification on the
    /// transport boundary also lets another renderer define a Slack user/channel destination
    /// without projecting it through an email address.
    fn classify_external_destination(&self, value: &str) -> ExternalDestinationClassification;

    fn render(&self, envelope: &DeliveryEnvelope) -> AppResult<Vec<RenderedPart>>;

    /// Freeze a notification that has no canonical message or channel attribution.
    ///
    /// This is not a second delivery protocol: the returned parts enter the same queue and worker.
    /// It exists because inventing tenant identifiers for a bounce to an unknown company would be
    /// worse than admitting that the notification is intentionally unattributed.
    fn render_standalone(
        &self,
        envelope: &StandaloneDeliveryEnvelope,
    ) -> AppResult<Vec<RenderedPart>>;

    /// The key this part will go out under, when the transport's own key is derivable before the
    /// provider is called.
    ///
    /// Mail can: an RFC `Message-ID` is chosen by the sender, and deriving it from the part key
    /// makes it stable across every attempt. A chat provider cannot -- the timestamp is the
    /// provider's answer, not the caller's -- and says so with `None`.
    ///
    /// It matters because a producer sometimes has to record *what it sent* in the same
    /// transaction that queues the send: an outreach's question has to be findable by the reply
    /// that quotes it, and a reply that arrives before the send was recorded would resolve to no
    /// conversation at all.
    ///
    /// Not defaulted: a renderer that quietly answered `None` would turn that into a thread
    /// nobody can find, one delayed reply at a time.
    fn predicted_provider_key(&self, part: &RenderedPart) -> Option<ExternalMessageKey>;
}

/// What a transport learned while parsing a requested external destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalDestinationClassification {
    External(ExternalDestination),
    /// A valid address owned by this deployment. The caller must use an internal channel selector
    /// so the channel's authorization policy is applied.
    InternalEndpoint,
    /// The value is in the deployment's namespace but is not a valid internal endpoint.
    InvalidInternalEndpoint,
    /// Not valid destination syntax for this transport.
    InvalidSyntax,
}

/// One external request, and what the provider said about it.
///
/// The return type is [`ProviderSendOutcome`] rather than `AppResult` on purpose: see that type
/// for why an `Err` at this seam destroys the distinction a delivery queue exists to preserve.
///
/// It is given the durable record and the frozen part, not the [`DeliveryEnvelope`] the renderer
/// worked from: that envelope carries the render context, which is not persisted and so does not
/// exist by the time a claimed row is sent. Everything a provider call needs is in the part it is
/// sending.
#[async_trait]
pub trait TransportSender: Send + Sync {
    fn transport(&self) -> TransportKind;

    async fn send(&self, delivery: &DeliveryRecord, part: &RenderedPart) -> ProviderSendOutcome;
}

/// Delivering to a mailbox this deployment itself owns.
///
/// Inter-channel delegation is not SMTP. When one channel's agent addresses another channel of the
/// same company, the message is ingested in process under the relay's own identity -- there is no
/// DMARC verdict to earn, because nothing left the building -- and rendering it to RFC 5322 only
/// to parse it back was how a mailbox became the internal identity of a channel.
///
/// A port because the *email sender* is what discovers that a recipient is one of ours, and the
/// ingest it then needs belongs to the application. Implemented by the thread use cases.
#[async_trait]
pub trait InternalMailRelay: Send + Sync {
    /// Ingest `mail` as an inbound message on the recipient channel, or report that the recipient
    /// is not one of this deployment's channels.
    ///
    /// `Ok(RelayDisposition::NotInternal)` is the ordinary case and means "send it yourself"; it
    /// is a distinct value rather than an error because a stranger's address is not a fault.
    async fn relay_internal(&self, mail: &InternalRelayMail<'_>) -> AppResult<RelayDisposition>;
}

/// One mail offered to the internal relay.
///
/// A named struct because it carries four `EmailAddress`/`MessageId` values and three loop-control
/// fields; positional parameters here are the argument-swap `src/AGENTS.md` warns about.
#[derive(Debug, Clone)]
pub struct InternalRelayMail<'a> {
    pub from: &'a EmailAddress,
    pub recipient_to: &'a EmailAddress,
    pub subject: &'a str,
    pub body_text: &'a str,
    pub message_id: &'a MessageId,
    pub in_reply_to: Option<&'a MessageId>,
    pub references: &'a [MessageId],
    /// The channel this mail goes out as. Checked against the stated sender address, so a relayed
    /// message cannot claim to come from a channel it does not.
    pub source_channel_id: Uuid,
    /// Hops this message *is*, not hops it answers: the same value the wire header carries, so an
    /// internal hop costs exactly what an external one costs.
    pub hop_count: u32,
    pub trace: Vec<Uuid>,
    pub correlation_id: CorrelationId,
}

/// What the internal relay did with a mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayDisposition {
    /// Ingested on the recipient channel. Nothing should be sent.
    Relayed,
    /// The recipient is outside this deployment. Hand it to the provider.
    NotInternal,
    /// The recipient is one of ours and the message was refused -- a disabled channel, a hop
    /// budget spent, an ACL. Carries the reason so the delivery records why rather than retrying
    /// into the same refusal.
    Refused(String),
}

/// The renderer and sender that speak one transport.
#[derive(Clone)]
pub struct RegisteredTransport {
    renderer: Arc<dyn TransportRenderer>,
    sender: Arc<dyn TransportSender>,
}

impl RegisteredTransport {
    pub fn renderer(&self) -> &Arc<dyn TransportRenderer> {
        &self.renderer
    }

    pub fn sender(&self) -> &Arc<dyn TransportSender> {
        &self.sender
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportRegistrationError {
    #[error("a {renderer} renderer cannot be registered with a {sender} sender")]
    Mismatched {
        renderer: TransportKind,
        sender: TransportKind,
    },
    #[error("{transport} is already registered")]
    Duplicate { transport: TransportKind },
}

/// The renderers this deployment can freeze parts with, keyed by transport.
///
/// A separate view from [`TransportRegistry`] because rendering and sending have different
/// consumers at different times: a *producer* freezes parts inside the transaction that creates
/// the state the delivery answers for, while the delivery *worker* is the only thing that sends.
/// Splitting them is also what keeps the wiring acyclic -- the email sender needs the internal
/// relay, which is a use case that itself renders -- so the renderers are built first and both
/// consumers share the same instances.
#[derive(Clone, Default)]
pub struct TransportRenderers {
    renderers: HashMap<TransportKind, Arc<dyn TransportRenderer>>,
}

impl TransportRenderers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one renderer, refusing a second for the same transport: a silent replacement is how a
    /// deployment ends up freezing parts with the adapter nobody configured.
    pub fn register(
        mut self,
        renderer: Arc<dyn TransportRenderer>,
    ) -> Result<Self, TransportRegistrationError> {
        let transport = renderer.transport();
        if self.renderers.contains_key(&transport) {
            return Err(TransportRegistrationError::Duplicate { transport });
        }
        self.renderers.insert(transport, renderer);
        Ok(self)
    }

    pub fn get(&self, transport: TransportKind) -> Option<&Arc<dyn TransportRenderer>> {
        self.renderers.get(&transport)
    }

    /// The renderer for `transport`, or the error a producer should refuse to enqueue with.
    ///
    /// A missing adapter is a configuration fact about this deployment, so it is reported as the
    /// unsupported transport it is rather than as an internal fault.
    pub fn require(
        &self,
        transport: TransportKind,
    ) -> Result<&Arc<dyn TransportRenderer>, UnsupportedTransport> {
        self.get(transport)
            .ok_or_else(|| UnsupportedTransport::new(transport.as_str()))
    }

    pub fn registered(&self) -> impl Iterator<Item = TransportKind> + '_ {
        self.renderers.keys().copied()
    }
}

/// Which transports this deployment can actually speak, keyed by kind.
///
/// Lives in the application/service layer rather than under `adapters` because the delivery worker
/// is what consults it; the adapters are what register themselves into it. The previous shape --
/// an `EgressRegistry` defined inside `src/adapters/protocols` and held by an application use case
/// -- was an abstraction living inside the layer it abstracts.
#[derive(Clone, Default)]
pub struct TransportRegistry {
    transports: HashMap<TransportKind, RegisteredTransport>,
}

impl TransportRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one transport's pair.
    ///
    /// Fallible rather than last-write-wins: a renderer and sender that disagree about which
    /// transport they speak is a wiring bug that would otherwise surface as a decode failure
    /// mid-delivery, and a second registration for the same kind silently replacing the first is
    /// how a deployment ends up sending through the adapter nobody configured.
    pub fn register(
        mut self,
        renderer: Arc<dyn TransportRenderer>,
        sender: Arc<dyn TransportSender>,
    ) -> Result<Self, TransportRegistrationError> {
        let transport = renderer.transport();
        if transport != sender.transport() {
            return Err(TransportRegistrationError::Mismatched {
                renderer: transport,
                sender: sender.transport(),
            });
        }
        if self.transports.contains_key(&transport) {
            return Err(TransportRegistrationError::Duplicate { transport });
        }
        self.transports
            .insert(transport, RegisteredTransport { renderer, sender });
        Ok(self)
    }

    pub fn get(&self, transport: TransportKind) -> Option<&RegisteredTransport> {
        self.transports.get(&transport)
    }

    /// The registered pair, or the error a worker should record for a delivery it cannot send.
    ///
    /// A missing adapter is a configuration fact about this deployment, not an internal fault, so
    /// it is reported as the unsupported transport it is.
    pub fn require(
        &self,
        transport: TransportKind,
    ) -> Result<&RegisteredTransport, UnsupportedTransport> {
        self.get(transport)
            .ok_or_else(|| UnsupportedTransport::new(transport.as_str()))
    }

    pub fn registered(&self) -> impl Iterator<Item = TransportKind> + '_ {
        self.transports.keys().copied()
    }
}
