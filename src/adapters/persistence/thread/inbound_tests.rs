//! Database tests for the one transaction an accepted inbound message goes through.
//!
//! These are the assertions the in-memory double cannot make. Atomicity, the advisory lock that
//! serializes two simultaneous deliveries, and the unique indexes that absorb a redelivery are all
//! properties of PostgreSQL, so they are exercised against it.

use super::test_support::*;
use super::*;
use crate::entities::{
    correlation::CorrelationId,
    email_message::EmailMessageMetadata,
    participant::ThreadPrincipalRole,
    transport::{
        ChannelBindingId, ExternalMessageKey, ExternalThreadKey, IdentityNamespace,
        IdentitySubject, InboundSource, QualifiedIdentity, TransportKind,
    },
    value_objects::MessageId,
};
use crate::transport::{
    AddressedIdentity, BoundedVec, CanonicalContent, CommitDisposition, ExternalCorrelationStore,
    InboundCommitOutcome, InboundCommitRequest, InboundEnvelope, InboundMessageCommitter,
    InboundTaskRequest, InboundTaskTarget, IngressDirectives, IngressPolicyFacts, PipelineStep,
    ProtocolExtension, RecipientRole, ReplyDelivery, ThreadAssociation, ThreadPrincipalIntent,
    ThreadTarget,
};

/// The one task type inbound mail produces.
const AGENT_DISPATCH: &str = "email_agent_dispatch";

fn identity(address: &str) -> QualifiedIdentity {
    QualifiedIdentity::new(
        TransportKind::Email,
        IdentityNamespace::parse("email").unwrap(),
        IdentitySubject::parse(address).unwrap(),
    )
}

fn sender_principals()
-> BoundedVec<ThreadPrincipalIntent, { crate::transport::MAX_THREAD_PRINCIPALS }> {
    BoundedVec::parse(
        "thread principals",
        vec![
            ThreadPrincipalIntent::new(identity("sender@example.com"), ThreadPrincipalRole::Author),
            ThreadPrincipalIntent::new(
                identity("sender@example.com"),
                ThreadPrincipalRole::Participant,
            ),
        ],
    )
    .unwrap()
}

/// One arriving mail, bound to the interface it came in on.
fn envelope(binding_id: ChannelBindingId, rfc: &str, body: &str) -> InboundEnvelope {
    let metadata = EmailMessageMetadata::new(MessageId::from(rfc.to_string()))
        .raw_bodies(Some(body.to_string()), None);
    InboundEnvelope {
        source: InboundSource {
            binding_id,
            event_key: None,
            message_key: ExternalMessageKey::parse(rfc).unwrap(),
            thread_key: ExternalThreadKey::parse(rfc).unwrap(),
        },
        author: identity("sender@example.com"),
        addressed: BoundedVec::parse(
            "addressed identities",
            vec![AddressedIdentity::new(
                RecipientRole::To,
                identity("support@acme.example"),
            )],
        )
        .unwrap(),
        content: CanonicalContent::parse("Quick question", body).unwrap(),
        attachments: BoundedVec::empty(),
        reply_candidates: Default::default(),
        directives: IngressDirectives::default(),
        policy: IngressPolicyFacts::TrustedApplication,
        correlation_id: CorrelationId::new(),
        extension: ProtocolExtension::email(metadata),
    }
}

/// A commit that opens one thread on the fixture's channel and asks for an agent run.
async fn request(fixture: &Fixture, rfc: &str, body: &str) -> InboundCommitRequest {
    let binding_id = fixture.email_binding_of(fixture.channel_id).await;
    InboundCommitRequest {
        company_id: fixture.company_id,
        envelope: envelope(binding_id, rfc, body),
        claimed_event: None,
        associations: BoundedVec::parse(
            "thread associations",
            vec![ThreadAssociation {
                channel_id: fixture.channel_id,
                binding_id,
                target: ThreadTarget::Create {
                    subject: "Quick question".to_string(),
                },
                role: RecipientRole::To,
                step: PipelineStep::only(),
                principals: sender_principals(),
            }],
        )
        .unwrap(),
        task: Some(InboundTaskRequest {
            task_type: AGENT_DISPATCH.to_string(),
            targets: vec![InboundTaskTarget {
                channel_id: fixture.channel_id,
                role: RecipientRole::To,
            }],
        }),
        outreach_transitions: BoundedVec::empty(),
        deliveries: Vec::new(),
        reply_delivery: ReplyDelivery::Send,
    }
}

