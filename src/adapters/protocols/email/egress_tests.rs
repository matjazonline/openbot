//! What the email renderer freezes, and what the sender does with it.
//!
//! The renderer is the whole reason a retry cannot drift: everything about the mail is decided
//! here, from one envelope, with no I/O. So these tests pin the decisions — the `Re:` prefix, the
//! `Cc` filtering, the hop header, and above all the `Message-ID`, which the queue's idempotency
//! depends on being a pure function of the delivery's key.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::*;
use crate::{
    entities::{
        message::CanonicalMessageId,
        transport::{ChannelBindingId, DeliveryPurpose},
    },
    transport::{
        CanonicalContent, DeliveryComposer, DeliveryDestination, DeliveryIntent, DeliveryKey,
        DeliveryRecord, MAX_DELIVERY_ATTEMPTS,
    },
};

const DOMAIN: &str = "mailagents.test";

fn renderer() -> EmailRenderer {
    EmailRenderer::new(DOMAIN)
}

/// A delivery ready to render: a reply from `support`, quoting one inbound mail.
fn envelope(context: EmailDeliveryContext) -> DeliveryEnvelope {
    let content = CanonicalContent::parse("Order 91", "On its way.").expect("bounded content");
    let destination = DeliveryDestination::External(
        crate::entities::transport::ExternalDestination::Email(context.recipient_to.clone()),
    );
    let message_id = CanonicalMessageId::random();
    let binding = ChannelBindingId::random();
    DeliveryEnvelope::new(
        crate::entities::transport::DeliveryId::random(),
        DeliveryIntent {
            message_id,
            source_binding_id: binding,
            destination: destination.clone(),
            purpose: DeliveryPurpose::Reply,
            key: DeliveryKey::parse("reply:task:abc:email:customer@example.com")
                .expect("a short key"),
        },
        TransportKind::Email,
        CorrelationId::new(),
        &content,
        crate::transport::DeliveryContext::Email(context),
    )
    .expect("an email context matches an email transport")
}

fn context(recipient: &str) -> EmailDeliveryContext {
    EmailDeliveryContext {
        from: EmailAddress::from(format!("support@acme.{DOMAIN}")),
        from_name: Some("Support".to_string()),
        recipient_to: EmailAddress::from(recipient),
        recipients_cc: Vec::new(),
        in_reply_to: Some(MessageId::from("<inbound@example.com>")),
        references: vec![MessageId::from("<root@example.com>")],
        relay: Some(crate::transport::EmailRelayTrace {
            source_channel_id: Uuid::new_v4(),
            hop_count: 1,
            trace_channels: Vec::new(),
        }),
    }
}

/// The frozen mail one render produced.
fn frozen(parts: &[RenderedPart]) -> OutboundEmailV1 {
    parts[0]
        .payload
        .decode(TransportKind::Email, OUTBOUND_EMAIL_VERSION)
        .expect("the renderer wrote this payload")
}

/// The queue-then-deliver split rests on this: whoever queues a delivery derives the outbound
/// `Message-ID` from the part key and persists it *before* the worker sends, so both sides must
/// arrive at the same value from the same key. If this drifts, the message recorded in the thread
/// and the mail actually delivered stop matching, and replies stop threading.
#[test]
fn the_message_id_is_a_pure_function_of_the_delivery_key() {
    let renderer = renderer();
    let first = renderer
        .render(&envelope(context("customer@example.com")))
        .unwrap();
    let second = renderer
        .render(&envelope(context("customer@example.com")))
        .unwrap();

    assert_eq!(frozen(&first).message_id, frozen(&second).message_id);
    assert_eq!(first[0].key, second[0].key, "the part key is stable too");
    assert!(
        frozen(&first)
            .message_id
            .as_str()
            .ends_with(&format!("@{DOMAIN}>")),
        "the Message-ID is qualified by this deployment's domain"
    );

    // A different key is a different mail, or two logical deliveries would collide onto one
    // Message-ID and a recipient's client would thread them as one.
    let other = renderer.message_id_for(
        &EmailRenderer::part_key("reply:task:xyz:email:customer@example.com").unwrap(),
    );
    assert_ne!(frozen(&first).message_id, other);
}

/// Email freezes exactly one part. Not a limitation to be lifted: a mail has no length bound that
/// would justify splitting one answer, and doing so would break threading for every client.
#[test]
fn one_delivery_freezes_exactly_one_mail() {
    let parts = renderer()
        .render(&envelope(context("customer@example.com")))
        .unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].index.get(), 0);
}

