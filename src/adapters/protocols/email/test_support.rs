//! Mail transports the email tests share.
//!
//! Hoisted here rather than re-declared per file: `egress_tests.rs` proves what the renderer
//! freezes, and the round-trip tests in `use_cases::thread` need the same double to read the mail
//! a real send actually posted.

use std::sync::Mutex;

use async_trait::async_trait;

use super::{MailMessage, MailSendOutcome, MailTransport};

/// Accepts every mail and keeps it, so a test can assert on the envelope that went out.
#[derive(Default)]
pub struct RecordingTransport {
    sent: Mutex<Vec<MailMessage>>,
}

impl RecordingTransport {
    /// Everything posted so far, in order.
    pub fn sent(&self) -> Vec<MailMessage> {
        self.sent.lock().unwrap().clone()
    }
}

#[async_trait]
impl MailTransport for RecordingTransport {
    async fn send(&self, message: MailMessage) -> MailSendOutcome {
        self.sent.lock().unwrap().push(message);
        MailSendOutcome::Accepted { provider_key: None }
    }
}

/// A transport whose relay always fails without a verdict, for the ambiguity rules.
pub struct RefusingTransport;

#[async_trait]
impl MailTransport for RefusingTransport {
    async fn send(&self, _message: MailMessage) -> MailSendOutcome {
        MailSendOutcome::Unknown {
            class: crate::entities::transport::FailureClass::Network,
            detail: "the relay closed the connection".to_string(),
        }
    }
}

/// A relayed channel mail frozen as a delivery's only part, for tests that need the relay proof a
/// queued hop carries without rendering a whole delivery. It is built here because only the email
/// adapter decides what goes on an envelope.
pub fn relayed_part(
    delivery_key: &str,
    from: &str,
    to: &str,
    message_id: &str,
    body: &str,
    source_channel_id: uuid::Uuid,
) -> crate::transport::RenderedPart {
    use super::egress::OutboundRelayV1;
    use super::{OUTBOUND_EMAIL_VERSION, OutboundEmailV1};
    use crate::entities::correlation::CorrelationId;
    use crate::entities::transport::TransportKind;
    use crate::entities::value_objects::{EmailAddress, MessageId};
    use crate::transport::{ContentDigest, PartIndex, PartKey, RenderedPart, TransportPayload};
    let email = OutboundEmailV1 {
        from: EmailAddress::from(from),
        from_name: None,
        recipients_to: vec![EmailAddress::from(to)],
        recipients_cc: Vec::new(),
        subject: "Re: Question".into(),
        body_text: body.into(),
        message_id: MessageId::from(message_id),
        in_reply_to: None,
        references: Vec::new(),
        internal_target: None,
        correlation_id: CorrelationId::new(),
        relay: Some(OutboundRelayV1 {
            source_channel_id,
            hop_count: 0,
            trace_channels: Vec::new(),
        }),
    };
    RenderedPart {
        index: PartIndex::new(0),
        key: PartKey::parse(format!("email:{delivery_key}")).unwrap(),
        payload: TransportPayload::encode(TransportKind::Email, OUTBOUND_EMAIL_VERSION, &email)
            .unwrap(),
        digest: ContentDigest::sha256_of(body.as_bytes()),
    }
}