async fn count(fixture: &Fixture, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(fixture.company_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap()
}

async fn message_count(fixture: &Fixture) -> i64 {
    count(
        fixture,
        "SELECT count(*) FROM messages WHERE company_id = $1",
    )
    .await
}

async fn task_count(fixture: &Fixture) -> i64 {
    count(
        fixture,
        "SELECT count(*) FROM background_tasks WHERE company_id = $1",
    )
    .await
}

/// The whole point of the commit: the message, its mapping and its task exist together.
#[tokio::test]
async fn an_accepted_message_lands_with_its_mapping_thread_and_task_at_once() {
    let Some(fixture) = Fixture::new("inbound_commit").await else {
        return;
    };
    let rfc = format!("<one-{}@example.com>", fixture.suffix);

    let outcome = fixture
        .persistence
        .commit_inbound(request(&fixture, &rfc, "Anyone there?").await)
        .await
        .unwrap();

    assert_eq!(outcome.disposition, CommitDisposition::Created);
    assert_eq!(outcome.thread_ids.len(), 1);
    let task_id = outcome
        .task_id
        .expect("an answerable message enqueues a run");

    // No accepted message needs a post-commit mapping or task insert: every row is already there.
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_messages WHERE company_id = $1"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_threads WHERE company_id = $1"
        )
        .await,
        1
    );
    assert_eq!(task_count(&fixture).await, 1);

    // The task carries identifiers only, and names the message it answers.
    let (source, payload): (Option<Uuid>, serde_json::Value) =
        sqlx::query_as("SELECT source_message_uuid, payload FROM background_tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(source, Some(outcome.message_id.as_uuid()));
    assert_eq!(payload["version"], "1");
    assert_eq!(payload["source_message_id"], outcome.message_id.to_string());
    assert!(payload.get("parsed_email").is_none());

    // And the message is reachable through the thread it landed in.
    let stored = fixture
        .persistence
        .get_thread_message(outcome.thread_ids[0], outcome.message_id)
        .await
        .unwrap()
        .expect("the committed message is in its thread");
    assert_eq!(stored.clean_text_body, "Anyone there?");

    fixture.cleanup().await;
}

/// A provider retrying an event it never saw acknowledged is not a second message, and must not
/// start a second agent run on the same turn.
#[tokio::test]
async fn a_redelivery_returns_the_first_delivery_and_enqueues_nothing_further() {
    let Some(fixture) = Fixture::new("inbound_redelivery").await else {
        return;
    };
    let rfc = format!("<redelivered-{}@example.com>", fixture.suffix);

    let first = fixture
        .persistence
        .commit_inbound(request(&fixture, &rfc, "Anyone there?").await)
        .await
        .unwrap();
    let second = fixture
        .persistence
        .commit_inbound(request(&fixture, &rfc, "Anyone there?").await)
        .await
        .unwrap();

    assert_eq!(first.disposition, CommitDisposition::Created);
    assert_eq!(second.disposition, CommitDisposition::Duplicate);
    assert_eq!(second.message_id, first.message_id);
    assert_eq!(second.task_id, first.task_id);
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(task_count(&fixture).await, 1);
    // Two commits, but the second joined the thread the first opened rather than a second one.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM thread_messages WHERE company_id = $1"
        )
        .await,
        1
    );

    fixture.cleanup().await;
}

