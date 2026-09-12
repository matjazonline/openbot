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
    outreach::OutreachReplyMatch,
    participant::{IdentityClaimMetadata, IdentityProvenance, ThreadPrincipalRole},
    task::NewTask,
    transport::{
        ChannelBindingId, ExternalEventKey, ExternalMessageKey, ExternalThreadKey,
        IdentityNamespace, IdentitySubject, InboundEventId, InboundSource, QualifiedIdentity,
        TransportKind,
    },
    value_objects::MessageId,
};
use crate::task_queue::TaskPersistence;
use crate::transport::{
    AddressedIdentity, AuthenticatedInboundEvent, BoundedVec, CanonicalContent,
    ClaimedInboundEvent, CommitDisposition, ExternalCorrelationStore, InboundCommitOutcome,
    InboundCommitRequest, InboundEnvelope, InboundEventInbox, InboundEventPayload,
    InboundEventQueue, InboundHold, InboundMessageCommitter, InboundOutreachTransition,
    InboundTaskRequest, InboundTaskTarget, IngressDirectives, IngressPolicyFacts, PipelineStep,
    ProtocolExtension, RecipientRole, ReplyDelivery, SafeHeaderFacts, ThreadAssociation,
    ThreadPrincipalIntent, ThreadTarget, WorkerId,
};
use crate::use_cases::participant::{IdentityDirectory, IdentityObservation};

/// The one task type inbound mail produces.
const AGENT_DISPATCH: &str = "email_agent_dispatch";

#[path = "inbound_multi_task_tests.rs"]
mod multi_task_tests;

fn identity(address: &str) -> QualifiedIdentity {
    QualifiedIdentity::new(
        TransportKind::Email,
        IdentityNamespace::parse("email").unwrap(),
        IdentitySubject::parse(address).unwrap(),
    )
}

fn slack_identity(namespace: &str, subject: &str) -> QualifiedIdentity {
    QualifiedIdentity::new(
        TransportKind::Slack,
        IdentityNamespace::parse(namespace).unwrap(),
        IdentitySubject::parse(subject).unwrap(),
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
        tasks: BoundedVec::parse(
            "inbound tasks",
            vec![InboundTaskRequest {
                task_type: AGENT_DISPATCH.to_string(),
                targets: vec![InboundTaskTarget {
                    channel_id: fixture.channel_id,
                    role: RecipientRole::To,
                }],
            }],
        )
        .unwrap(),
        holds: BoundedVec::empty(),
        outreach_transitions: BoundedVec::empty(),
        deliveries: Vec::new(),
        reply_delivery: ReplyDelivery::Send,
    }
}

async fn request_on(
    fixture: &Fixture,
    channel_id: Uuid,
    binding_id: ChannelBindingId,
    message_key: &str,
    thread_key: &str,
    body: &str,
) -> InboundCommitRequest {
    let mut request = request(fixture, message_key, body).await;
    request.envelope.source.binding_id = binding_id;
    request.envelope.source.message_key = ExternalMessageKey::parse(message_key).unwrap();
    request.envelope.source.thread_key = ExternalThreadKey::parse(thread_key).unwrap();
    request.associations = BoundedVec::parse(
        "thread associations",
        vec![ThreadAssociation {
            channel_id,
            binding_id,
            target: ThreadTarget::Create {
                subject: "Quick question".to_string(),
            },
            role: RecipientRole::To,
            step: PipelineStep::only(),
            principals: sender_principals(),
        }],
    )
    .unwrap();
    request.tasks = BoundedVec::parse(
        "inbound tasks",
        vec![InboundTaskRequest {
            task_type: AGENT_DISPATCH.to_string(),
            targets: vec![InboundTaskTarget {
                channel_id,
                role: RecipientRole::To,
            }],
        }],
    )
    .unwrap();
    request
}

