//! Database tests for the one transaction an accepted inbound message goes through.
//!
//! These are the assertions the in-memory double cannot make. Atomicity, the advisory lock that
//! serializes two simultaneous deliveries, and the unique indexes that absorb a redelivery are all
//! properties of PostgreSQL, so they are exercised against it.

use super::test_support::*;
use super::*;
use crate::adapters::persistence::channel_edit::{
    EditBudget, update_channel_on, update_channel_within,
};
use crate::entities::{
    correlation::CorrelationId,
    email_message::EmailMessageMetadata,
    participant::{IdentityClaimMetadata, IdentityProvenance, ThreadPrincipalRole},
    transport::{
        ChannelBindingId, ExternalEventKey, ExternalMessageKey, ExternalThreadKey,
        IdentityNamespace, IdentitySubject, InboundEventId, InboundSource, QualifiedIdentity,
        TransportKind,
    },
    value_objects::MessageId,
};
use crate::transport::{
    AddressedIdentity, AuthenticatedInboundEvent, BoundedVec, CanonicalContent,
    ClaimedInboundEvent, CommitDisposition, ExternalCorrelationStore, InboundAdmissionIntent,
    InboundCommitOutcome, InboundCommitRequest, InboundEnvelope, InboundEventInbox,
    InboundEventPayload, InboundEventQueue, InboundMessageCommitter, InboundOrigin,
    IngressDirectives, IngressPolicyFacts, PipelineStep, ProtocolExtension, RecipientRole,
    ReplyDelivery, SafeHeaderFacts, ThreadAssociation, ThreadPrincipalIntent, ThreadTarget,
    WorkerId,
};
use crate::use_cases::channel::{ChannelPersistence, ChannelUpdate, ChannelWrite};
use crate::use_cases::participant::{IdentityDirectory, IdentityObservation};

#[path = "inbound_multi_task_tests.rs"]
mod multi_task_tests;

/// An answering intent for one channel, with no mention or parent.
fn intent(channel_id: Uuid, role: RecipientRole) -> InboundAdmissionIntent {
    InboundAdmissionIntent {
        channel_id,
        role,
        mentioned: false,
        reply_to: None,
        settings_revision: 0,
    }
}

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
        intents: BoundedVec::parse(
            "admission intents",
            vec![intent(fixture.channel_id, RecipientRole::To)],
        )
        .unwrap(),
        admissible: true,
        origin: InboundOrigin::External,
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
    request.intents = BoundedVec::parse(
        "admission intents",
        vec![intent(channel_id, RecipientRole::To)],
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

async fn intent_count(fixture: &Fixture) -> i64 {
    count(
        fixture,
        "SELECT count(*) FROM workflow_message_intents WHERE company_id = $1",
    )
    .await
}

/// The whole point of the commit: the message, its mapping and its admission intent exist
/// together (p08 D1.3).
#[tokio::test]
async fn an_accepted_message_lands_with_its_mapping_thread_and_intent_at_once() {
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
    assert_eq!(outcome.intent_ids.len(), 1);
    let intent_id = outcome.intent_ids[0];

    // No accepted message needs a post-commit mapping or intent insert: every row is already there.
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
    assert_eq!(intent_count(&fixture).await, 1);

    // The intent carries identifiers and ingress facts only: the message, its association, the
    // stored author as responder, the relay trace, and no legacy task anywhere.
    let row: (
        Uuid,
        Uuid,
        Uuid,
        String,
        bool,
        bool,
        String,
        i32,
        String,
        String,
        bool,
    ) = sqlx::query_as(
        "SELECT intent.message_id, intent.channel_id, intent.thread_id, intent.role, \
               intent.mentioned, intent.admissible, intent.origin, intent.hop_count, \
               intent.delivery_policy, intent.state, \
               intent.responder_principal_id = message.author_principal_id \
             FROM workflow_message_intents AS intent JOIN messages AS message \
               ON message.company_id = intent.company_id AND message.id = intent.message_id \
             WHERE intent.company_id = $1 AND intent.id = $2",
    )
    .bind(fixture.company_id)
    .bind(intent_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            outcome.message_id.as_uuid(),
            fixture.channel_id,
            outcome.thread_ids[0],
            "to".to_string(),
            false,
            true,
            "external".to_string(),
            0,
            "deliver".to_string(),
            "pending".to_string(),
            true
        )
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM background_tasks WHERE company_id = $1"
        )
        .await,
        0
    );

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
    assert_eq!(second.intent_ids, first.intent_ids);
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(intent_count(&fixture).await, 1);
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

    let first = multi_task_tests::two_channel_request(&fixture, &rfc).await;
    let second = first.clone();
    let one = PostgresPersistence::new(fixture.pool.clone());
    let two = PostgresPersistence::new(fixture.pool.clone());
    let (left, right) = tokio::join!(one.commit_inbound(first), two.commit_inbound(second));

    let outcomes: Vec<InboundCommitOutcome> = vec![left.unwrap(), right.unwrap()];
    assert_eq!(outcomes[0].message_id, outcomes[1].message_id);
    assert_eq!(outcomes[0].intent_ids, outcomes[1].intent_ids);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.disposition == CommitDisposition::Created)
            .count(),
        1,
        "exactly one of the two deliveries stored the message"
    );
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(intent_count(&fixture).await, 2);
    assert_eq!(outcomes[0].intent_ids.len(), 2);
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