/// The same key carrying *different* content is not a redelivery at all. Rewriting the stored
/// message would rewrite history an agent has already read and answered.
#[tokio::test]
async fn a_repeated_key_with_changed_content_is_a_collision_rather_than_a_rewrite() {
    let Some(fixture) = Fixture::new("inbound_collision").await else {
        return;
    };
    let rfc = format!("<collision-{}@example.com>", fixture.suffix);

    fixture
        .persistence
        .commit_inbound(request(&fixture, &rfc, "The original").await)
        .await
        .unwrap();
    let changed = fixture
        .persistence
        .commit_inbound(request(&fixture, &rfc, "Something else entirely").await)
        .await;

    let error = changed.expect_err("a changed redelivery is refused");
    assert!(error.to_string().contains("different content"), "{error}");
    // The refusal wrote nothing: still one message, and it still says what it originally said.
    assert_eq!(message_count(&fixture).await, 1);
    let body: String =
        sqlx::query_scalar("SELECT clean_text_body FROM messages WHERE company_id = $1")
            .bind(fixture.company_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(body, "The original");

    fixture.cleanup().await;
}

/// Two deliveries of one message at the same instant. The advisory lock is what makes this a
/// question about ordering rather than a race: without it both transactions read "not stored yet"
/// and both insert.
#[tokio::test]
async fn two_concurrent_deliveries_of_one_message_produce_one_of_everything() {
    let Some(fixture) = Fixture::new("inbound_concurrent").await else {
        return;
    };
    let rfc = format!("<concurrent-{}@example.com>", fixture.suffix);

    let first = request(&fixture, &rfc, "Anyone there?").await;
    let second = request(&fixture, &rfc, "Anyone there?").await;
    let one = PostgresPersistence::new(fixture.pool.clone());
    let two = PostgresPersistence::new(fixture.pool.clone());
    let (left, right) = tokio::join!(one.commit_inbound(first), two.commit_inbound(second));

    let outcomes: Vec<InboundCommitOutcome> = vec![left.unwrap(), right.unwrap()];
    assert_eq!(outcomes[0].message_id, outcomes[1].message_id);
    assert_eq!(outcomes[0].task_id, outcomes[1].task_id);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.disposition == CommitDisposition::Created)
            .count(),
        1,
        "exactly one of the two deliveries stored the message"
    );
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(task_count(&fixture).await, 1);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_messages WHERE company_id = $1"
        )
        .await,
        1
    );

    fixture.cleanup().await;
}

/// A commit that cannot finish leaves nothing behind -- including the thread it had already
/// created, which is the row the pre-canonical path committed separately and stranded.
#[tokio::test]
async fn a_commit_that_fails_at_the_task_leaves_no_thread_message_or_mapping() {
    let Some(fixture) = Fixture::new("inbound_rollback").await else {
        return;
    };
    let rfc = format!("<rollback-{}@example.com>", fixture.suffix);

    let mut request = request(&fixture, &rfc, "Anyone there?").await;
    // A task naming a channel this commit has no association for: refused at the last statement
    // group, after the thread, the message and both mappings have been written.
    request.task = Some(InboundTaskRequest {
        task_type: AGENT_DISPATCH.to_string(),
        targets: vec![InboundTaskTarget {
            channel_id: Uuid::new_v4(),
            role: RecipientRole::To,
        }],
    });

    let refused = fixture.persistence.commit_inbound(request).await;
    assert!(refused.is_err(), "a task with no association is refused");

    assert_eq!(message_count(&fixture).await, 0);
    assert_eq!(task_count(&fixture).await, 0);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_messages WHERE company_id = $1"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_threads WHERE company_id = $1"
        )
        .await,
        0
    );
    // The fixture's own thread survives; nothing this commit would have opened does.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM threads WHERE company_id = $1"
        )
        .await,
        1
    );

    fixture.cleanup().await;
}