async fn count(fixture: &Fixture, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(fixture.company_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap()
}

/// Store one authenticated inbox event for `request` and point the request at it.
///
/// Every inbox test needs the same producer half — the key on the envelope, the same key on the
/// protocol extension, and a stored row — before it can say anything about the transition it is
/// actually testing.
async fn store_event(
    fixture: &Fixture,
    request: &mut InboundCommitRequest,
    key: &str,
) -> InboundEventId {
    let event_key = ExternalEventKey::parse(key).unwrap();
    request.envelope.source.event_key = Some(event_key.clone());
    request.envelope.extension =
        ProtocolExtension::stored_event(request.envelope.source.binding_id, event_key.clone());
    fixture
        .persistence
        .store_authenticated(AuthenticatedInboundEvent {
            transport: TransportKind::Email,
            company_id: fixture.company_id,
            installation_id: None,
            external_event_key: event_key,
            correlation_id: request.envelope.correlation_id,
            payload: InboundEventPayload::parse(br#"{"event":"message"}"#.to_vec()).unwrap(),
            content_type: None,
            safe_header_facts: SafeHeaderFacts::default(),
            received_at: chrono::Utc::now(),
        })
        .await
        .unwrap()
        .event_id()
}

/// Claim a stored event now, whatever backoff a previous attempt left on it.
async fn claim_stored(fixture: &Fixture, event_id: InboundEventId) -> ClaimedInboundEvent {
    sqlx::query(
        "UPDATE inbound_events SET available_at = CURRENT_TIMESTAMP - INTERVAL '1 day' \
         WHERE id = $1",
    )
    .bind(event_id.as_uuid())
    .execute(fixture.persistence.pool())
    .await
    .unwrap();
    fixture
        .persistence
        .claim_inbound_events(WorkerId::random(), std::time::Duration::from_secs(120), 1)
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("the stored event is claimable")
}

/// Age a held lease past its expiry, which is what a crashed worker leaves behind.
async fn expire_lease(fixture: &Fixture, event_id: InboundEventId) {
    sqlx::query(
        r#"UPDATE inbound_events
              SET locked_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes',
                  lock_expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute'
            WHERE id = $1"#,
    )
    .bind(event_id.as_uuid())
    .execute(fixture.persistence.pool())
    .await
    .unwrap();
}

async fn event_status(fixture: &Fixture, event_id: InboundEventId) -> String {
    sqlx::query_scalar("SELECT status FROM inbound_events WHERE id = $1")
        .bind(event_id.as_uuid())
        .fetch_one(fixture.persistence.pool())
        .await
        .unwrap()
}

async fn event_attempts(fixture: &Fixture, event_id: InboundEventId) -> i32 {
    sqlx::query_scalar("SELECT attempt_count FROM inbound_events WHERE id = $1")
        .bind(event_id.as_uuid())
        .fetch_one(fixture.persistence.pool())
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

async fn durable_counts(fixture: &Fixture) -> [i64; 11] {
    [
        count(
            fixture,
            "SELECT count(*) FROM principals WHERE company_id = $1",
        )
        .await,
        count(
            fixture,
            "SELECT count(*) FROM participant_identities WHERE company_id = $1",
        )
        .await,
        count(
            fixture,
            "SELECT count(*) FROM threads WHERE company_id = $1",
        )
        .await,
        count(
            fixture,
            "SELECT count(*) FROM thread_principals WHERE company_id = $1",
        )
        .await,
        message_count(fixture).await,
        count(
            fixture,
            "SELECT count(*) FROM message_participants WHERE company_id = $1",
        )
        .await,
        count(
            fixture,
            "SELECT count(*) FROM thread_messages WHERE company_id = $1",
        )
        .await,
        count(
            fixture,
            "SELECT count(*) FROM external_threads WHERE company_id = $1",
        )
        .await,
        count(
            fixture,
            "SELECT count(*) FROM external_messages WHERE company_id = $1",
        )
        .await,
        task_count(fixture).await,
        count(
            fixture,
            "SELECT count(*) FROM message_deliveries WHERE company_id = $1",
        )
        .await,
    ]
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
    assert_eq!(outcome.task_ids.len(), 1);
    let task_id = outcome.task_ids[0];

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
    assert_eq!(second.task_ids, first.task_ids);
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

    let first = multi_task_tests::two_pipeline_request(&fixture, &rfc).await;
    let second = first.clone();
    let one = PostgresPersistence::new(fixture.pool.clone());
    let two = PostgresPersistence::new(fixture.pool.clone());
    let (left, right) = tokio::join!(one.commit_inbound(first), two.commit_inbound(second));

    let outcomes: Vec<InboundCommitOutcome> = vec![left.unwrap(), right.unwrap()];
    assert_eq!(outcomes[0].message_id, outcomes[1].message_id);
    assert_eq!(outcomes[0].task_ids, outcomes[1].task_ids);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.disposition == CommitDisposition::Created)
            .count(),
        1,
        "exactly one of the two deliveries stored the message"
    );
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(task_count(&fixture).await, 2);
    assert_eq!(outcomes[0].task_ids.len(), 2);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_messages WHERE company_id = $1"
        )
        .await,
        2
    );

    fixture.cleanup().await;
}

/// Different posts in one newly-seen provider conversation serialize on the thread key, not only
/// their distinct message keys. Both therefore join the one conversation whichever claimant wins.
#[tokio::test]
async fn concurrent_different_messages_sharing_a_new_thread_key_open_one_thread() {
    let Some(fixture) = Fixture::new("inbound_thread_race").await else {
        return;
    };
    let binding = fixture.email_binding_of(fixture.channel_id).await;
    let root = format!("<thread-{}@example.com>", fixture.suffix);
    let first_key = format!("<first-{}@example.com>", fixture.suffix);
    let second_key = format!("<second-{}@example.com>", fixture.suffix);
    let first = request_on(
        &fixture,
        fixture.channel_id,
        binding,
        &first_key,
        &root,
        "First",
    )
    .await;
    let second = request_on(
        &fixture,
        fixture.channel_id,
        binding,
        &second_key,
        &root,
        "Second",
    )
    .await;

    let one = PostgresPersistence::new(fixture.pool.clone());
    let two = PostgresPersistence::new(fixture.pool.clone());
    let (left, right) = tokio::join!(one.commit_inbound(first), two.commit_inbound(second));
    let left = left.unwrap();
    let right = right.unwrap();

    assert_eq!(left.thread_ids, right.thread_ids);
    assert_ne!(left.message_id, right.message_id);
    assert_eq!(message_count(&fixture).await, 2);
    assert_eq!(task_count(&fixture).await, 2);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_threads WHERE company_id = $1"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM thread_messages WHERE company_id = $1"
        )
        .await,
        2
    );

    fixture.cleanup().await;
}

/// A reply can be the first event observed for a provider conversation. A later root carrying the
/// same thread key must join the conversation the reply opened rather than create a rival thread.
#[tokio::test]
async fn a_reply_observed_before_its_root_wins_the_provider_thread_mapping() {
    let Some(fixture) = Fixture::new("inbound_reply_before_root").await else {
        return;
    };
    let binding = fixture.email_binding_of(fixture.channel_id).await;
    let root_key = format!("<root-{}@example.com>", fixture.suffix);
    let reply_key = format!("<reply-{}@example.com>", fixture.suffix);

    let reply = fixture
        .persistence
        .commit_inbound(
            request_on(
                &fixture,
                fixture.channel_id,
                binding,
                &reply_key,
                &root_key,
                "Reply arrived first",
            )
            .await,
        )
        .await
        .unwrap();
    let root = fixture
        .persistence
        .commit_inbound(
            request_on(
                &fixture,
                fixture.channel_id,
                binding,
                &root_key,
                &root_key,
                "Root arrived later",
            )
            .await,
        )
        .await
        .unwrap();

    assert_eq!(reply.thread_ids, root.thread_ids);
    assert_ne!(reply.message_id, root.message_id);
    assert_eq!(message_count(&fixture).await, 2);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_threads WHERE company_id = $1"
        )
        .await,
        1
    );

    fixture.cleanup().await;
}

