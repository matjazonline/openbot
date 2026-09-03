//! Contract tests for the transport ports.
//!
//! These prove the two things a future transport adapter is allowed to rely on: that fan-out
//! excludes exactly what policy says it excludes, and that a sender can report an ambiguous
//! provider result without it being flattened into success or failure.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        creation::CreationProvenance,
        message::CanonicalMessageId,
        transport::{
            BindingAccessPolicy, BindingAccessSnapshot, BindingDeliveryPolicy, BindingStatus,
            ChannelBinding, ChannelBindingId, DeliveryId, EndpointNamespace, ExternalDestination,
            ExternalEndpointKey, ExternalMessageKey, InstallationId, TransportKind,
        },
        value_objects::EmailAddress,
    },
    transport::{
        CanonicalContent, DeliveryCandidate, DeliveryDestination, DeliveryEnvelope, DeliveryIntent,
        DeliveryPlanRequest, DeliveryPlanner, DeliveryPurpose, FailureClass, FailureDetail,
        PartIndex, PartKey, PolicyDeliveryPlanner, ProviderSendOutcome, RenderedPart,
        TransportPayload, TransportRegistrationError, TransportRegistry, TransportRenderer,
        TransportSender, delivery::ContentDigest,
    },
};

fn binding(
    transport: TransportKind,
    status: BindingStatus,
    delivery_policy: BindingDeliveryPolicy,
) -> ChannelBinding {
    ChannelBinding {
        id: ChannelBindingId::random(),
        company_id: Uuid::new_v4(),
        channel_id: Uuid::new_v4(),
        installation_id: transport
            .requires_installation()
            .then(InstallationId::random),
        transport,
        namespace: EndpointNamespace::parse("namespace").unwrap(),
        external_endpoint_key: ExternalEndpointKey::parse("endpoint").unwrap(),
        display_label: "Support".into(),
        access_policy: BindingAccessPolicy::ChannelAcl,
        delivery_policy,
        status,
        disabled_reason: None,
        created_by: CreationProvenance::system(),
        access_snapshot: BindingAccessSnapshot::deployment_endpoint(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn active_email_binding() -> ChannelBinding {
    binding(
        TransportKind::Email,
        BindingStatus::Active,
        BindingDeliveryPolicy::ReplyAndInitiate,
    )
}

/// One row of the fan-out table: the bindings offered, and the destinations that must come back.
struct FanOutCase {
    name: &'static str,
    purpose: DeliveryPurpose,
    candidates: Vec<DeliveryCandidate<'static>>,
    explicit: Vec<ExternalDestination>,
    expected_bindings: Vec<ChannelBindingId>,
    expected_external: usize,
}

#[test]
fn fan_out_includes_exactly_what_policy_allows() {
    // Leaked so the cases can hold `&'static ChannelBinding` and stay readable as a table; a test
    // process ends before the leak matters.
    let source: &'static ChannelBinding = Box::leak(Box::new(active_email_binding()));
    let peer: &'static ChannelBinding = Box::leak(Box::new(active_email_binding()));
    let second_peer: &'static ChannelBinding = Box::leak(Box::new(binding(
        TransportKind::Slack,
        BindingStatus::Active,
        BindingDeliveryPolicy::ReplyAndInitiate,
    )));
    let disabled: &'static ChannelBinding = Box::leak(Box::new(binding(
        TransportKind::Email,
        BindingStatus::Disabled,
        BindingDeliveryPolicy::ReplyAndInitiate,
    )));
    let paused: &'static ChannelBinding = Box::leak(Box::new(binding(
        TransportKind::Slack,
        BindingStatus::Paused,
        BindingDeliveryPolicy::ReplyAndInitiate,
    )));
    let reply_only: &'static ChannelBinding = Box::leak(Box::new(binding(
        TransportKind::Slack,
        BindingStatus::Active,
        BindingDeliveryPolicy::ReplyOnly,
    )));
    let uninstalled: &'static ChannelBinding = Box::leak(Box::new(binding(
        TransportKind::Slack,
        BindingStatus::Active,
        BindingDeliveryPolicy::ReplyAndInitiate,
    )));

    let cases = vec![
        FanOutCase {
            name: "the source binding never receives its own message",
            purpose: DeliveryPurpose::Mirror,
            candidates: vec![
                DeliveryCandidate::deployment(source),
                DeliveryCandidate::deployment(peer),
            ],
            explicit: Vec::new(),
            expected_bindings: vec![peer.id],
            expected_external: 0,
        },
        FanOutCase {
            name: "a binding that is not active is excluded",
            purpose: DeliveryPurpose::Mirror,
            candidates: vec![
                DeliveryCandidate::deployment(disabled),
                DeliveryCandidate::deployment(paused),
                DeliveryCandidate::deployment(peer),
            ],
            explicit: Vec::new(),
            expected_bindings: vec![peer.id],
            expected_external: 0,
        },
        FanOutCase {
            name: "a binding whose installation is unusable is excluded",
            purpose: DeliveryPurpose::Mirror,
            candidates: vec![
                DeliveryCandidate {
                    binding: uninstalled,
                    installation_usable: false,
                },
                DeliveryCandidate::deployment(peer),
            ],
            explicit: Vec::new(),
            expected_bindings: vec![peer.id],
            expected_external: 0,
        },
        FanOutCase {
            name: "a reply-only binding takes replies but not outreach",
            purpose: DeliveryPurpose::Outreach,
            candidates: vec![
                DeliveryCandidate::deployment(reply_only),
                DeliveryCandidate::deployment(peer),
            ],
            explicit: Vec::new(),
            expected_bindings: vec![peer.id],
            expected_external: 0,
        },
        FanOutCase {
            name: "a reply reaches the same reply-only binding",
            purpose: DeliveryPurpose::Reply,
            candidates: vec![DeliveryCandidate::deployment(reply_only)],
            explicit: Vec::new(),
            expected_bindings: vec![reply_only.id],
            expected_external: 0,
        },
        FanOutCase {
            name: "an explicitly named destination is retained",
            purpose: DeliveryPurpose::Outreach,
            candidates: vec![DeliveryCandidate::deployment(source)],
            explicit: vec![ExternalDestination::Email(EmailAddress::from(
                "person@example.com",
            ))],
            expected_bindings: Vec::new(),
            expected_external: 1,
        },
        FanOutCase {
            name: "several different eligible bindings are all delivered to",
            purpose: DeliveryPurpose::Mirror,
            candidates: vec![
                DeliveryCandidate::deployment(source),
                DeliveryCandidate::deployment(peer),
                DeliveryCandidate::deployment(second_peer),
            ],
            explicit: Vec::new(),
            expected_bindings: vec![peer.id, second_peer.id],
            expected_external: 0,
        },
    ];

    let message_id = CanonicalMessageId::random();
    for case in cases {
        let intents = PolicyDeliveryPlanner.plan(&DeliveryPlanRequest {
            message_id,
            source_binding_id: source.id,
            purpose: case.purpose,
            candidates: &case.candidates,
            explicit: &case.explicit,
        });

        let bindings: Vec<_> = intents
            .iter()
            .filter_map(DeliveryIntent::destination_binding)
            .collect();
        assert_eq!(bindings, case.expected_bindings, "{}", case.name);
        assert_eq!(
            intents
                .iter()
                .filter(|intent| matches!(intent.destination, DeliveryDestination::External(_)))
                .count(),
            case.expected_external,
            "{}",
            case.name
        );
        let keys: std::collections::HashSet<_> =
            intents.iter().map(|intent| intent.key.clone()).collect();
        assert_eq!(
            keys.len(),
            intents.len(),
            "{}: one key per destination, or a fan-out silently loses a delivery",
            case.name
        );
        assert!(
            intents.iter().all(|intent| intent.key
                == DeliveryIntent::stable_key(case.purpose, message_id, &intent.destination)),
            "{}: every key is derived from its own destination",
            case.name
        );
    }
}