/// `Re:` exactly once, however the subject arrived, and the parent appended to `References:` when
/// it is not already there.
#[test]
fn the_reply_subject_and_reference_chain_are_normalized() {
    let rendered = frozen(
        &renderer()
            .render(&envelope(context("c@example.com")))
            .unwrap(),
    );
    assert_eq!(rendered.subject, "Re: Order 91");
    assert_eq!(
        rendered.references,
        vec![
            MessageId::from("<root@example.com>"),
            MessageId::from("<inbound@example.com>"),
        ],
        "the parent joins the chain it answers"
    );

    assert_eq!(reply_subject("Re: Order 91"), "Re: Order 91");
    assert_eq!(reply_subject("RE: Order 91"), "RE: Order 91");
    assert_eq!(reply_subject("   "), "Re:");
}

/// The `Cc` line loses the recipient, the sender, and every address inside this deployment.
///
/// A platform address on a `Cc` line is an inbound message waiting to happen: the pipeline that
/// wanted one has its own delivery, and copying it here would deliver the answer twice.
#[test]
fn copied_addresses_drop_duplicates_and_this_deployment_s_own_mailboxes() {
    let rendered = frozen(
        &renderer()
            .render(&envelope(EmailDeliveryContext {
                recipients_cc: vec![
                    EmailAddress::from("Customer@Example.com"),
                    EmailAddress::from(format!("support@acme.{DOMAIN}")),
                    EmailAddress::from(format!("other@acme.{DOMAIN}")),
                    EmailAddress::from("manager@example.com"),
                    EmailAddress::from("manager@example.com"),
                ],
                ..context("customer@example.com")
            }))
            .unwrap(),
    );

    assert_eq!(
        rendered.recipients_cc,
        vec![EmailAddress::from("manager@example.com")]
    );
}

/// A channel's mail carries the hop budget; a platform notice deliberately carries none.
///
/// Without the header the receiving side has no hop count to continue, which is exactly what makes
/// a bounce or a stop notice unanswerable.
#[test]
fn only_a_relayed_message_carries_the_loop_control_headers() {
    let relayed = frozen(
        &renderer()
            .render(&envelope(context("c@example.com")))
            .unwrap(),
    );
    let headers = relayed.wire_headers();
    let value = |name: &str| {
        headers
            .iter()
            .find(|header| header.name == name)
            .map(|header| header.value.clone())
    };
    assert_eq!(value("Auto-Submitted").as_deref(), Some("auto-replied"));
    // The hop this mail *is*, not the hop it answers: ingress compares the received value against
    // its budget, so incrementing here is what makes the budget finite.
    assert_eq!(value("X-MailAgents-Hop-Count").as_deref(), Some("2"));
    assert!(value("X-MailAgents-Trace").is_some());

    let notice = frozen(
        &renderer()
            .render(&envelope(EmailDeliveryContext {
                relay: None,
                ..context("c@example.com")
            }))
            .unwrap(),
    );
    let notice_headers = notice.wire_headers();
    assert!(
        notice_headers
            .iter()
            .all(|header| !header.name.starts_with("X-MailAgents-")),
        "a notice must offer nothing to answer with"
    );
    assert!(
        notice_headers
            .iter()
            .any(|header| header.name == "Auto-Submitted"),
        "it is still auto-submitted, which is what stops it being answered"
    );
}

/// An internal hop costs exactly what an external one costs.
///
/// The relay is a transport, so what it hands the receiving channel has to be what the headers
/// would have carried. Handing over the pre-increment values instead makes an in-process hop free,
/// and the inter-channel loop budget is never spent -- which is a loop with no bound on it.
#[test]
fn the_internal_relay_and_the_wire_agree_on_the_hop_budget() {
    let relayed = frozen(
        &renderer()
            .render(&envelope(context("c@example.com")))
            .unwrap(),
    );
    let mail = relayed
        .as_relay_mail()
        .expect("a channel's mail can be relayed");
    let header = relayed
        .wire_headers()
        .into_iter()
        .find(|header| header.name == "X-MailAgents-Hop-Count")
        .expect("a relayed message carries its hop count");

    assert_eq!(header.value, mail.hop_count.to_string());
    assert!(
        mail.trace.contains(&mail.source_channel_id),
        "the sending channel joins the trace, or a return hop reads as an unexplained cycle"
    );
}