/// A provider key already mapped on two interfaces must not be treated as a redelivery when those
/// interfaces disagree about which canonical message it names.
#[tokio::test]
async fn inconsistent_multi_binding_message_mappings_are_a_typed_collision() {
    let Some(fixture) = Fixture::new("inbound_binding_collision").await else {
        return;
    };
    let second_channel = fixture.extra_channel("billing").await;
    let first_binding = fixture.email_binding_of(fixture.channel_id).await;
    let second_binding = fixture.email_binding_of(second_channel).await;
    let key = format!("<shared-{}@example.com>", fixture.suffix);

    fixture
        .persistence
        .commit_inbound(
            request_on(
                &fixture,
                fixture.channel_id,
                first_binding,
                &key,
                &format!("<first-thread-{}>", fixture.suffix),
                "Same provider body",
            )
            .await,
        )
        .await
        .unwrap();
    fixture
        .persistence
        .commit_inbound(
            request_on(
                &fixture,
                second_channel,
                second_binding,
                &key,
                &format!("<second-thread-{}>", fixture.suffix),
                "Same provider body",
            )
            .await,
        )
        .await
        .unwrap();

    let mut combined = request_on(
        &fixture,
        fixture.channel_id,
        first_binding,
        &key,
        &format!("<combined-thread-{}>", fixture.suffix),
        "Same provider body",
    )
    .await;
    combined.associations = BoundedVec::parse(
        "thread associations",
        vec![
            combined.associations[0].clone(),
            ThreadAssociation {
                channel_id: second_channel,
                binding_id: second_binding,
                target: ThreadTarget::Create {
                    subject: "Quick question".into(),
                },
                role: RecipientRole::Cc,
                step: PipelineStep { index: 1, total: 2 },
                principals: sender_principals(),
            },
        ],
    )
    .unwrap();

    let error = fixture
        .persistence
        .commit_inbound(combined)
        .await
        .expect_err("two canonical mappings cannot be one redelivery");
    assert!(error.to_string().contains("different content"), "{error}");
    assert_eq!(message_count(&fixture).await, 2);
    assert_eq!(task_count(&fixture).await, 2);

    fixture.cleanup().await;
}

/// A non-email adapter supplies the same participant intent without inventing an email address.
#[tokio::test]
async fn slack_participant_intents_persist_with_explicit_roles_and_no_email_projection() {
    let Some(fixture) = Fixture::new("inbound_slack_principals").await else {
        return;
    };
    let binding = fixture.slack_binding("C123").await;
    let namespace = format!("T{}", fixture.suffix);
    let author = slack_identity(&namespace, "U123");
    let event_key = ExternalEventKey::parse(format!("Ev{}", fixture.suffix)).unwrap();
    let message_key = format!("1712345.{}", &fixture.suffix[..8]);
    let thread_key = format!("1712345.{}", &fixture.suffix[8..16]);
    let mut request = request_on(
        &fixture,
        fixture.channel_id,
        binding,
        &message_key,
        &thread_key,
        "Slack body",
    )
    .await;
    request.envelope.source.event_key = Some(event_key.clone());
    request.envelope.author = author.clone();
    request.envelope.addressed = BoundedVec::empty();
    request.envelope.policy = IngressPolicyFacts::InstalledConversation;
    request.envelope.extension = ProtocolExtension::stored_event(binding, event_key);
    request.associations = BoundedVec::parse(
        "thread associations",
        vec![ThreadAssociation {
            channel_id: fixture.channel_id,
            binding_id: binding,
            target: ThreadTarget::Create {
                subject: "Slack thread".into(),
            },
            role: RecipientRole::To,
            step: PipelineStep::only(),
            principals: BoundedVec::parse(
                "thread principals",
                vec![
                    ThreadPrincipalIntent::new(author.clone(), ThreadPrincipalRole::Author),
                    ThreadPrincipalIntent::new(author.clone(), ThreadPrincipalRole::Participant),
                ],
            )
            .unwrap(),
        }],
    )
    .unwrap();

    let outcome = fixture.persistence.commit_inbound(request).await.unwrap();
    let roles: Vec<String> = sqlx::query_scalar(
        r#"SELECT thread_principal.role
           FROM thread_principals AS thread_principal
           JOIN participant_identities AS identity
             ON (identity.company_id, identity.principal_id) =
                (thread_principal.company_id, thread_principal.principal_id)
           WHERE thread_principal.company_id = $1 AND thread_principal.thread_id = $2
             AND identity.transport = 'slack' AND identity.namespace = $3
             AND identity.subject = 'U123'
           ORDER BY thread_principal.role"#,
    )
    .bind(fixture.company_id)
    .bind(outcome.thread_ids[0])
    .bind(&namespace)
    .fetch_all(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(roles, vec!["author", "participant"]);
    let email_projection: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM participant_identities
           WHERE company_id = $1 AND principal_id IN (
               SELECT principal_id FROM thread_principals
               WHERE company_id = $1 AND thread_id = $2
           ) AND transport = 'email'"#,
    )
    .bind(fixture.company_id)
    .bind(outcome.thread_ids[0])
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(email_projection, 0);

    fixture.cleanup().await;
}

/// Stored events have no raw-email representation, so their canonical body participates in
/// collision detection.
#[tokio::test]
async fn a_stored_event_key_reused_with_a_different_body_is_a_collision() {
    let Some(fixture) = Fixture::new("inbound_event_hash").await else {
        return;
    };
    let binding = fixture.slack_binding("C456").await;
    let event_key = ExternalEventKey::parse(format!("Ev{}", fixture.suffix)).unwrap();
    let message_key = format!("event-message-{}", fixture.suffix);
    let thread_key = format!("event-thread-{}", fixture.suffix);
    let mut first = request_on(
        &fixture,
        fixture.channel_id,
        binding,
        &message_key,
        &thread_key,
        "Original event body",
    )
    .await;
    first.envelope.extension = ProtocolExtension::stored_event(binding, event_key.clone());
    first.envelope.policy = IngressPolicyFacts::InstalledConversation;
    let mut changed = first.clone();
    changed.envelope.content =
        CanonicalContent::parse("Quick question", "Edited event body").unwrap();

    fixture.persistence.commit_inbound(first).await.unwrap();
    let error = fixture
        .persistence
        .commit_inbound(changed)
        .await
        .expect_err("changed event content is not a redelivery");
    assert!(error.to_string().contains("different content"), "{error}");
    assert_eq!(message_count(&fixture).await, 1);

    fixture.cleanup().await;
}