/// The same rollback, failing at the *first* statement group instead of the last. Between them
/// they cover both sides of every write the commit makes.
#[tokio::test]
async fn a_commit_that_fails_at_the_threads_writes_nothing_either() {
    let Some(fixture) = Fixture::new("inbound_rollback_early").await else {
        return;
    };
    let rfc = format!("<early-{}@example.com>", fixture.suffix);
    let binding_id = fixture.email_binding_of(fixture.channel_id).await;

    let mut request = request(&fixture, &rfc, "Anyone there?").await;
    let mut associations = request.associations.into_inner();
    // A second association naming a thread that belongs to another channel: refused while the
    // threads are still being resolved, before the message exists at all.
    let foreign_channel = fixture.extra_channel("billing").await;
    associations.push(ThreadAssociation {
        channel_id: foreign_channel,
        binding_id: fixture.email_binding_of(foreign_channel).await,
        target: ThreadTarget::Existing(fixture.thread.id),
        role: RecipientRole::Cc,
        step: PipelineStep { index: 1, total: 2 },
        principals: BoundedVec::empty(),
    });
    request.associations = BoundedVec::parse("thread associations", associations).unwrap();

    assert!(fixture.persistence.commit_inbound(request).await.is_err());

    assert_eq!(message_count(&fixture).await, 0);
    assert_eq!(task_count(&fixture).await, 0);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_threads WHERE company_id = $1"
        )
        .await,
        0
    );
    // The thread the first association had already created is gone with the rest.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM threads WHERE company_id = $1"
        )
        .await,
        1
    );
    // And nothing was left half-bound: a retry still finds no conversation on this key.
    assert!(
        fixture
            .persistence
            .thread_for_thread_keys(binding_id, &[ExternalThreadKey::parse(&rfc).unwrap()])
            .await
            .unwrap()
            .is_none()
    );

    fixture.cleanup().await;
}

/// One mail addressed to two channels: one payload, one mapping per interface, one thread each.
#[tokio::test]
async fn a_message_addressed_to_two_channels_is_stored_once_and_mapped_on_each_binding() {
    let Some(fixture) = Fixture::new("inbound_fanout").await else {
        return;
    };
    let second_channel = fixture.extra_channel("billing").await;
    let second_binding = fixture.email_binding_of(second_channel).await;
    let rfc = format!("<fanout-{}@example.com>", fixture.suffix);

    let mut request = request(&fixture, &rfc, "Anyone there?").await;
    let mut associations = request.associations.into_inner();
    associations.push(ThreadAssociation {
        channel_id: second_channel,
        binding_id: second_binding,
        target: ThreadTarget::Create {
            subject: "Quick question".to_string(),
        },
        role: RecipientRole::Cc,
        step: PipelineStep { index: 1, total: 2 },
        principals: sender_principals(),
    });
    request.associations = BoundedVec::parse("thread associations", associations).unwrap();

    let outcome = fixture.persistence.commit_inbound(request).await.unwrap();

    assert_eq!(outcome.thread_ids.len(), 2);
    assert_ne!(outcome.thread_ids[0], outcome.thread_ids[1]);
    // One payload, two conversations, two provider mappings -- the body is stored exactly once.
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM thread_messages WHERE company_id = $1"
        )
        .await,
        2
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_messages WHERE company_id = $1"
        )
        .await,
        2
    );
    for thread_id in &outcome.thread_ids {
        assert!(
            fixture
                .persistence
                .get_thread_message(*thread_id, outcome.message_id)
                .await
                .unwrap()
                .is_some(),
            "each thread holds the one canonical message"
        );
    }

    fixture.cleanup().await;
}

/// A claimed inbound event has no durable inbox until step 10, so the commit refuses one rather
/// than completing it silently. An event that stays claimed is work that never runs again.
#[tokio::test]
async fn work_this_build_has_no_durable_queue_for_is_refused_rather_than_dropped() {
    let Some(fixture) = Fixture::new("inbound_unsupported").await else {
        return;
    };
    let rfc = format!("<unsupported-{}@example.com>", fixture.suffix);

    let mut request = request(&fixture, &rfc, "Anyone there?").await;
    request.claimed_event = Some(crate::transport::ExecutionLease::new(
        crate::entities::transport::InboundEventId::random(),
        crate::transport::WorkerId::random(),
        chrono::Utc::now() + chrono::Duration::minutes(5),
    ));

    assert!(fixture.persistence.commit_inbound(request).await.is_err());
    assert_eq!(message_count(&fixture).await, 0);

    fixture.cleanup().await;
}
