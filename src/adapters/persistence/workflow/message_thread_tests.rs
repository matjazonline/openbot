//! 04.11-D2 §1.2-§1.5 (F005): replies resolve the exact source, sends never join a customer
//! conversation, and existing thread consumers keep their meaning.
use super::*;
use crate::application::transport::ExternalCorrelationStore;
use crate::application::use_cases::thread::{
    MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, MessageWrite,
    ThreadPersistence, qualified_email_identity,
};
use crate::application::workflow::DeliveryPolicy;
use crate::application::workflow::lease::ClaimedWorkflow;
use crate::domain::entities::correlation::CorrelationId;
use crate::entities::email_message::EmailMessageMetadata;
use crate::entities::message::{
    CanonicalMessageId, MessageDirection, MessageParticipantKind, MessageRole, ThreadEntryKind,
};
use crate::entities::participant::{IdentityClaimMetadata, IdentityProvenance};
use crate::entities::transport::{
    ChannelBindingId, ExternalMessageKey, ExternalThreadKey, IdentityNamespace, IdentitySubject,
    QualifiedIdentity, TransportKind,
};
use crate::entities::value_objects::{EmailAddress, MessageId};
use crate::use_cases::participant::IdentityObservation;

const CUSTOMER: &str = "customer@partner.test";

pub(super) fn inbound(thread: Uuid, rfc: &str, references: &[&str], body: &str) -> MessageWrite {
    let identity = qualified_email_identity(CUSTOMER).unwrap();
    MessageWrite::internal(
        thread,
        MessageAuthorWrite::Observed(IdentityObservation {
            identity: identity.clone(),
            display_label: None,
            claim_metadata: IdentityClaimMetadata::observation(),
            provenance: IdentityProvenance::TransportIngress,
        }),
        "Question",
        body,
        MessageDirection::Inbound,
        MessageRole::Human,
        CorrelationId::new(),
    )
    .external_conversation()
    .with_participants(vec![
        MessageParticipantWrite::new(MessageParticipantKind::Sender, identity),
        MessageParticipantWrite::new(
            MessageParticipantKind::To,
            qualified_email_identity("runs@workflow-tests.agents.test").unwrap(),
        ),
    ])
    .with_correlation(MessageCorrelation::Email(
        EmailMessageMetadata::new(MessageId::from(rfc))
            .references(references.iter().map(|r| MessageId::from(*r)).collect())
            .raw_bodies(Some(body.into()), None),
    ))
}

struct Conversation {
    setup: Setup,
    thread: Uuid,
    question: CanonicalMessageId,
}

/// A customer email thread in the run channel, a run associated with that thread, and a
/// `message.*` step built by `step` from (channel, thread, question, other channel, member).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Question,
    Note,
    /// An earlier outbound email answer in the same thread.
    Outbound,
    /// An inbound email in another channel's thread, outside the run's scope.
    Foreign,
    /// An inbound Slack message in the run's thread: no email reply can answer it.
    Slack,
    Missing,
}

