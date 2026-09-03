//! Turning "this message is owed to that recipient" into a delivery ready to commit.
//!
//! Every producer of outbound mail -- an agent reply, a schedule's answer, an approval request, an
//! outreach question, a stop notice -- needs the same four things: the interface the message goes
//! out through, a stable idempotency key, frozen parts, and a `NewDelivery` to hand to its own
//! transaction. Doing that in five places is how five slightly different keys and five slightly
//! different bounds appear, so it is done here once.
//!
//! Composing is deliberately separate from committing. This resolves and renders; the caller
//! writes the result inside the transaction that also creates the state the delivery answers for.

use std::sync::Arc;

use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        message::CanonicalMessageId,
        transport::{
            ChannelBinding, DeliveryId, DeliveryPurpose, ExternalDestination, ExternalMessageKey,
            TransportKind,
        },
    },
    transport::{
        DeliveryContext, DeliveryDestination, DeliveryEnvelope, DeliveryIntent, DeliveryKey,
        EmailDeliveryContext, MAX_DELIVERY_ATTEMPTS, NewDelivery, NewStandaloneDelivery,
        StandaloneDeliveryEnvelope,
        ingress::CanonicalContent,
        ports::{TransportRenderer, TransportRenderers},
    },
    use_cases::integration::ChannelBindingPersistence,
};

/// One delivery a producer wants queued, before its interface is resolved and its parts frozen.
///
/// `source_key` is what makes the delivery unique beyond its destination, and it names the durable
/// row that owes the delivery -- `task:<id>:reply`, `approval:<id>`, `outreach:<id>:target:2`.
/// Deliberately *not* derived from the canonical message id for task-driven work: a superseded run
/// re-executing a task mints a fresh message id, so a message-derived key would let the second run
/// queue a second copy of the same answer. The task id is what stays the same across those runs.
#[derive(Debug, Clone)]
pub struct DeliveryRequest<'a> {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    /// The canonical message this delivery exposes. Minted by the producer, which is what lets it
    /// write the message and the delivery in one transaction.
    pub message_id: CanonicalMessageId,
    pub task_id: Option<Uuid>,
    pub correlation_id: CorrelationId,
    pub purpose: DeliveryPurpose,
    pub source_key: String,
    pub content: &'a CanonicalContent,
    pub context: DeliveryContext,
}

/// One provider notification that has no canonical message, channel, or binding to attribute.
#[derive(Debug, Clone)]
pub struct StandaloneDeliveryRequest<'a> {
    pub correlation_id: CorrelationId,
    pub purpose: DeliveryPurpose,
    pub source_key: String,
    pub content: &'a CanonicalContent,
    pub context: DeliveryContext,
}

impl DeliveryRequest<'_> {
    /// Where this delivery is going, within the interface that carries it.
    ///
    /// Email names a recipient; a transport that addresses only conversations names the interface
    /// itself and the destination is the binding.
    fn destination(&self, binding: &ChannelBinding) -> DeliveryDestination {
        match &self.context {
            DeliveryContext::Email(email) => DeliveryDestination::External(
                ExternalDestination::Email(email.recipient_to.clone()),
            ),
            #[allow(unreachable_patterns)]
            _ => DeliveryDestination::Binding(binding.id),
        }
    }
}

/// One delivery, composed and ready to commit, plus what the producer needs to record about it
/// before it is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedDelivery {
    pub delivery: NewDelivery,
    /// The key the first provider message will carry, when the transport can name it in advance.
    ///
    /// `None` for a provider whose key is its own answer. Where it *is* known, a producer that has
    /// to be findable by the reply that quotes it -- an outreach's question -- records it on the
    /// canonical message in the same transaction that queues the send. Without it, a reply that
    /// arrived before the send was recorded would resolve to no conversation at all.
    pub provider_key: Option<ExternalMessageKey>,
}

/// Resolves an interface, renders parts, and hands back a delivery to commit.
#[derive(Clone)]
pub struct DeliveryComposer {
    renderers: Arc<TransportRenderers>,
    bindings: Arc<dyn ChannelBindingPersistence>,
}

impl DeliveryComposer {
    pub fn new(
        renderers: Arc<TransportRenderers>,
        bindings: Arc<dyn ChannelBindingPersistence>,
    ) -> Self {
        Self {
            renderers,
            bindings,
        }
    }

    /// The protocol boundary used to validate an explicitly external destination before any
    /// delivery exists. Callers receive the application-owned port, never the adapter type.
    pub fn renderer(&self, transport: TransportKind) -> AppResult<&dyn TransportRenderer> {
        self.renderers
            .require(transport)
            .map(|renderer| renderer.as_ref())
            .map_err(|error| AppError::Internal(error.to_string()))
    }