/// Replanning after a crash must produce the key that already exists, or the unique index has
/// nothing to absorb and the message goes out twice.
#[test]
fn the_same_logical_delivery_replans_to_the_same_key() {
    let message_id = CanonicalMessageId::random();
    let destination = DeliveryDestination::Binding(ChannelBindingId::random());
    assert_eq!(
        DeliveryIntent::stable_key(DeliveryPurpose::Reply, message_id, &destination),
        DeliveryIntent::stable_key(DeliveryPurpose::Reply, message_id, &destination)
    );
    assert_ne!(
        DeliveryIntent::stable_key(DeliveryPurpose::Reply, message_id, &destination),
        DeliveryIntent::stable_key(DeliveryPurpose::Mirror, message_id, &destination)
    );

    // Two outreach recipients on one message: no binding column separates these rows, so the keys
    // have to.
    let first = DeliveryDestination::External(ExternalDestination::Email(EmailAddress::from(
        "first@example.com",
    )));
    let second = DeliveryDestination::External(ExternalDestination::Email(EmailAddress::from(
        "second@example.com",
    )));
    assert_ne!(
        DeliveryIntent::stable_key(DeliveryPurpose::Outreach, message_id, &first),
        DeliveryIntent::stable_key(DeliveryPurpose::Outreach, message_id, &second)
    );
    // And one recipient written two ways is one delivery.
    let shouting = DeliveryDestination::External(ExternalDestination::Email(EmailAddress::from(
        "First@Example.COM",
    )));
    assert_eq!(
        DeliveryIntent::stable_key(DeliveryPurpose::Outreach, message_id, &first),
        DeliveryIntent::stable_key(DeliveryPurpose::Outreach, message_id, &shouting)
    );
}

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct FakePayload {
    body: String,
}

