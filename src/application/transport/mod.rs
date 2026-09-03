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
pub mod delivery;
pub mod ingress;
pub mod lease;
pub mod ports;
pub mod task_payload;

pub use bounded::{BoundedVec, BoundsError};
pub use delivery::{
    ContentDigest, DELIVERY_ENVELOPE_VERSION, DeliveryCandidate, DeliveryDestination,
    DeliveryEnvelope, DeliveryIntent, DeliveryKey, DeliveryPlanRequest, DeliveryPurpose,
    FailureClass, FailureDetail, MAX_DELIVERY_PARTS, MAX_PART_PAYLOAD_BYTES, PartIndex, PartKey,
    PayloadError, ProviderSendOutcome, RenderedPart, TransportPayload, plan_deliveries,
};
pub use ingress::{
    AddressedIdentity, CanonicalContent, CommitDisposition, EmailIngressFacts,
    InboundCommitOutcome, InboundCommitRequest, InboundEnvelope, InboundTaskRequest,
    IngressDirectives, IngressPolicyFacts, MAX_ADDRESSED_IDENTITIES, MAX_ATTACHMENTS,
    MAX_BODY_BYTES, MAX_REPLY_CANDIDATES, MAX_SUBJECT_BYTES, MAX_THREAD_ASSOCIATIONS,
    MessageDisposition, NewThread, PipelineStep, ProtocolExtension, RecipientRole, ReplyCandidates,
    ThreadAssociation, ThreadTarget,
};
pub use lease::{ExecutionId, ExecutionLease, WorkerId};
pub use ports::{
    DeliveryPlanner, ExternalCorrelationStore, InboundMessageCommitter, PolicyDeliveryPlanner,
    RegisteredTransport, TransportRegistrationError, TransportRegistry, TransportRenderer,
    TransportSender,
};
pub use task_payload::{InboundTaskPayload, InboundTaskPayloadV1};

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "dependency_tests.rs"]
mod dependency_tests;