    /// The delivery `request` is owed, with its parts already frozen.
    ///
    /// Fails rather than silently dropping the message when the channel has no live interface on
    /// the transport the context names: a reply that cannot be addressed is a fact the caller has
    /// to see, and a producer that swallowed it would leave a thread showing an answer nobody
    /// received.
    pub async fn compose(&self, request: DeliveryRequest<'_>) -> AppResult<ComposedDelivery> {
        let transport = request.context.transport();
        let binding = self.interface(&request, transport).await?;
        let destination = request.destination(&binding);
        let key = delivery_key(request.purpose, &request.source_key, &destination);

        let id = DeliveryId::random();
        let intent = DeliveryIntent {
            message_id: request.message_id,
            // For email the interface a channel answers on is also the one it speaks through, so
            // source and destination are the same binding. They are separate columns because a
            // mirror to another interface is the case where they differ.
            source_binding_id: binding.id,
            destination: destination.clone(),
            purpose: request.purpose,
            key: key.clone(),
        };
        let envelope = DeliveryEnvelope::new(
            id,
            intent,
            transport,
            request.correlation_id,
            request.content,
            request.context,
        )
        .map_err(|error| AppError::Internal(error.to_string()))?;

        let renderer = self.renderer(transport)?;
        let parts = renderer.render(&envelope)?;
        let provider_key = parts
            .first()
            .and_then(|part| renderer.predicted_provider_key(part));

        Ok(ComposedDelivery {
            provider_key,
            delivery: NewDelivery {
                id,
                company_id: request.company_id,
                channel_id: request.channel_id,
                message_id: request.message_id,
                source_binding_id: binding.id,
                destination_binding_id: binding.id,
                external_destination: match &destination {
                    DeliveryDestination::External(external) => Some(external.clone()),
                    DeliveryDestination::Binding(_) => None,
                },
                task_id: request.task_id,
                depends_on_delivery_id: None,
                correlation_id: request.correlation_id,
                transport,
                purpose: request.purpose,
                idempotency_key: key,
                max_attempts: MAX_DELIVERY_ATTEMPTS,
                parts: NewDelivery::frozen_parts(parts)?,
            },
        })
    }

    /// Freeze an unattributed notification for the same durable queue as canonical deliveries.
    pub fn compose_standalone(
        &self,
        request: StandaloneDeliveryRequest<'_>,
    ) -> AppResult<NewStandaloneDelivery> {
        let transport = request.context.transport();
        let destination = DeliveryDestination::External(request.context.external_destination());
        let key = delivery_key(request.purpose, &request.source_key, &destination);
        let id = DeliveryId::random();
        let renderer = self.renderer(transport)?;
        let envelope = StandaloneDeliveryEnvelope::new(
            id,
            key.clone(),
            request.purpose,
            request.correlation_id,
            request.content,
            request.context,
        );
        let external_destination = match destination {
            DeliveryDestination::External(destination) => destination,
            DeliveryDestination::Binding(_) => unreachable!("constructed as external above"),
        };
        Ok(NewStandaloneDelivery {
            id,
            external_destination,
            correlation_id: request.correlation_id,
            transport,
            purpose: request.purpose,
            idempotency_key: key,
            max_attempts: MAX_DELIVERY_ATTEMPTS,
            parts: NewDelivery::frozen_parts(renderer.render_standalone(&envelope)?)?,
        })
    }

    /// The channel's live interface on one transport.
    async fn interface(
        &self,
        request: &DeliveryRequest<'_>,
        transport: TransportKind,
    ) -> AppResult<ChannelBinding> {
        self.bindings
            .active_bindings_for_channel(request.company_id, request.channel_id)
            .await?
            .into_iter()
            .find(|binding| binding.transport == transport)
            .ok_or_else(|| {
                AppError::Internal(format!(
                    "Channel {} has no active {transport} interface to deliver through",
                    request.channel_id
                ))
            })
    }
}

/// The idempotency key one logical delivery always has.
///
/// Shape: `<purpose>:<source>:<destination>`. The destination is part of it, not merely of the
/// row, because deduplication is `(destination_binding_id, key)` and an explicitly named recipient
/// has no binding of its own -- without it, two outreach recipients on one message would share a
/// key and one of them would silently never be written.
pub fn delivery_key(
    purpose: DeliveryPurpose,
    source_key: &str,
    destination: &DeliveryDestination,
) -> DeliveryKey {
    let key = format!(
        "{}:{source_key}:{}",
        purpose.as_str(),
        destination.key_fragment()
    );
    DeliveryKey::parse(key.clone()).unwrap_or_else(|_| {
        // A source key is built from identifiers this deployment minted, so overrunning the bound
        // means one of them is not what it claims. Hashing keeps the key stable and unique rather
        // than truncating two long keys into one.
        DeliveryKey::parse(format!(
            "{}:sha256:{:x}",
            purpose.as_str(),
            <sha2::Sha256 as sha2::Digest>::digest(key.as_bytes())
        ))
        .expect("a purpose and a hex digest are within the delivery key bound")
    })
}

/// A convenience for the common email context, so producers do not each spell the enum arm.
pub fn email_context(context: EmailDeliveryContext) -> DeliveryContext {
    DeliveryContext::Email(context)
}