const FAKE_PAYLOAD_VERSION: u16 = 1;

fn envelope(transport: TransportKind) -> DeliveryEnvelope {
    let content = CanonicalContent::parse("Subject", "Body").unwrap();
    let part = RenderedPart {
        index: PartIndex::new(0),
        key: PartKey::parse("part-0").unwrap(),
        payload: TransportPayload::encode(
            transport,
            FAKE_PAYLOAD_VERSION,
            &FakePayload {
                body: "Body".into(),
            },
        )
        .unwrap(),
        digest: ContentDigest::sha256_of(b"Body"),
    };
    DeliveryEnvelope::new(
        DeliveryId::random(),
        DeliveryIntent {
            message_id: CanonicalMessageId::random(),
            source_binding_id: ChannelBindingId::random(),
            destination: DeliveryDestination::Binding(ChannelBindingId::random()),
            purpose: DeliveryPurpose::Reply,
            key: DeliveryIntent::stable_key(
                DeliveryPurpose::Reply,
                CanonicalMessageId::random(),
                &DeliveryDestination::Binding(ChannelBindingId::random()),
            ),
        },
        transport,
        &content,
        vec![part],
    )
    .unwrap()
}

/// A sender that answers with whatever the test queued. It cannot return a bare `Err`, because the
/// port has no error channel -- which is the property being asserted.
struct ScriptedSender {
    transport: TransportKind,
    outcome: ProviderSendOutcome,
}

#[async_trait]
impl TransportSender for ScriptedSender {
    fn transport(&self) -> TransportKind {
        self.transport
    }

    async fn send(
        &self,
        _envelope: &DeliveryEnvelope,
        _part: &RenderedPart,
    ) -> ProviderSendOutcome {
        self.outcome.clone()
    }
}

struct EchoRenderer {
    transport: TransportKind,
}

impl TransportRenderer for EchoRenderer {
    fn transport(&self) -> TransportKind {
        self.transport
    }

    fn render(&self, envelope: &DeliveryEnvelope) -> AppResult<Vec<RenderedPart>> {
        Ok(envelope.parts.to_vec())
    }
}

