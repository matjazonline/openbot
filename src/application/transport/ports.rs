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
        message::CanonicalMessageId,
        transport::{
            ChannelBindingId, ExternalMessageKey, ExternalThreadKey, TransportKind,
            UnsupportedTransport,
        },
    },
    transport::{
        delivery::{
            DeliveryEnvelope, DeliveryIntent, DeliveryPlanRequest, ProviderSendOutcome,
            RenderedPart,
        },
        ingress::{InboundCommitOutcome, InboundCommitRequest},
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

    fn render(&self, envelope: &DeliveryEnvelope) -> AppResult<Vec<RenderedPart>>;
}

/// One external request, and what the provider said about it.
///
/// The return type is [`ProviderSendOutcome`] rather than `AppResult` on purpose: see that type
/// for why an `Err` at this seam destroys the distinction a delivery queue exists to preserve.
#[async_trait]
pub trait TransportSender: Send + Sync {
    fn transport(&self) -> TransportKind;

    async fn send(&self, envelope: &DeliveryEnvelope, part: &RenderedPart) -> ProviderSendOutcome;
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