/// The renderer refuses a context it does not speak rather than reading the wrong arm.
#[test]
fn a_renderer_refuses_a_context_for_another_transport() {
    let content = CanonicalContent::parse("Subject", "Body").unwrap();
    let mismatch = DeliveryEnvelope::new(
        crate::entities::transport::DeliveryId::random(),
        DeliveryIntent {
            message_id: CanonicalMessageId::random(),
            source_binding_id: ChannelBindingId::random(),
            destination: DeliveryDestination::Binding(ChannelBindingId::random()),
            purpose: DeliveryPurpose::Mirror,
            key: DeliveryKey::parse("mirror:key").unwrap(),
        },
        TransportKind::Slack,
        CorrelationId::new(),
        &content,
        crate::transport::DeliveryContext::Email(context("c@example.com")),
    );
    assert!(
        mismatch.is_err(),
        "an email context cannot ride a Slack delivery"
    );
}

/// A relay that fails gives no verdict, and the honest classification is ambiguity.
///
/// An accepted `DATA` whose final acknowledgement was lost is indistinguishable here from a refused
/// connection, so a retry is exactly how one message becomes two. This is the behaviour the shape
/// this replaces got wrong.
#[tokio::test]
async fn a_failed_submission_is_ambiguous_rather_than_retried() {
    let sender = EmailSender::new(
        Arc::new(RefusingTransport),
        Arc::new(NeverInternal::default()),
    );
    let parts = renderer()
        .render(&envelope(EmailDeliveryContext {
            relay: None,
            ..context("outsider@example.com")
        }))
        .unwrap();

    let outcome = sender.send(&record(), &parts[0]).await;
    assert!(
        matches!(outcome, ProviderSendOutcome::OutcomeUnknown { .. }),
        "a lost acknowledgement must not be re-sent: {outcome:?}"
    );
    assert!(!outcome.is_safely_retryable());
}

/// A successful submission names the `Message-ID` it went out under as the provider key, because
/// that is the value a recipient's `References:` header will quote back.
#[tokio::test]
async fn a_delivered_mail_reports_the_message_id_it_went_out_under() {
    let transport = Arc::new(RecordingTransport::default());
    let sender = EmailSender::new(transport.clone(), Arc::new(NeverInternal::default()));
    let parts = renderer()
        .render(&envelope(EmailDeliveryContext {
            relay: None,
            ..context("outsider@example.com")
        }))
        .unwrap();
    let expected = frozen(&parts).message_id;

    match sender.send(&record(), &parts[0]).await {
        ProviderSendOutcome::Delivered { provider_key } => assert_eq!(
            provider_key.map(|key| key.as_str().to_string()),
            Some(expected.as_str().to_string())
        ),
        other => panic!("a working relay delivers: {other:?}"),
    }
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
}

/// A payload this build cannot read is terminal, not retryable: it will not become readable on the
/// fifth attempt, and spending five backoffs to reach the same verdict is pure delay.
#[tokio::test]
async fn an_unreadable_payload_is_terminal() {
    let sender = EmailSender::new(
        Arc::new(RecordingTransport::default()),
        Arc::new(NeverInternal::default()),
    );
    let alien = RenderedPart {
        index: crate::transport::PartIndex::new(0),
        key: crate::transport::PartKey::parse("email:alien").unwrap(),
        payload: crate::transport::TransportPayload::encode(
            TransportKind::Email,
            OUTBOUND_EMAIL_VERSION + 1,
            &serde_json::json!({}),
        )
        .unwrap(),
        digest: ContentDigest::sha256_of(b"body"),
    };

    let outcome = sender.send(&record(), &alien).await;
    assert!(
        matches!(
            outcome,
            ProviderSendOutcome::Terminal {
                class: FailureClass::InvalidPayload,
                ..
            }
        ),
        "{outcome:?}"
    );
}

/// A recipient this deployment owns never reaches SMTP.
#[tokio::test]
async fn a_recipient_of_our_own_is_relayed_rather_than_posted() {
    let transport = Arc::new(RecordingTransport::default());
    let relay = Arc::new(NeverInternal {
        disposition: Mutex::new(RelayDisposition::Relayed),
    });
    let sender = EmailSender::new(transport.clone(), relay);
    let parts = renderer()
        .render(&envelope(context("other@acme.mailagents.test")))
        .unwrap();

    let outcome = sender.send(&record(), &parts[0]).await;
    assert!(matches!(outcome, ProviderSendOutcome::Delivered { .. }));
    assert!(
        transport.sent.lock().unwrap().is_empty(),
        "an internal hop must not leave the building"
    );
}