/// A settings edit that switches the fixture's channel off and leaves its address alone.
async fn disable(fixture: &Fixture) -> ChannelUpdate {
    let channel = ChannelPersistence::get_by_id(&fixture.persistence, fixture.channel_id)
        .await
        .unwrap()
        .unwrap();
    ChannelUpdate {
        company_id: fixture.company_id,
        channel_id: fixture.channel_id,
        write: ChannelWrite {
            name: channel.name,
            slug: channel.slug.into_string(),
            enabled: false,
            ..ChannelWrite::default()
        },
    }
}

/// Switch the fixture channel on behind the edit path's back, so the revision stays where routing
/// read it.
async fn switch_on(fixture: &Fixture) {
    sqlx::query("UPDATE channels SET enabled = true WHERE id = $1")
        .bind(fixture.channel_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
}

/// The fixture channel's `(enabled, settings_revision)`.
async fn channel_settings(fixture: &Fixture) -> (bool, i64) {
    sqlx::query_as("SELECT enabled, settings_revision FROM channels WHERE id = $1")
        .bind(fixture.channel_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap()
}

/// The competing claimant, edit first: a commit that reaches the gate while a settings edit holds
/// it waits for the edit, then refuses -- it was routed against settings that no longer exist --
/// and stores nothing, so its transport's retry routes it against the new ones (p08 D4.8).
#[tokio::test]
async fn a_commit_queued_behind_a_settings_edit_is_refused_with_nothing_stored() {
    let Some(fixture) = Fixture::new("inbound_gate_edit_first").await else {
        return;
    };
    switch_on(&fixture).await;
    let rfc = format!("<gate-edit-first-{}@example.com>", fixture.suffix);
    let routed = request(&fixture, &rfc, "Still open?").await;
    let mut edit = fixture.pool.begin().await.unwrap();
    update_channel_on(&mut edit, &disable(&fixture).await, EditBudget::DEFAULT)
        .await
        .unwrap();

    let commit = tokio::spawn({
        let persistence = PostgresPersistence::new(fixture.pool.clone());
        async move { persistence.commit_inbound(routed).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!commit.is_finished(), "the commit must wait for the edit");

    edit.commit().await.unwrap();
    let refused = commit.await.unwrap();
    assert!(
        matches!(&refused, Err(AppError::Superseded(_))),
        "{refused:?}"
    );
    assert_eq!(message_count(&fixture).await, 0);
    assert_eq!(intent_count(&fixture).await, 0);
    assert_eq!(channel_settings(&fixture).await, (false, 1));

    fixture.cleanup().await;
}

/// The competing claimant, commit first: an edit that reaches the gate while an inbound commit
/// holds it waits for that commit, so the message lands under the settings it was routed against
/// and the edit applies after it -- never in between (p08 D4.8).
///
/// The commit is paused right after its settings check, before it has written or locked a row:
/// the window where only the gate keeps the edit out. Later, its foreign keys would hold the
/// channel row too.
#[tokio::test]
async fn a_settings_edit_queued_behind_a_commit_applies_after_it() {
    let Some(fixture) = Fixture::new("inbound_gate_commit_first").await else {
        return;
    };
    switch_on(&fixture).await;
    let rfc = format!("<gate-commit-first-{}@example.com>", fixture.suffix);
    let routed = request(&fixture, &rfc, "Anyone there?").await;
    let mut admission = fixture.pool.begin().await.unwrap();
    super::inbound::hold_channel_settings(&mut admission, &routed)
        .await
        .unwrap();

    let edit = tokio::spawn({
        let pool = fixture.pool.clone();
        let update = disable(&fixture).await;
        // Patient enough to outlast the commit on a loaded run: the wait is the point here, and
        // the edit's own refusal on a lock timeout is covered in `channel_edit_tests`.
        let patient = EditBudget {
            lock_timeout: std::time::Duration::from_secs(30),
            ..EditBudget::DEFAULT
        };
        async move { update_channel_within(&pool, update, patient).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!edit.is_finished(), "the edit must wait for the commit");
    assert_eq!(channel_settings(&fixture).await, (true, 0));

    // The rest of the commit, under the gate it already holds.
    super::inbound::commit_on(&mut admission, &routed)
        .await
        .unwrap();
    admission.commit().await.unwrap();
    edit.await
        .unwrap()
        .expect("the edit applies once the commit is done");
    assert_eq!(message_count(&fixture).await, 1);
    assert_eq!(intent_count(&fixture).await, 1);
    assert_eq!(channel_settings(&fixture).await, (false, 1));

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
    assert_eq!(intent_count(&fixture).await, 2);
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
    assert_eq!(intent_count(&fixture).await, 2);

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
async fn a_commit_that_fails_at_the_intent_leaves_no_thread_message_or_mapping() {
    let Some(fixture) = Fixture::new("inbound_rollback").await else {
        return;
    };
    let rfc = format!("<rollback-{}@example.com>", fixture.suffix);

    let mut request = request(&fixture, &rfc, "Anyone there?").await;
    // An intent naming a channel this commit has no association for: refused after the thread,
    // the message and both mappings have been written.
    let mut intents = request.intents.into_inner();
    intents.push(intent(Uuid::new_v4(), RecipientRole::To));
    request.intents = BoundedVec::parse("admission intents", intents).unwrap();

    let refused = fixture.persistence.commit_inbound(request).await;
    assert!(refused.is_err(), "an intent with no association is refused");

    assert_eq!(message_count(&fixture).await, 0);
    assert_eq!(intent_count(&fixture).await, 0);
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
    assert_eq!(intent_count(&fixture).await, 0);
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
    assert_eq!(intent_count(&fixture).await, 1);

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
