//! Protocol-neutral transport contracts, defined in the layer that consumes them.
//!
//! A business channel can speak more than one protocol. What arrives is an [`InboundEnvelope`];
//! what goes out is a [`DeliveryIntent`] that a [`TransportRenderer`] freezes into parts and a
//! [`TransportSender`] hands to one provider. Adding a transport means implementing these ports
//! and registering the pair -- not editing the canonical message, the ingest use case, or this
//! module.
//!
//! Everything here is owned by the application because the application is what consumes it. The
//! shape this replaces had `ProtocolEgressAdapter` and `EgressRegistry` declared inside
//! `src/adapters/protocols`, which put an abstraction inside the layer it exists to abstract; the
//! dependency-direction test in `dependency_tests.rs` is what stops that returning.

pub mod bounded;
pub mod compose;
pub mod delivery;
pub mod inbox;
pub mod ingress;
pub mod lease;
pub mod ports;
pub mod queue;
pub mod task_payload;

/// Addressing role and the delivery state machine are domain transport vocabulary; re-exported
/// here so the transport module reads as one place.
pub use crate::entities::transport::{
    DeliveryPartStatus, DeliveryPurpose, DeliveryStatus, FailureClass, RecipientRole,
    UnknownRecipientRole, aggregate_parent_status,
};
pub use bounded::{BoundedVec, BoundsError};
pub use compose::{
    ComposedDelivery, DeliveryComposer, DeliveryRequest, StandaloneDeliveryRequest, delivery_key,
    email_context,
};
pub use delivery::{
    CanonicalContentV1, ContentDigest, ContextMismatch, DELIVERY_ENVELOPE_VERSION,
    DeliveryCandidate, DeliveryContext, DeliveryDestination, DeliveryEnvelope, DeliveryIntent,
    DeliveryKey, DeliveryPlanRequest, EmailDeliveryContext, EmailRelayTrace, FailureDetail,
    MAX_DELIVERY_PARTS, MAX_PART_PAYLOAD_BYTES, PartIndex, PartKey, PartTransition, PayloadError,
    ProviderSendOutcome, RenderedPart, StandaloneDeliveryEnvelope, TransportPayload,
    plan_deliveries,
};
pub use inbox::{
    AuthenticatedInboundEvent, ClaimedInboundEvent, DuplicateInboundDecoder,
    INBOUND_EVENT_CLAIM_BATCH, INBOUND_EVENT_LEASE_SECONDS, InboundContentType, InboundDigestError,
    InboundEventCensus, InboundEventDecodeOutcome, InboundEventDecoder,
    InboundEventDecoderRegistry, InboundEventFailure, InboundEventInbox, InboundEventPayload,
    InboundEventQueue, InboundEventReaping, InboundEventRecord, InboundEventRetention,
    InboundEventStoreOutcome, InboundEventTransition, InboundFailureDetail, InboundPayloadDigest,
    InboundPayloadError, InboundRetentionPolicy, MAX_INBOUND_EVENT_ATTEMPTS,
    MAX_INBOUND_EVENT_PAYLOAD_BYTES, MonitoredInboundEventInbox, SafeHeaderFacts,
    SafeHeaderFactsError,
};
pub use ingress::{
    AddressedIdentity, AddressedRecipient, AddressedTarget, CanonicalContent, CommitDisposition,
    EmailIngressFacts, InboundCommitOutcome, InboundCommitRequest, InboundDraft, InboundEnvelope,
    InboundOutreachTransition, InboundRouting, InboundTaskRequest, InboundTaskTarget,
    IngressDirectives, IngressPolicyFacts, MAX_ADDRESSED_IDENTITIES, MAX_ADDRESSED_TARGETS,
    MAX_ATTACHMENTS, MAX_BODY_BYTES, MAX_INGRESS_HOPS, MAX_REPLY_CANDIDATES, MAX_SUBJECT_BYTES,
    MAX_THREAD_ASSOCIATIONS, MAX_THREAD_PRINCIPALS, MAX_TRACE_CHANNELS, MessageDisposition,
    PipelineStep, ProtocolExtension, ReplyCandidates, ReplyDelivery, SMALL_INLINE_IMAGE_BYTES,
    SystemAddress, ThreadAssociation, ThreadPrincipalIntent, ThreadTarget,
};
pub use lease::{ExecutionId, ExecutionLease, WorkerId};
pub use ports::{
    DeliveryPlanner, ExternalCorrelationStore, ExternalDestinationClassification,
    InboundMessageCommitter, InternalMailRelay, InternalRelayMail, PolicyDeliveryPlanner,
    RegisteredTransport, RelayDisposition, TransportRegistrationError, TransportRegistry,
    TransportRenderer, TransportRenderers, TransportSender,
};
pub use queue::{
    ClaimedDelivery, DELIVERY_CLAIM_BATCH, DELIVERY_LEASE_SECONDS, DeliveryAttribution,
    DeliveryBackoff, DeliveryCreation, DeliveryFailure, DeliveryOutcome, DeliveryQueue,
    DeliveryReaping, DeliveryRecord, Disposition, MAX_DELIVERY_ATTEMPTS, NewDelivery,
    NewStandaloneDelivery, PartResult, StandaloneDeliveryEnqueuer, StoredPart,
};
pub use task_payload::{InboundTaskPayload, InboundTaskPayloadV1};

#[cfg(test)]
pub mod test_support;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "dependency_tests.rs"]
mod dependency_tests;