/// A refusal by one of our own channels is definite: the same channel will refuse the same message
/// next time, so it goes terminal rather than spending an attempt budget on it.
#[tokio::test]
async fn an_internal_refusal_is_terminal() {
    let relay = Arc::new(NeverInternal {
        disposition: Mutex::new(RelayDisposition::Refused(
            "Max inter-channel hop count reached".into(),
        )),
    });
    let sender = EmailSender::new(Arc::new(RecordingTransport::default()), relay);
    let parts = renderer()
        .render(&envelope(context("other@acme.mailagents.test")))
        .unwrap();

    let outcome = sender.send(&record(), &parts[0]).await;
    assert!(
        matches!(
            outcome,
            ProviderSendOutcome::Terminal {
                class: FailureClass::DestinationUnavailable,
                ..
            }
        ),
        "{outcome:?}"
    );
}

/// The durable identity a sender is handed.
fn record() -> DeliveryRecord {
    DeliveryRecord {
        id: crate::entities::transport::DeliveryId::random(),
        attribution: Some(crate::transport::DeliveryAttribution {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            message_id: CanonicalMessageId::random(),
            source_binding_id: ChannelBindingId::random(),
            destination_binding_id: ChannelBindingId::random(),
        }),
        external_destination: None,
        task_id: None,
        correlation_id: CorrelationId::new(),
        transport: TransportKind::Email,
        purpose: DeliveryPurpose::Reply,
        idempotency_key: DeliveryKey::parse("reply:task:abc:email:customer@example.com").unwrap(),
        attempt_count: 0,
        max_attempts: MAX_DELIVERY_ATTEMPTS,
    }
}

#[derive(Default)]
struct RecordingTransport {
    sent: Mutex<Vec<MailMessage>>,
}

#[async_trait]
impl MailTransport for RecordingTransport {
    async fn send(&self, message: MailMessage) -> AppResult<()> {
        self.sent.lock().unwrap().push(message);
        Ok(())
    }
}

struct RefusingTransport;

#[async_trait]
impl MailTransport for RefusingTransport {
    async fn send(&self, _message: MailMessage) -> AppResult<()> {
        Err(AppError::Internal("the relay closed the connection".into()))
    }
}

/// A relay that answers however the test told it to, defaulting to "not one of ours".
struct NeverInternal {
    disposition: Mutex<RelayDisposition>,
}

impl Default for NeverInternal {
    fn default() -> Self {
        Self {
            disposition: Mutex::new(RelayDisposition::NotInternal),
        }
    }
}

#[async_trait]
impl InternalMailRelay for NeverInternal {
    async fn relay_internal(&self, _mail: &InternalRelayMail<'_>) -> AppResult<RelayDisposition> {
        Ok(self.disposition.lock().unwrap().clone())
    }
}

/// The composer is what every producer goes through, and its key shape is what deduplication is
/// scoped by. Two purposes, two sources or two destinations are three different deliveries.
#[test]
fn a_delivery_key_separates_purpose_source_and_destination() {
    let destination = |address: &str| {
        DeliveryDestination::External(crate::entities::transport::ExternalDestination::Email(
            EmailAddress::from(address),
        ))
    };
    let key = |purpose, source: &str, address: &str| {
        crate::transport::delivery_key(purpose, source, &destination(address))
            .as_str()
            .to_string()
    };

    let base = key(DeliveryPurpose::Reply, "task:1", "a@example.com");
    assert_eq!(base, "reply:task:1:email:a@example.com");
    assert_ne!(
        base,
        key(DeliveryPurpose::Outreach, "task:1", "a@example.com")
    );
    assert_ne!(base, key(DeliveryPurpose::Reply, "task:2", "a@example.com"));
    assert_ne!(base, key(DeliveryPurpose::Reply, "task:1", "b@example.com"));
    // Case-folded, so one recipient written two ways is one delivery rather than two.
    assert_eq!(base, key(DeliveryPurpose::Reply, "task:1", "A@Example.com"));

    // The composer type is the only thing that builds these in production; naming it here keeps
    // the import honest about that.
    let _: fn(
        std::sync::Arc<crate::transport::ports::TransportRenderers>,
        std::sync::Arc<dyn crate::use_cases::integration::ChannelBindingPersistence>,
    ) -> DeliveryComposer = DeliveryComposer::new;
}