/// The database independently enforces that an authored handle belongs to the stated principal.
#[tokio::test]
async fn authored_identity_cannot_belong_to_a_different_author_principal() {
    let Some(fixture) = Fixture::new("message_author_identity_fk").await else {
        return;
    };
    let first = fixture
        .persistence
        .resolve_or_create_external_identity(
            fixture.company_id,
            IdentityObservation {
                identity: identity("first@example.com"),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::TransportIngress,
            },
        )
        .await
        .unwrap();
    let second = fixture
        .persistence
        .resolve_or_create_external_identity(
            fixture.company_id,
            IdentityObservation {
                identity: identity("second@example.com"),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::TransportIngress,
            },
        )
        .await
        .unwrap();

    let error = sqlx::query(
        r#"INSERT INTO messages (
                id, company_id, author_principal_id, authored_identity_id, subject,
                clean_text_body, direction, role, correlation_id, content_hash
           ) VALUES ($1, $2, $3, $4, '', '', 'inbound', 'human', $5, $6)"#,
    )
    .bind(Uuid::new_v4())
    .bind(fixture.company_id)
    .bind(first.principal.id.as_uuid())
    .bind(second.identity.id.as_uuid())
    .bind(Uuid::new_v4())
    .bind(vec![0_u8; 32])
    .execute(&fixture.pool)
    .await
    .expect_err("a handle owned by another principal must be rejected");
    assert!(
        error
            .as_database_error()
            .and_then(|error| error.constraint())
            .is_some_and(|name| name == "messages_authored_identity_author_fk"),
        "{error}"
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
    request.tasks = BoundedVec::parse(
        "inbound tasks",
        vec![InboundTaskRequest {
            task_type: AGENT_DISPATCH.to_string(),
            targets: vec![InboundTaskTarget {
                channel_id: Uuid::new_v4(),
                role: RecipientRole::To,
            }],
        }],
    )
    .unwrap();

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

/// Outreach association is not follow-up work: if it cannot be recorded, every message-side row
/// written before it rolls back, and the unchanged request succeeds once the bad transition is
/// removed.
#[tokio::test]
async fn an_outreach_transition_failure_rolls_back_every_inbound_row_and_retry_succeeds() {
    let Some(fixture) = Fixture::new("inbound_outreach_rollback").await else {
        return;
    };
    let key = format!("<outreach-rollback-{}@example.com>", fixture.suffix);
    let before = durable_counts(&fixture).await;
    let mut request = request(&fixture, &key, "Reply").await;
    request.outreach_transitions = BoundedVec::parse(
        "outreach transitions",
        vec![InboundOutreachTransition {
            channel_id: fixture.channel_id,
            matched: OutreachReplyMatch {
                outreach_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                target_id: Uuid::new_v4(),
                target_email: "vendor@example.com".into(),
            },
        }],
    )
    .unwrap();
    let mut retry = request.clone();
    retry.outreach_transitions = BoundedVec::empty();

    assert!(fixture.persistence.commit_inbound(request).await.is_err());
    assert_eq!(durable_counts(&fixture).await, before);

    let outcome = fixture.persistence.commit_inbound(retry).await.unwrap();
    assert_eq!(outcome.disposition, CommitDisposition::Created);
    assert_eq!(message_count(&fixture).await, before[4] + 1);

    fixture.cleanup().await;
}

/// A response association and the waiting-task transition become visible in the same commit as
/// the inbound message. There is no post-commit window in which one exists without the other.
#[tokio::test]
async fn an_outreach_reply_association_and_task_wakeup_commit_with_the_message() {
    let Some(fixture) = Fixture::new("inbound_outreach_atomic").await else {
        return;
    };
    let task = fixture
        .persistence
        .enqueue_task(NewTask::starting_new_chain(
            fixture.company_id,
            fixture.channel_id,
            Some(fixture.thread.id),
            "outreach-test",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    sqlx::query(
        r#"UPDATE background_tasks
           SET status = 'waiting_for_third_party_reply',
               wait_expires_at = CURRENT_TIMESTAMP + interval '1 day'
           WHERE id = $1"#,
    )
    .bind(task.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let outreach_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO task_outreaches (
                id, company_id, task_id, status, required_threshold_percent, expires_at,
                outreach_key, subject, body
           ) VALUES ($1, $2, $3, 'waiting', 100, CURRENT_TIMESTAMP + interval '1 day',
                     'atomic-reply', 'Question', 'Please reply')"#,
    )
    .bind(outreach_id)
    .bind(fixture.company_id)
    .bind(task.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE background_tasks SET awaited_outreach_id = $2 WHERE id = $1")
        .bind(task.id)
        .bind(outreach_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    sqlx::query(
        r#"INSERT INTO task_outreach_targets (
               outreach_id, company_id, email, target_kind,
               external_transport, external_namespace, external_subject
           ) VALUES ($1, $2, $3, 'external', 'email', 'email', $3)"#,
    )
    .bind(outreach_id)
    .bind(fixture.company_id)
    .bind("vendor@example.com")
    .execute(&fixture.pool)
    .await
    .unwrap();
    let target_id: Uuid =
        sqlx::query_scalar("SELECT id FROM task_outreach_targets WHERE outreach_id = $1")
            .bind(outreach_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();

    let key = format!("<outreach-response-{}@example.com>", fixture.suffix);
    let binding = fixture.email_binding_of(fixture.channel_id).await;
    let mut request = request_on(
        &fixture,
        fixture.channel_id,
        binding,
        &key,
        &key,
        "The answer",
    )
    .await;
    request.associations = BoundedVec::parse(
        "thread associations",
        vec![ThreadAssociation {
            channel_id: fixture.channel_id,
            binding_id: binding,
            target: ThreadTarget::Existing(fixture.thread.id),
            role: RecipientRole::To,
            step: PipelineStep::only(),
            principals: sender_principals(),
        }],
    )
    .unwrap();
    request.tasks = BoundedVec::empty();
    request.outreach_transitions = BoundedVec::parse(
        "outreach transitions",
        vec![InboundOutreachTransition {
            channel_id: fixture.channel_id,
            matched: OutreachReplyMatch {
                outreach_id,
                task_id: task.id,
                target_id,
                target_email: "vendor@example.com".into(),
            },
        }],
    )
    .unwrap();

    let outcome = fixture.persistence.commit_inbound(request).await.unwrap();
    let association_id: Uuid = sqlx::query_scalar(
        r#"SELECT id FROM thread_messages
           WHERE company_id = $1 AND channel_id = $2 AND message_id = $3"#,
    )
    .bind(fixture.company_id)
    .bind(fixture.channel_id)
    .bind(outcome.message_id.as_uuid())
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let (responded, response_id): (bool, Option<Uuid>) = sqlx::query_as(
        r#"SELECT responded_at IS NOT NULL, response_association_id
           FROM task_outreach_targets WHERE outreach_id = $1 AND email = $2"#,
    )
    .bind(outreach_id)
    .bind("vendor@example.com")
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert!(responded);
    assert_eq!(response_id, Some(association_id));
    let (outreach_status, task_status): (String, String) = sqlx::query_as(
        r#"SELECT outreach.status, task.status
           FROM task_outreaches AS outreach
           JOIN background_tasks AS task ON task.id = outreach.task_id
           WHERE outreach.id = $1"#,
    )
    .bind(outreach_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(outreach_status, "threshold_met");
    assert_eq!(task_status, "pending");

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

/// Every association's binding is checked in one statement, and a refusal reads as it did when each
/// was checked on its own: the first bad pair in the order stated, named exactly.
#[tokio::test]
async fn an_association_through_another_channels_binding_names_the_first_bad_pair() {
    let Some(fixture) = Fixture::new("inbound_unbound_association").await else {
        return;
    };
    let billing = fixture.extra_channel("billing").await;
    let sales = fixture.extra_channel("sales").await;
    let primary_binding = fixture.email_binding_of(fixture.channel_id).await;
    let billing_binding = fixture.email_binding_of(billing).await;
    let rfc = format!("<unbound-{}@example.com>", fixture.suffix);
    // A real, active binding -- of some other channel than the one it is paired with.
    let misbound =
        |channel_id: Uuid, binding_id: ChannelBindingId, index: usize| ThreadAssociation {
            channel_id,
            binding_id,
            target: ThreadTarget::Create {
                subject: "Quick question".to_string(),
            },
            role: RecipientRole::Cc,
            step: PipelineStep { index, total: 3 },
            principals: sender_principals(),
        };

    let mut request = request(&fixture, &rfc, "Anyone there?").await;
    let mut associations = request.associations.into_inner();
    associations.push(misbound(billing, primary_binding, 1));
    associations.push(misbound(sales, billing_binding, 2));
    request.associations = BoundedVec::parse("thread associations", associations).unwrap();

    let error = fixture
        .persistence
        .commit_inbound(request)
        .await
        .expect_err("a binding cannot carry another channel's thread");
    let expected = format!("Active binding {primary_binding} does not belong to channel {billing}");
    assert!(
        matches!(&error, AppError::NotFound(message) if *message == expected),
        "{error:?}"
    );
    assert_eq!(message_count(&fixture).await, 0);

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

#[tokio::test]
async fn canonical_rows_and_inbox_completion_commit_together() {
    let Some(fixture) = Fixture::isolated("inbound_unsupported").await else {
        return;
    };
    let rfc = format!("<event-{}@example.com>", fixture.suffix);
    let mut request = request(&fixture, &rfc, "Anyone there?").await;
    let stored = store_event(&fixture, &mut request, &format!("Ev{}", fixture.suffix)).await;
    request.claimed_event = Some(claim_stored(&fixture, stored).await.lease);

    assert!(fixture.persistence.commit_inbound(request).await.is_ok());
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(event_status(&fixture, stored).await, "completed");

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_crash_at_any_inbox_phase_recovers_without_a_duplicate_canonical_message() {
    let Some(fixture) = Fixture::isolated("inbound_crash").await else {
        return;
    };
    let rfc = format!("<crash-{}@example.com>", fixture.suffix);
    let mut request = request(&fixture, &rfc, "Survives every crash").await;
    let stored = store_event(
        &fixture,
        &mut request,
        &format!("EvCrash{}", fixture.suffix),
    )
    .await;

    // A crash between acknowledgement and the first claim: the acknowledged event is simply
    // still waiting, and holds no lease for the reaper to charge an attempt for.
    assert_eq!(event_status(&fixture, stored).await, "pending");
    assert_eq!(
        fixture
            .persistence
            .reap_expired_inbound_events()
            .await
            .unwrap()
            .leases_expired,
        0
    );
    assert_eq!(event_attempts(&fixture, stored).await, 0);

    // A crash immediately after claiming, one partway through decode, and one after decode but
    // before the canonical commit are the same durable state: a lease nobody will renew, and no
    // canonical row anywhere. Each recovers by expiry, and each costs exactly one attempt.
    for expected_attempt in 1..=3 {
        let claimed = claim_stored(&fixture, stored).await;
        assert_eq!(event_status(&fixture, stored).await, "processing");
        expire_lease(&fixture, stored).await;
        drop(claimed);

        assert!(
            fixture
                .persistence
                .reap_expired_inbound_events()
                .await
                .unwrap()
                .leases_expired
                >= 1
        );
        assert_eq!(event_status(&fixture, stored).await, "retryable");
        assert_eq!(event_attempts(&fixture, stored).await, expected_attempt);
        assert_eq!(message_count(&fixture).await, 0);
    }

    // The replacement execution that finally commits produces one message, not four.
    request.claimed_event = Some(claim_stored(&fixture, stored).await.lease);
    let correlation_id = request.envelope.correlation_id;
    assert!(fixture.persistence.commit_inbound(request).await.is_ok());
    assert_eq!(event_status(&fixture, stored).await, "completed");
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(task_count(&fixture).await, 1);

    // A crash after the commit cannot be recovered *into* a second message. The completed row is
    // outside every claimable and reapable predicate, and a provider redelivery of the same key
    // deduplicates onto it rather than opening a fresh attempt.
    assert_eq!(
        fixture
            .persistence
            .reap_expired_inbound_events()
            .await
            .unwrap()
            .leases_expired,
        0
    );
    let redelivered = fixture
        .persistence
        .store_authenticated(AuthenticatedInboundEvent {
            transport: TransportKind::Email,
            company_id: fixture.company_id,
            installation_id: None,
            external_event_key: ExternalEventKey::parse(format!("EvCrash{}", fixture.suffix))
                .unwrap(),
            correlation_id,
            payload: InboundEventPayload::parse(br#"{"event":"message"}"#.to_vec()).unwrap(),
            content_type: None,
            safe_header_facts: SafeHeaderFacts::default(),
            received_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    assert!(!redelivered.was_stored());
    assert_eq!(redelivered.event_id(), stored);
    assert_eq!(event_status(&fixture, stored).await, "completed");
    assert_eq!(message_count(&fixture).await, 1);

    fixture.cleanup().await;
}

#[tokio::test]
async fn lease_loss_at_final_commit_rolls_back_every_canonical_effect() {
    let Some(fixture) = Fixture::isolated("inbound_lost_fence").await else {
        return;
    };
    let rfc = format!("<lost-{}@example.com>", fixture.suffix);
    let mut request = request(&fixture, &rfc, "Must roll back").await;
    let stored = store_event(&fixture, &mut request, &format!("EvLost{}", fixture.suffix)).await;
    request.claimed_event = Some(claim_stored(&fixture, stored).await.lease);
    expire_lease(&fixture, stored).await;

    assert!(fixture.persistence.commit_inbound(request).await.is_err());
    assert_eq!(message_count(&fixture).await, 0);
    // The row keeps the lapsed lease rather than settling itself: the reaper owns that decision,
    // and it is the only place an attempt is charged for a lease nobody renewed.
    assert_eq!(event_status(&fixture, stored).await, "processing");

    fixture.cleanup().await;
}

// ---------------------------------------------------------------------------------------------
// The hold gate: an eligible outside reply is filed as the customer message it is, and opens a
// `thread_handoffs` generation, with no `background_tasks` row for that channel.
//
// Every assertion below is scoped to the fixture's own company. The suite shares one database and
// runs in parallel, so a whole-table count would be a coin toss.
// ---------------------------------------------------------------------------------------------

/// A commit that holds the fixture's channel instead of running its agent.
async fn held_request(fixture: &Fixture, rfc: &str, body: &str) -> InboundCommitRequest {
    let mut request = request(fixture, rfc, body).await;
    hold_the_only_channel(&mut request, fixture.channel_id);
    request
}

/// Drop the task and state the hold, exactly as `CommitPlan::build` would for a held channel.
fn hold_the_only_channel(request: &mut InboundCommitRequest, channel_id: Uuid) {
    request.tasks = BoundedVec::empty();
    request.holds = BoundedVec::parse(
        "thread handoffs",
        vec![InboundHold {
            channel_id,
            handoff_id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }],
    )
    .unwrap();
}

/// The one handoff of the fixture's company, by the key that makes it unique.
async fn handoff_row(
    fixture: &Fixture,
) -> (Uuid, Uuid, String, i64, Option<Uuid>, String, bool, Uuid) {
    sqlx::query_as(
        r#"SELECT id, generation, state, version, responsible_principal_id, business_priority,
                  (closed_at IS NOT NULL), source_message_id
           FROM thread_handoffs WHERE company_id = $1"#,
    )
    .bind(fixture.company_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap()
}

async fn handoff_count(fixture: &Fixture) -> i64 {
    count(
        fixture,
        "SELECT count(*) FROM thread_handoffs WHERE company_id = $1",
    )
    .await
}

/// Every event of this company's handoffs, oldest first, as the fields the opening write sets.
async fn handoff_events(
    fixture: &Fixture,
) -> Vec<(String, String, Option<Uuid>, i64, i64, Uuid, Uuid)> {
    sqlx::query_as(
        r#"SELECT operation, actor_kind, actor_principal_id, from_version, to_version,
                  command_id, generation
           FROM thread_handoff_events WHERE company_id = $1
           ORDER BY to_version, occurred_at"#,
    )
    .bind(fixture.company_id)
    .fetch_all(&fixture.pool)
    .await
    .unwrap()
}

/// Case 8 and 9: the first generation, and no run for it.
#[tokio::test]
async fn a_held_reply_opens_one_generation_and_creates_no_task() {
    let Some(fixture) = Fixture::new("inbound_hold_first").await else {
        return;
    };
    let rfc = format!("<hold-first-{}@example.com>", fixture.suffix);
    let request = held_request(&fixture, &rfc, "Any news on the invoice?").await;

    let outcome = fixture.persistence.commit_inbound(request).await.unwrap();

    assert_eq!(outcome.disposition, CommitDisposition::Created);
    assert!(outcome.task_ids.is_empty(), "a held reply runs no agent");
    assert_eq!(outcome.handoff_ids.len(), 1);
    assert_eq!(task_count(&fixture).await, 0);
    assert_eq!(message_count(&fixture).await, 1);
    // Everything else about a held message is exactly what an unheld one produces: its thread
    // association, and the provider mapping a redelivery is recognised by.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM thread_messages WHERE company_id = $1"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_messages WHERE company_id = $1"
        )
        .await,
        1
    );

    let (id, _, state, version, responsible, priority, closed, source) =
        handoff_row(&fixture).await;
    assert_eq!(id, outcome.handoff_ids[0]);
    assert_eq!(state, "needs_instruction");
    assert_eq!(version, 1);
    assert_eq!(responsible, None, "a fresh handoff is the channel team's");
    assert_eq!(priority, "normal");
    assert!(!closed);
    assert_eq!(source, outcome.message_id.as_uuid());

    let events = handoff_events(&fixture).await;
    assert_eq!(events.len(), 1);
    let (operation, actor_kind, actor, from_version, to_version, command_id, _) = &events[0];
    assert_eq!(operation, "opened");
    assert_eq!(actor_kind, "system");
    assert_eq!(*actor, None, "an arriving message is not a person acting");
    assert_eq!((*from_version, *to_version), (0, 1));
    assert_eq!(
        *command_id,
        outcome.message_id.as_uuid(),
        "the canonical message id is what makes the opening write idempotent"
    );

    // The held copy is an ordinary customer message: same audience, same entry kind.
    let (audience, entry_kind): (String, String) = sqlx::query_as(
        r#"SELECT message.audience, association.entry_kind
           FROM messages AS message
           JOIN thread_messages AS association ON association.message_id = message.id
           WHERE message.company_id = $1"#,
    )
    .bind(fixture.company_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(audience, "external_conversation");
    assert_eq!(entry_kind, "conversation");

    fixture.cleanup().await;
}

/// Case 10: a second reply replaces the generation rather than opening a second handoff.
#[tokio::test]
async fn a_second_held_reply_replaces_the_generation_in_place() {
    let Some(fixture) = Fixture::new("inbound_hold_second").await else {
        return;
    };
    let first_rfc = format!("<hold-a-{}@example.com>", fixture.suffix);
    let first = fixture
        .persistence
        .commit_inbound(held_request(&fixture, &first_rfc, "First").await)
        .await
        .unwrap();
    let (_, first_generation, _, _, _, _, _, _) = handoff_row(&fixture).await;

    let second_rfc = format!("<hold-b-{}@example.com>", fixture.suffix);
    let mut second = held_request(&fixture, &second_rfc, "Second").await;
    second.envelope.source.thread_key = ExternalThreadKey::parse(&first_rfc).unwrap();
    let second = fixture.persistence.commit_inbound(second).await.unwrap();

    assert_eq!(handoff_count(&fixture).await, 1, "one handoff per thread");
    assert_eq!(
        second.handoff_ids, first.handoff_ids,
        "the same row moved on"
    );
    let (_, generation, state, version, _, _, closed, source) = handoff_row(&fixture).await;
    assert_ne!(generation, first_generation);
    assert_eq!(state, "needs_instruction");
    assert_eq!(version, 2);
    assert!(!closed);
    assert_eq!(source, second.message_id.as_uuid());

    let events = handoff_events(&fixture).await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].0, "regenerated");
    assert_eq!((events[1].3, events[1].4), (1, 2));
    assert_eq!(events[1].6, generation);

    fixture.cleanup().await;
}

/// Case 11: responsibility and attributes are about who is looking after this conversation. A new
/// customer message is not a reason to un-assign it.
#[tokio::test]
async fn a_replacement_keeps_the_responsibility_and_the_attributes() {
    let Some(fixture) = Fixture::new("inbound_hold_keep").await else {
        return;
    };
    let first_rfc = format!("<hold-keep-a-{}@example.com>", fixture.suffix);
    fixture
        .persistence
        .commit_inbound(held_request(&fixture, &first_rfc, "First").await)
        .await
        .unwrap();

    let claimant = fixture
        .persistence
        .resolve_or_create_external_identity(
            fixture.company_id,
            IdentityObservation {
                identity: identity("bo@acme.example"),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::TransportIngress,
            },
        )
        .await
        .unwrap()
        .principal
        .id;
    let due = chrono::Utc::now() + chrono::Duration::hours(4);
    sqlx::query(
        r#"UPDATE thread_handoffs
           SET responsible_principal_id = $2, business_priority = 'high', business_due_at = $3
           WHERE company_id = $1"#,
    )
    .bind(fixture.company_id)
    .bind(claimant.as_uuid())
    .bind(due)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let (_, first_generation, _, claimed_version, _, _, _, _) = handoff_row(&fixture).await;

    let second_rfc = format!("<hold-keep-b-{}@example.com>", fixture.suffix);
    let mut second = held_request(&fixture, &second_rfc, "Second").await;
    second.envelope.source.thread_key = ExternalThreadKey::parse(&first_rfc).unwrap();
    fixture.persistence.commit_inbound(second).await.unwrap();

    let (_, generation, state, version, responsible, priority, _, _) = handoff_row(&fixture).await;
    assert_ne!(generation, first_generation, "the generation moved");
    assert_eq!(state, "needs_instruction");
    assert_eq!(version, claimed_version + 1);
    assert_eq!(
        responsible,
        Some(claimant.as_uuid()),
        "a thread Bo claimed stays Bo's"
    );
    assert_eq!(priority, "high");
    let kept_due: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT business_due_at FROM thread_handoffs WHERE company_id = $1")
            .bind(fixture.company_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert!(kept_due.is_some());

    fixture.cleanup().await;
}

/// Case 12: the state returns to `needs_instruction` from anywhere, and `closed_at` is cleared —
/// which the closure check would otherwise refuse outright.
#[tokio::test]
async fn a_replacement_reopens_a_drafting_or_resolved_handoff() {
    let Some(fixture) = Fixture::new("inbound_hold_reopen").await else {
        return;
    };
    let root = format!("<hold-reopen-{}@example.com>", fixture.suffix);
    fixture
        .persistence
        .commit_inbound(held_request(&fixture, &root, "First").await)
        .await
        .unwrap();

    for (index, (state, closed)) in [("drafting", false), ("resolved", true)].iter().enumerate() {
        sqlx::query("UPDATE thread_handoffs SET state = $2, closed_at = $3 WHERE company_id = $1")
            .bind(fixture.company_id)
            .bind(state)
            .bind(closed.then(chrono::Utc::now))
            .execute(&fixture.pool)
            .await
            .expect("the fixture may put the row in any legal state");

        let rfc = format!("<hold-reopen-{index}-{}@example.com>", fixture.suffix);
        let mut next = held_request(&fixture, &rfc, "Another reply").await;
        next.envelope.source.thread_key = ExternalThreadKey::parse(&root).unwrap();
        fixture.persistence.commit_inbound(next).await.unwrap();

        let (_, _, reopened, _, _, _, still_closed, _) = handoff_row(&fixture).await;
        assert_eq!(reopened, "needs_instruction", "replaced a {state} handoff");
        assert!(
            !still_closed,
            "the closure check refuses an open row with a closed_at"
        );
    }

    fixture.cleanup().await;
}

/// Case 13: the same inbound message delivered twice is one message, one handoff, one generation,
/// one event and no task. `recognise_redelivery` returns before any of this is reached.
#[tokio::test]
async fn a_redelivered_held_message_opens_no_second_generation() {
    let Some(fixture) = Fixture::new("inbound_hold_redelivery").await else {
        return;
    };
    let rfc = format!("<hold-dup-{}@example.com>", fixture.suffix);
    let first = fixture
        .persistence
        .commit_inbound(held_request(&fixture, &rfc, "Same message").await)
        .await
        .unwrap();
    let (_, generation, _, _, _, _, _, _) = handoff_row(&fixture).await;

    let again = fixture
        .persistence
        .commit_inbound(held_request(&fixture, &rfc, "Same message").await)
        .await
        .unwrap();

    assert_eq!(again.disposition, CommitDisposition::Duplicate);
    assert_eq!(again.message_id, first.message_id);
    assert!(again.handoff_ids.is_empty(), "a redelivery opens nothing");
    assert!(again.task_ids.is_empty());
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(handoff_count(&fixture).await, 1);
    assert_eq!(task_count(&fixture).await, 0);
    assert_eq!(handoff_events(&fixture).await.len(), 1);
    let (_, still, _, version, _, _, _, _) = handoff_row(&fixture).await;
    assert_eq!(still, generation);
    assert_eq!(version, 1);

    fixture.cleanup().await;
}

/// Case 14: one message, two channels, one task and one handoff.
#[tokio::test]
async fn a_message_to_a_held_and_an_automatic_channel_produces_one_of_each() {
    let Some(fixture) = Fixture::new("inbound_hold_mixed").await else {
        return;
    };
    let billing_id = fixture.extra_channel("billing").await;
    let billing_binding = fixture.email_binding_of(billing_id).await;
    let support_binding = fixture.email_binding_of(fixture.channel_id).await;
    let rfc = format!("<hold-mixed-{}@example.com>", fixture.suffix);

    let mut request = request(&fixture, &rfc, "Two teams, please").await;
    request.associations = BoundedVec::parse(
        "thread associations",
        vec![
            ThreadAssociation {
                channel_id: fixture.channel_id,
                binding_id: support_binding,
                target: ThreadTarget::Create {
                    subject: "Quick question".to_string(),
                },
                role: RecipientRole::To,
                step: PipelineStep::only(),
                principals: sender_principals(),
            },
            ThreadAssociation {
                channel_id: billing_id,
                binding_id: billing_binding,
                target: ThreadTarget::Create {
                    subject: "Quick question".to_string(),
                },
                role: RecipientRole::To,
                step: PipelineStep::only(),
                principals: sender_principals(),
            },
        ],
    )
    .unwrap();
    // Support is held; billing answers.
    request.tasks = BoundedVec::parse(
        "inbound tasks",
        vec![InboundTaskRequest {
            task_type: AGENT_DISPATCH.to_string(),
            targets: vec![InboundTaskTarget {
                channel_id: billing_id,
                role: RecipientRole::To,
            }],
        }],
    )
    .unwrap();
    request.holds = BoundedVec::parse(
        "thread handoffs",
        vec![InboundHold {
            channel_id: fixture.channel_id,
            handoff_id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }],
    )
    .unwrap();

    let outcome = fixture.persistence.commit_inbound(request).await.unwrap();

    assert_eq!(message_count(&fixture).await, 1, "one canonical message");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM thread_messages WHERE company_id = $1"
        )
        .await,
        2
    );
    assert_eq!(task_count(&fixture).await, 1);
    let task_channel: Uuid =
        sqlx::query_scalar("SELECT channel_id FROM background_tasks WHERE company_id = $1")
            .bind(fixture.company_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(task_channel, billing_id, "billing's task still runs");

    assert_eq!(handoff_count(&fixture).await, 1);
    let handoff_channel: Uuid =
        sqlx::query_scalar("SELECT channel_id FROM thread_handoffs WHERE company_id = $1")
            .bind(fixture.company_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(handoff_channel, fixture.channel_id);
    assert_eq!(outcome.handoff_ids.len(), 1);

    fixture.cleanup().await;
}

/// Case 15: two commits racing to hold one thread. Neither may raise a duplicate-key error out of
/// an ingest -- the `ON CONFLICT (company_id, thread_id) DO UPDATE` is what turns the loser into a
/// replacement rather than a failure. The row ends at version 2 with one of the two generations,
/// and there are two events.
#[tokio::test]
async fn two_concurrent_commits_on_one_thread_serialise_on_the_unique_key() {
    let Some(fixture) = Fixture::new("inbound_hold_race").await else {
        return;
    };
    // Both generations open on this already-existing thread, so neither commit creates it and the
    // race is exactly the one the `ON CONFLICT` has to survive.
    let thread = fixture
        .extra_thread(fixture.channel_id, "Invoice 4471")
        .await;
    let binding = fixture.email_binding_of(fixture.channel_id).await;

    let mut requests = Vec::new();
    for index in 0..2 {
        let rfc = format!("<hold-race-{index}-{}@example.com>", fixture.suffix);
        let mut request = held_request(&fixture, &rfc, "Racing reply").await;
        request.envelope.source.thread_key = ExternalThreadKey::parse(&rfc).unwrap();
        request.associations = BoundedVec::parse(
            "thread associations",
            vec![ThreadAssociation {
                channel_id: fixture.channel_id,
                binding_id: binding,
                target: ThreadTarget::Existing(thread.id),
                role: RecipientRole::To,
                step: PipelineStep::only(),
                principals: sender_principals(),
            }],
        )
        .unwrap();
        requests.push(request);
    }
    let second = requests.pop().unwrap();
    let first = requests.pop().unwrap();
    let first_generation = first.holds[0].generation;
    let second_generation = second.holds[0].generation;

    let left = fixture.persistence.clone();
    let right = fixture.persistence.clone();
    let (one, two) = tokio::join!(
        tokio::spawn(async move { left.commit_inbound(first).await }),
        tokio::spawn(async move { right.commit_inbound(second).await }),
    );
    one.unwrap()
        .expect("neither commit may raise a duplicate key");
    two.unwrap()
        .expect("neither commit may raise a duplicate key");

    assert_eq!(handoff_count(&fixture).await, 1);
    let (_, generation, state, version, _, _, _, _) = handoff_row(&fixture).await;
    assert_eq!(version, 2, "the loser saw the winner's version");
    assert_eq!(state, "needs_instruction");
    assert!(
        generation == first_generation || generation == second_generation,
        "the surviving generation is one of the two that raced"
    );
    let events = handoff_events(&fixture).await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, "opened");
    assert_eq!(events[1].0, "regenerated");
    assert_eq!(task_count(&fixture).await, 0);

    fixture.cleanup().await;
}

/// Case 19: a hold naming a channel with no association is a planner bug, and the whole commit
/// rolls back — no message, no thread, no mapping. The task equivalent is proved just above.
#[tokio::test]
async fn a_hold_with_no_association_rolls_the_whole_commit_back() {
    let Some(fixture) = Fixture::new("inbound_hold_rollback").await else {
        return;
    };
    let rfc = format!("<hold-orphan-{}@example.com>", fixture.suffix);
    let mut request = held_request(&fixture, &rfc, "Anyone there?").await;
    hold_the_only_channel(&mut request, Uuid::new_v4());

    let refused = fixture.persistence.commit_inbound(request).await;
    assert!(refused.is_err(), "a hold with no association is refused");

    assert_eq!(message_count(&fixture).await, 0);
    assert_eq!(handoff_count(&fixture).await, 0);
    assert_eq!(task_count(&fixture).await, 0);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM external_messages WHERE company_id = $1"
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