async fn conversation(
    step: impl FnOnce(Uuid, Uuid, CanonicalMessageId, Uuid, Uuid) -> (&'static str, Value),
    kind: Source,
) -> Conversation {
    conversation_with(step, kind, DeliveryPolicy::Deliver).await
}

/// `conversation` with the run admitted under `policy`.
async fn conversation_with(
    step: impl FnOnce(Uuid, Uuid, CanonicalMessageId, Uuid, Uuid) -> (&'static str, Value),
    kind: Source,
    policy: DeliveryPolicy,
) -> Conversation {
    let fixture = Fixture::new().await;
    let channel = new_channel(&fixture, "runs").await;
    let other = new_channel(&fixture, "notices").await;
    let user = member(&fixture, "member").await;
    let p = &fixture.persistence;
    let thread = p
        .create_thread(channel, "Question", &[EmailAddress::from(CUSTOMER)])
        .await
        .unwrap();
    p.create_message(&inbound(thread.id, "<root@partner.test>", &[], "First"))
        .await
        .unwrap();
    let question = p
        .create_message(&inbound(
            thread.id,
            "<question@partner.test>",
            &["<root@partner.test>"],
            "The question",
        ))
        .await
        .unwrap()
        .canonical_id;
    let source = match kind {
        Source::Question => question,
        Source::Missing => CanonicalMessageId::new(Uuid::new_v4()),
        Source::Note => {
            p.create_message(
                &MessageWrite::internal(
                    thread.id,
                    MessageAuthorWrite::Platform,
                    "Note",
                    "An internal note",
                    MessageDirection::Outbound,
                    MessageRole::System,
                    CorrelationId::new(),
                )
                .with_entry_kind(ThreadEntryKind::Note),
            )
            .await
            .unwrap()
            .canonical_id
        }
        Source::Outbound => {
            p.create_message(
                &MessageWrite::internal(
                    thread.id,
                    MessageAuthorWrite::Platform,
                    "Re: Question",
                    "An earlier answer",
                    MessageDirection::Outbound,
                    MessageRole::System,
                    CorrelationId::new(),
                )
                .external_conversation()
                .with_participants(vec![
                    MessageParticipantWrite::new(
                        MessageParticipantKind::Sender,
                        qualified_email_identity("runs@workflow-tests.agents.test").unwrap(),
                    ),
                    MessageParticipantWrite::new(
                        MessageParticipantKind::To,
                        qualified_email_identity(CUSTOMER).unwrap(),
                    ),
                ])
                .with_correlation(MessageCorrelation::Email(
                    EmailMessageMetadata::new(MessageId::from("<ours@agents.test>"))
                        .raw_bodies(Some("An earlier answer".into()), None),
                )),
            )
            .await
            .unwrap()
            .canonical_id
        }
        Source::Slack => {
            let identity = QualifiedIdentity::new(
                TransportKind::Slack,
                IdentityNamespace::parse("T0WORKSPACE").unwrap(),
                IdentitySubject::parse("U0CUSTOMER").unwrap(),
            );
            p.create_message(
                &MessageWrite::internal(
                    thread.id,
                    MessageAuthorWrite::Observed(IdentityObservation {
                        identity: identity.clone(),
                        display_label: None,
                        claim_metadata: IdentityClaimMetadata::observation(),
                        provenance: IdentityProvenance::TransportIngress,
                    }),
                    "Question",
                    "Asked in Slack",
                    MessageDirection::Inbound,
                    MessageRole::Human,
                    CorrelationId::new(),
                )
                .external_conversation()
                .with_participants(vec![MessageParticipantWrite::new(
                    MessageParticipantKind::Sender,
                    identity,
                )]),
            )
            .await
            .unwrap()
            .canonical_id
        }
        Source::Foreign => {
            let elsewhere = p
                .create_thread(other, "Elsewhere", &[EmailAddress::from(CUSTOMER)])
                .await
                .unwrap();
            p.create_message(&inbound(
                elsewhere.id,
                "<elsewhere@partner.test>",
                &[],
                "Elsewhere",
            ))
            .await
            .unwrap()
            .canonical_id
        }
    };
    // A newer distractor: the reply must still answer its own source, not "the latest".
    p.create_message(&inbound(
        thread.id,
        "<newer@partner.test>",
        &["<root@partner.test>"],
        "Newer",
    ))
    .await
    .unwrap();
    let (kind, with) = step(channel, thread.id, source, other, user);
    let version = Box::pin(published(&fixture, kind, with)).await;
    let target = Box::pin(bound(&fixture, version)).await;
    let scope = admitted_with(
        &fixture,
        target,
        RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(channel),
            thread_id: RelatedThreadId::new(thread.id),
        },
        "message-thread-run",
        policy,
    )
    .await;
    let claim: ClaimedWorkflow = fixture
        .persistence
        .claim_io(
            scope,
            crate::application::workflow::lease::WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    Conversation {
        setup: Setup {
            fixture,
            channel,
            other,
            user,
            owner: Uuid::nil(),
            claim,
            kind: crate::application::workflow::polling::WorkflowStepKind(kind.into()),
        },
        thread: thread.id,
        question,
    }
}

fn reply(source: CanonicalMessageId) -> (&'static str, Value) {
    (
        "message.reply",
        json!({"content":{"literal":{"body":"Here is the answer."}},"source_message":{"literal":source.as_uuid().to_string()}}),
    )
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct Written {
    thread_id: Uuid,
    direction: String,
    audience: String,
    entry_kind: String,
}
async fn written(setup: &Setup) -> Vec<Written> {
    sqlx::query_as(
        "SELECT target.thread_id, message.direction, message.audience, entry.entry_kind FROM workflow_message_destinations AS target \
         JOIN messages AS message ON message.id=target.message_id JOIN thread_messages AS entry ON entry.message_id=target.message_id \
         WHERE target.run_id=$1 ORDER BY target.ordinal",
    )
    .bind(setup.scope().run.as_uuid())
    .fetch_all(setup.pool())
    .await
    .unwrap()
}

/// An external email reply: the source's own thread, Received threading on the source's RFC id
/// and references despite a newer distractor, the source author as recipient, and the
/// "already answered" guard treats it as the answer.
#[tokio::test]
async fn workflow_message_thread_external_reply_answers_its_own_source() {
    let conversation = Box::pin(conversation(
        |_, _, source, _, _| reply(source),
        Source::Question,
    ))
    .await;
    let setup = &conversation.setup;
    assert!(matches!(
        setup.runtime().execute(&setup.kind, &setup.claim).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    assert_eq!(
        written(setup).await,
        vec![Written {
            thread_id: conversation.thread,
            direction: "outbound".into(),
            audience: "external_conversation".into(),
            entry_kind: "conversation".into()
        }]
    );
    let (purpose, recipient): (String, String) = sqlx::query_as(
        "SELECT delivery.purpose, target.recipient_address FROM workflow_message_destinations AS target JOIN message_deliveries AS delivery ON delivery.id=target.delivery_id WHERE target.run_id=$1",
    )
    .bind(setup.scope().run.as_uuid()).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(
        (purpose.as_str(), recipient.as_str()),
        ("workflow_reply", CUSTOMER)
    );
    let (in_reply_to, references): (Option<String>, Option<Vec<String>>) = sqlx::query_as(
        "SELECT metadata.in_reply_to, metadata.references_list FROM workflow_message_destinations AS target JOIN email_message_metadata AS metadata ON metadata.message_id=target.message_id WHERE target.run_id=$1",
    )
    .bind(setup.scope().run.as_uuid()).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(in_reply_to.as_deref(), Some("<question@partner.test>"));
    assert_eq!(references, Some(vec!["<root@partner.test>".to_string()]));
    let p = &setup.fixture.persistence;
    let answer = p
        .find_outbound_reply_after(conversation.thread, conversation.question)
        .await
        .unwrap();
    assert!(
        answer.is_some(),
        "the guard sees the workflow reply as the answer"
    );
}

/// An internal reply is a note in the source's thread: neither the guard nor the RFC fallback
/// changes, and it carries no email delivery.
#[tokio::test]
async fn workflow_message_thread_internal_reply_note_leaves_guards_unchanged() {
    let conversation = Box::pin(conversation(
        |_, _, source, _, _| reply(source),
        Source::Note,
    ))
    .await;
    let setup = &conversation.setup;
    let p = &setup.fixture.persistence;
    let rfc_before = p
        .latest_thread_rfc_message_id(conversation.thread)
        .await
        .unwrap();
    assert!(matches!(
        setup.runtime().execute(&setup.kind, &setup.claim).await,
        WorkflowHandlerOutcome::Completed(Ok(_))
    ));
    assert_eq!(
        written(setup).await,
        vec![Written {
            thread_id: conversation.thread,
            direction: "outbound".into(),
            audience: "internal_only".into(),
            entry_kind: "note".into()
        }]
    );
    assert_eq!(rows(setup.pool(), setup.scope().run).await.deliveries, 0);
    assert!(
        p.find_outbound_reply_after(conversation.thread, conversation.question)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        p.latest_thread_rfc_message_id(conversation.thread)
            .await
            .unwrap(),
        rfc_before
    );
}

/// A send in a run associated with a customer thread opens its own threads and writes nothing
/// into the customer's: the channel agent's later answer is not suppressed, its In-Reply-To
/// fallback is unchanged.
#[tokio::test]
async fn workflow_message_thread_send_never_joins_the_customer_thread() {
    let conversation = Box::pin(conversation(
        |_, _, _, other, user| {
            (
                "message.send",
                json!({"content":{"literal":{"subject":"Status","body":"Sent elsewhere."}},
                       "destinations":{"literal":[{"kind":"user","id":user.to_string()},{"kind":"channel","id":other.to_string()}]}}),
            )
        },
        Source::Question,
    ))
    .await;
    let setup = &conversation.setup;
    let p = &setup.fixture.persistence;
    let rfc_before = p
        .latest_thread_rfc_message_id(conversation.thread)
        .await
        .unwrap();
    let entries_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM thread_messages WHERE thread_id=$1")
            .bind(conversation.thread)
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert!(matches!(
        setup.runtime().execute(&setup.kind, &setup.claim).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let written = written(setup).await;
    assert_eq!(written.len(), 2);
    assert!(
        written
            .iter()
            .all(|row| row.thread_id != conversation.thread)
    );
    assert_eq!(written[0].audience, "external_conversation");
    assert_eq!(written[1].audience, "internal_only");
    let entries_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM thread_messages WHERE thread_id=$1")
            .bind(conversation.thread)
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert_eq!(entries_after, entries_before);
    assert!(
        p.find_outbound_reply_after(conversation.thread, conversation.question)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        p.latest_thread_rfc_message_id(conversation.thread)
            .await
            .unwrap(),
        rfc_before
    );
    // The user email thread lives in the run channel and threads standalone (no In-Reply-To).
    let (channel, in_reply_to): (Uuid, Option<String>) = sqlx::query_as(
        "SELECT thread.channel_id, metadata.in_reply_to FROM workflow_message_destinations AS target JOIN threads AS thread ON thread.id=target.thread_id \
         JOIN email_message_metadata AS metadata ON metadata.message_id=target.message_id WHERE target.run_id=$1 AND target.ordinal=0",
    )
    .bind(setup.scope().run.as_uuid()).fetch_one(setup.pool()).await.unwrap();
    assert_eq!((channel, in_reply_to), (setup.channel, None));
}

/// Foreign, outbound, Slack and missing sources are refused before any write.
#[tokio::test]
async fn workflow_message_thread_reply_refuses_sources_it_cannot_answer() {
    for kind in [
        Source::Missing,
        Source::Outbound,
        Source::Foreign,
        Source::Slack,
    ] {
        let refused = Box::pin(conversation(|_, _, source, _, _| reply(source), kind)).await;
        let setup = &refused.setup;
        let WorkflowHandlerOutcome::Completed(Err(failure)) =
            setup.runtime().execute(&setup.kind, &setup.claim).await
        else {
            panic!("refused");
        };
        assert_eq!(failure.failure.code().as_str(), "message.rejected");
        assert_eq!(rows(setup.pool(), setup.scope().run).await.markers, 0);
    }
}

/// D2 §8.2 F005: a reply quoting either recipient's copy of one send resolves, through the
/// inbound router's own lookups (`find_ancestor_thread` and the conversation key), to that
/// recipient's canonical message and thread, never the other's.
#[tokio::test]
async fn workflow_message_thread_inbound_reply_resolves_each_recipients_copy() {
    let setup = Box::pin(send(&[To::User, To::Owner])).await;
    assert!(matches!(
        setup.runtime().execute(&setup.kind, &setup.claim).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let p = &setup.fixture.persistence;
    let binding: Uuid = sqlx::query_scalar(
        "SELECT id FROM channel_bindings WHERE company_id=$1 AND channel_id=$2 AND transport='email' \
           AND installation_id IS NULL ORDER BY created_at, id LIMIT 1",
    )
    .bind(setup.scope().company.as_uuid())
    .bind(setup.channel)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    let binding = ChannelBindingId::new(binding);
    let copies: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT target.message_id, target.thread_id, metadata.rfc_message_id FROM workflow_message_destinations AS target \
         JOIN email_message_metadata AS metadata ON metadata.message_id=target.message_id WHERE target.run_id=$1 ORDER BY target.ordinal",
    )
    .bind(setup.scope().run.as_uuid())
    .fetch_all(setup.pool())
    .await
    .unwrap();
    assert_eq!(copies.len(), 2);
    assert_ne!(copies[0].1, copies[1].1, "one thread per recipient");
    assert_ne!(copies[0].2, copies[1].2, "one Message-ID per recipient");
    for (index, (message, thread, rfc)) in copies.iter().enumerate() {
        // What the email adapter derives from a reply: its own key, then In-Reply-To.
        let keys = [
            ExternalMessageKey::parse(format!("<reply-{index}@partner.test>")).unwrap(),
            ExternalMessageKey::parse(rfc.as_str()).unwrap(),
        ];
        assert_eq!(
            p.thread_for_message_keys(binding, &keys).await.unwrap(),
            Some(*thread)
        );
        assert_eq!(
            p.thread_for_thread_keys(binding, &[ExternalThreadKey::parse(rfc.as_str()).unwrap()])
                .await
                .unwrap(),
            Some(*thread),
            "the conversation key is the copy's own Message-ID"
        );
        assert_eq!(
            p.message_for_external_key(binding, &keys[1])
                .await
                .unwrap()
                .map(|id| id.as_uuid()),
            Some(*message)
        );
    }
}

/// The relay trace frozen into the reply's email part.
async fn reply_relay(setup: &Setup) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_path_query_first(part.payload, 'strict $.**.relay') FROM workflow_message_destinations AS target \
         JOIN message_delivery_parts AS part ON part.delivery_id=target.delivery_id WHERE target.run_id=$1",
    )
    .bind(setup.scope().run.as_uuid())
    .fetch_one(setup.pool())
    .await
    .unwrap()
}

/// D2 §1.3 (R006 F015), p08 D1.6: an external reply continues the relay trace its source arrived
/// with, as the source's admission intent recorded it, with no legacy task; a source with none
/// recorded starts a chain at hop 0.
#[tokio::test]
async fn workflow_message_thread_external_reply_continues_the_source_relay_trace() {
    for recorded in [true, false] {
        let conversation = Box::pin(conversation(
            |_, _, source, _, _| reply(source),
            Source::Question,
        ))
        .await;
        let setup = &conversation.setup;
        let trace = vec![Uuid::new_v4(), Uuid::new_v4()];
        if recorded {
            sqlx::query(
                "INSERT INTO workflow_message_intents (company_id, id, message_id, channel_id, \
                   thread_id, role, mentioned, admissible, origin, hop_count, trace_channels, \
                   delivery_policy, correlation_id) \
                 VALUES ($1, $2, $3, $4, $5, 'to', false, true, 'internal', 2, $6, 'deliver', $2)",
            )
            .bind(setup.scope().company.as_uuid())
            .bind(Uuid::new_v4())
            .bind(conversation.question.as_uuid())
            .bind(setup.channel)
            .bind(conversation.thread)
            .bind(&trace)
            .execute(setup.pool())
            .await
            .unwrap();
        }
        assert!(matches!(
            setup.runtime().execute(&setup.kind, &setup.claim).await,
            WorkflowHandlerOutcome::Suspended(_)
        ));
        let (hop, channels) = if recorded {
            (2, trace.iter().map(Uuid::to_string).collect::<Vec<_>>())
        } else {
            (0, Vec::new())
        };
        assert_eq!(
            reply_relay(setup).await,
            json!({"source_channel_id": setup.channel.to_string(), "hop_count": hop, "trace_channels": channels}),
            "recorded={recorded}"
        );
    }
}

/// One destination row of the run, as stored.
#[derive(Debug, sqlx::FromRow)]
struct StoredDestination {
    kind: String,
    thread_id: Uuid,
    message_id: Uuid,
    delivery_id: Option<Uuid>,
    principal_id: Option<Uuid>,
    recipient_address: Option<String>,
    in_app: bool,
}

async fn destination(setup: &Setup) -> StoredDestination {
    sqlx::query_as(
        "SELECT kind, thread_id, message_id, delivery_id, principal_id, recipient_address, in_app \
         FROM workflow_message_destinations WHERE run_id = $1",
    )
    .bind(setup.scope().run.as_uuid())
    .fetch_one(setup.pool())
    .await
    .unwrap()
}

/// The members recorded as participants of `thread`, by principal.
async fn participants(setup: &Setup, thread: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT principal_id FROM thread_principals WHERE thread_id = $1 AND role = 'participant' \
         ORDER BY principal_id",
    )
    .bind(thread)
    .fetch_all(setup.pool())
    .await
    .unwrap()
}

fn send_to_user(user: Uuid) -> (&'static str, Value) {
    (
        "message.send",
        json!({"content":{"literal":{"subject":"Status","body":"The run finished."}},
               "destinations":{"literal":[{"kind":"user","id":user.to_string()}]}}),
    )
}

/// p08 D1.5.4/D1.7: a member's send opens its own thread in the run channel with the member as
/// its participant, emailed under `deliver` and published with no delivery under `in_app_only`,
/// where the step completes at once with a valid result. A member destination without a
/// delivery is refused for a `deliver` run.
#[tokio::test]
async fn workflow_message_thread_user_send_opens_a_thread_the_member_takes_part_in() {
    for policy in [DeliveryPolicy::Deliver, DeliveryPolicy::InAppOnly] {
        let conversation = Box::pin(conversation_with(
            |_, _, _, _, user| send_to_user(user),
            Source::Question,
            policy,
        ))
        .await;
        let setup = &conversation.setup;
        let outcome = setup.runtime().execute(&setup.kind, &setup.claim).await;
        let stored = destination(setup).await;
        let member: Uuid = sqlx::query_scalar("SELECT id FROM principals WHERE user_id = $1")
            .bind(setup.user)
            .fetch_one(setup.pool())
            .await
            .unwrap();
        assert_eq!(stored.kind, "user");
        assert_eq!(stored.principal_id, Some(member));
        assert_ne!(
            stored.thread_id, conversation.thread,
            "never the customer thread"
        );
        assert_eq!(participants(setup, stored.thread_id).await, vec![member]);
        let deliveries = rows(setup.pool(), setup.scope().run).await.deliveries;
        if policy == DeliveryPolicy::InAppOnly {
            let result = completed(outcome).unwrap();
            assert_eq!(result["status"], "accepted");
            assert_eq!(
                result["messages"][0]["message_id"],
                stored.message_id.to_string()
            );
            assert!(stored.in_app);
            assert_eq!((stored.delivery_id, stored.recipient_address), (None, None));
            assert_eq!(deliveries, 0);
            continue;
        }
        assert!(matches!(outcome, WorkflowHandlerOutcome::Suspended(_)));
        assert!(!stored.in_app);
        assert!(stored.delivery_id.is_some() && stored.recipient_address.is_some());
        assert_eq!(deliveries, 1);
        // The guard, not only the planner: a deliver run's member destination has a delivery.
        let refused = sqlx::query(
            "INSERT INTO workflow_message_destinations (company_id, run_id, invocation_id, ordinal, \
               kind, destination_id, thread_id, message_id, principal_id) \
             SELECT company_id, run_id, invocation_id, ordinal + 1, kind, destination_id, thread_id, \
               message_id, principal_id FROM workflow_message_destinations WHERE run_id = $1",
        )
        .bind(setup.scope().run.as_uuid())
        .execute(setup.pool())
        .await
        .unwrap_err();
        assert!(
            refused.to_string().contains("requires a delivery"),
            "{refused}"
        );
        // And the shape check: a delivery always names its address.
        let shapeless = sqlx::query(
            "INSERT INTO workflow_message_destinations (company_id, run_id, invocation_id, ordinal, \
               kind, destination_id, thread_id, message_id, delivery_id, principal_id) \
             SELECT company_id, run_id, invocation_id, ordinal + 1, kind, destination_id, thread_id, \
               message_id, delivery_id, principal_id FROM workflow_message_destinations WHERE run_id = $1",
        )
        .bind(setup.scope().run.as_uuid())
        .execute(setup.pool())
        .await
        .unwrap_err();
        assert!(
            shapeless
                .to_string()
                .contains("workflow_message_destinations_check"),
            "{shapeless}"
        );
    }
}

/// p08 D1.7: under `in_app_only` an email reply becomes an in-app reply in the source thread,
/// with no delivery and a valid result.
#[tokio::test]
async fn workflow_message_thread_in_app_reply_stays_in_the_source_thread() {
    let conversation = Box::pin(conversation_with(
        |_, _, source, _, _| reply(source),
        Source::Question,
        DeliveryPolicy::InAppOnly,
    ))
    .await;
    let setup = &conversation.setup;
    let result = completed(setup.runtime().execute(&setup.kind, &setup.claim).await).unwrap();
    assert_eq!(result["status"], "accepted");
    let stored = destination(setup).await;
    assert_eq!(
        (
            stored.kind.as_str(),
            stored.thread_id,
            stored.in_app,
            stored.delivery_id,
            stored.recipient_address,
        ),
        ("reply", conversation.thread, true, None, None)
    );
    assert_eq!(rows(setup.pool(), setup.scope().run).await.deliveries, 0);
}
