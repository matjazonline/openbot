//! What a durable background task carries about the message that created it.
//!
//! Stable identifiers plus four small delivery directives. The previous payload serialized whole
//! `Company`, `Channel`, `Thread`, `Message`, parsed-email and normalized-protocol values into
//! `background_tasks.payload`, which made every queued row a snapshot of the domain model at the
//! moment it was written: a field rename broke rows already in flight, stale configuration was
//! replayed hours later, and raw provider content sat inside the task protocol.
//!
//! The worker reloads the current entities with tenant-scoped queries instead. Versioning stays,
//! so a future shape change is a deliberate decision rather than a silent reinterpretation -- but
//! there is no decoder for the pre-reset payload, by design: the database is reset before this
//! ships, so a broad payload is a bug rather than a legacy row.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{correlation::CorrelationId, message::CanonicalMessageId},
    transport::{
        bounded::BoundedVec,
        ingress::{MAX_TRACE_CHANNELS, ReplyDelivery},
    },
};

/// The canonical, transport-neutral identifiers one agent-dispatch task needs to reload its world,
/// plus the few delivery facts no row holds.
///
/// The line between the two is deliberate. Anything the commit wrote -- the body, the author, the
/// recipients, the headers, the threads it landed in -- is *reloaded*, so a task can never replay a
/// stale copy of it. What is copied here is only what was true of the delivery rather than of the
/// message: how many relay hops it had taken, which channels it had already passed through, and
/// whether the answer was asked to stay in the app. None of those is recoverable from a stored row,
/// and guessing any of them breaks loop protection or sends mail a user asked not to send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundTaskPayloadV1 {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    /// The canonical message this run answers. The message's own thread associations are what the
    /// worker walks to find every channel the message reached -- they are stored, so they do not
    /// need to be copied here and cannot drift from what was actually written.
    pub source_message_id: CanonicalMessageId,
    /// The chain this task belongs to, inherited from the message rather than minted here.
    pub correlation_id: CorrelationId,
    /// How many times this message had already been relayed between channels.
    pub hop_count: u32,
    /// The channels it had already passed through, so a reply cannot cycle back into one.
    pub trace_channels: BoundedVec<Uuid, MAX_TRACE_CHANNELS>,
    /// Whether the body is a forwarded conversation, which decides whether the sender's trust
    /// extends to the words inside it.
    pub is_forwarded: bool,
    pub reply_delivery: ReplyDelivery,
}

/// The stored payload, tagged by the version that wrote it.
///
/// Same shape as [`crate::entities::message::MessageAttachments`]: the tag is structural, so a
/// payload from a writer this process does not know fails to decode instead of being read as V1
/// with missing fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum InboundTaskPayload {
    #[serde(rename = "1")]
    V1(InboundTaskPayloadV1),
}

impl InboundTaskPayload {
    pub const fn v1(payload: InboundTaskPayloadV1) -> Self {
        Self::V1(payload)
    }

    pub const fn identifiers(&self) -> &InboundTaskPayloadV1 {
        match self {
            Self::V1(payload) => payload,
        }
    }

    pub fn encode(&self) -> AppResult<serde_json::Value> {
        serde_json::to_value(self).map_err(|error| {
            AppError::Internal(format!("Could not encode the task payload: {error}"))
        })
    }

    /// Reads a stored payload.
    ///
    /// A payload that will not decode will not decode on the next attempt either, so the caller
    /// must treat this as terminal rather than retryable -- which is only possible because the
    /// failure is reported instead of being papered over with defaults.
    pub fn decode(value: &serde_json::Value) -> AppResult<Self> {
        serde_json::from_value(value.clone())
            .map_err(|error| AppError::BadRequest(format!("Unreadable task payload: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> InboundTaskPayload {
        InboundTaskPayload::v1(InboundTaskPayloadV1 {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            source_message_id: CanonicalMessageId::random(),
            correlation_id: CorrelationId::new(),
            hop_count: 0,
            trace_channels: BoundedVec::empty(),
            is_forwarded: false,
            reply_delivery: ReplyDelivery::Send,
        })
    }

    #[test]
    fn a_payload_round_trips_through_its_stored_form() {
        let original = payload();
        let encoded = original.encode().unwrap();
        assert_eq!(encoded["version"], "1");
        assert_eq!(InboundTaskPayload::decode(&encoded).unwrap(), original);
    }

    /// The point of the tag: a row written by a newer deployment is an error a worker reports,
    /// not a V1 payload with silently defaulted identifiers.
    #[test]
    fn an_unknown_version_is_refused_rather_than_read_as_v1() {
        let mut encoded = payload().encode().unwrap();
        encoded["version"] = serde_json::Value::String("2".into());
        assert!(InboundTaskPayload::decode(&encoded).is_err());
    }

    /// The regression this type exists for: the old payload carried whole entities and provider
    /// content, so a task row was a copy of the domain model. Anything that shape must not decode.
    #[test]
    fn a_broad_pre_reset_payload_has_no_decoder() {
        let broad = serde_json::json!({
            "accepted": true,
            "company": { "id": Uuid::new_v4(), "slug": "acme" },
            "parsed_email": { "message_id": "<a@example.com>", "clean_text_body": "secret" },
        });
        assert!(InboundTaskPayload::decode(&broad).is_err());
    }

    #[test]
    fn decoding_never_panics_on_an_over_limit_or_malformed_value() {
        let oversized = serde_json::json!({ "version": "1", "company_id": "x".repeat(10_000) });
        assert!(InboundTaskPayload::decode(&oversized).is_err());
        assert!(InboundTaskPayload::decode(&serde_json::Value::Null).is_err());
    }

    #[test]
    fn a_trace_at_the_hop_limit_decodes_and_one_past_it_is_refused() {
        let mut encoded = payload().encode().unwrap();
        encoded["trace_channels"] = serde_json::to_value(
            (0..MAX_TRACE_CHANNELS)
                .map(|_| Uuid::new_v4())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(InboundTaskPayload::decode(&encoded).is_ok());

        encoded["trace_channels"] = serde_json::to_value(
            (0..=MAX_TRACE_CHANNELS)
                .map(|_| Uuid::new_v4())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(InboundTaskPayload::decode(&encoded).is_err());
    }
}