#[tokio::test]
async fn a_sender_reports_rate_limits_and_ambiguity_as_distinct_outcomes() {
    let envelope = envelope(TransportKind::Slack);
    let part = &envelope.parts[0];

    let rate_limited = ScriptedSender {
        transport: TransportKind::Slack,
        outcome: ProviderSendOutcome::RetryAfter {
            retry_after: Duration::from_secs(30),
            class: FailureClass::RateLimited,
            detail: FailureDetail::parse("retry_after=30").unwrap(),
        },
    };
    let outcome = rate_limited.send(&envelope, part).await;
    assert_eq!(outcome.retry_after(), Some(Duration::from_secs(30)));
    assert!(outcome.is_safely_retryable());
    assert_eq!(outcome.class(), Some(FailureClass::RateLimited));

    let ambiguous = ScriptedSender {
        transport: TransportKind::Slack,
        outcome: ProviderSendOutcome::OutcomeUnknown {
            class: FailureClass::Timeout,
            detail: FailureDetail::parse("connection dropped after the request was written")
                .unwrap(),
        },
    };
    let outcome = ambiguous.send(&envelope, part).await;
    // The distinction the whole delivery state machine rests on: this may already have been
    // accepted, so it is never re-sent automatically.
    assert!(!outcome.is_safely_retryable());
    assert_eq!(outcome.retry_after(), None);

    let delivered = ScriptedSender {
        transport: TransportKind::Slack,
        outcome: ProviderSendOutcome::Delivered {
            provider_key: Some(ExternalMessageKey::parse("1712345678.123456").unwrap()),
        },
    };
    assert_eq!(delivered.send(&envelope, part).await.class(), None);
}

#[test]
fn a_payload_is_read_back_only_by_the_transport_and_version_that_wrote_it() {
    let envelope = envelope(TransportKind::Slack);
    let payload = &envelope.parts[0].payload;

    assert_eq!(
        payload
            .decode::<FakePayload>(TransportKind::Slack, FAKE_PAYLOAD_VERSION)
            .unwrap(),
        FakePayload {
            body: "Body".into()
        }
    );
    assert!(
        payload
            .decode::<FakePayload>(TransportKind::Email, FAKE_PAYLOAD_VERSION)
            .is_err()
    );
    assert!(
        payload
            .decode::<FakePayload>(TransportKind::Slack, FAKE_PAYLOAD_VERSION + 1)
            .is_err()
    );
}

#[test]
fn an_over_limit_rendered_payload_is_refused_rather_than_stored() {
    let oversized = FakePayload {
        body: "x".repeat(crate::transport::MAX_PART_PAYLOAD_BYTES + 1),
    };
    assert!(
        TransportPayload::encode(TransportKind::Slack, FAKE_PAYLOAD_VERSION, &oversized).is_err()
    );

    // And on the way back in, where the value did not come from `encode` at all.
    let stored = serde_json::json!({
        "transport": "slack",
        "version": FAKE_PAYLOAD_VERSION,
        "body": { "body": "x".repeat(crate::transport::MAX_PART_PAYLOAD_BYTES + 1) },
    });
    assert!(serde_json::from_value::<TransportPayload>(stored).is_err());
}

#[test]
fn a_registry_refuses_a_mismatched_or_repeated_transport() {
    let registry = TransportRegistry::new()
        .register(
            Arc::new(EchoRenderer {
                transport: TransportKind::Email,
            }),
            Arc::new(ScriptedSender {
                transport: TransportKind::Email,
                outcome: ProviderSendOutcome::Delivered { provider_key: None },
            }),
        )
        .expect("a matched pair registers");

    assert!(registry.require(TransportKind::Email).is_ok());
    assert!(registry.require(TransportKind::Slack).is_err());

    let mismatched = registry.clone().register(
        Arc::new(EchoRenderer {
            transport: TransportKind::Slack,
        }),
        Arc::new(ScriptedSender {
            transport: TransportKind::Email,
            outcome: ProviderSendOutcome::Delivered { provider_key: None },
        }),
    );
    assert_eq!(
        mismatched.err(),
        Some(TransportRegistrationError::Mismatched {
            renderer: TransportKind::Slack,
            sender: TransportKind::Email,
        })
    );

    let duplicate = registry.register(
        Arc::new(EchoRenderer {
            transport: TransportKind::Email,
        }),
        Arc::new(ScriptedSender {
            transport: TransportKind::Email,
            outcome: ProviderSendOutcome::Delivered { provider_key: None },
        }),
    );
    assert_eq!(
        duplicate.err(),
        Some(TransportRegistrationError::Duplicate {
            transport: TransportKind::Email,
        })
    );
}
