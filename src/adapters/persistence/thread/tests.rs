//! Database tests for the canonical message store and its correlation maps.
//!
//! The fixture and the message-shaped helpers live in [`super::test_support`].

use super::test_support::*;
use super::*;
use crate::entities::{
    correlation::CorrelationId,
    email_message::EmailMessageMetadata,
    message::{AttachmentMetadata, MessageDirection, MessageParticipantKind, MessageRole},
    participant::IdentityProvenance,
    transport::ExternalThreadKey,
};
use crate::use_cases::{
    company::CompanyPersistence,
    thread::{MessageAuthorWrite, MessageCorrelation, MessageWrite},
};

/// The heart of the canonical store: one payload, many conversations -- and never a conversation
/// belonging to somebody else.
#[tokio::test]
async fn one_message_joins_several_threads_but_never_a_foreign_one() {
    let Some(fixture) = Fixture::new("message_fanout").await else {
        return;
    };

    let second_channel = fixture.extra_channel("second").await;
    let second_thread = fixture.extra_thread(second_channel, "Subject").await;

    let stored = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&format!("<fanout-{}@partner.test>", fixture.suffix)),
            "One body",
        ))
        .await
        .unwrap();

    let joined = fixture
        .persistence
        .associate_message(second_thread.id, stored.canonical_id)
        .await
        .unwrap();

    assert_eq!(joined.canonical_id, stored.canonical_id);
    assert_ne!(
        joined.id, stored.id,
        "each association has its own identity"
    );
    assert_eq!(joined.clean_text_body, stored.clean_text_body);
    assert_eq!(joined.thread_id, second_thread.id);

    let canonical_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE company_id = $1")
            .bind(fixture.company_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(canonical_rows, 1, "one payload, stored once");

    // A thread in another company is refused by the composite foreign key, not by a check in Rust
    // that a future caller could forget.
    let foreign_company = fixture.foreign_company().await;
    let foreign_channel = fixture
        .persistence
        .channel(foreign_company, "foreign", &fixture.suffix)
        .await
        .unwrap();
    let foreign_thread = fixture.extra_thread(foreign_channel.id, "Elsewhere").await;
    let refused = fixture
        .persistence
        .associate_message(foreign_thread.id, stored.canonical_id)
        .await;
    assert!(
        refused.is_err(),
        "a message must not cross into another company's thread"
    );

    let _ = CompanyPersistence::delete(&fixture.persistence, foreign_company).await;
    fixture.cleanup().await;
}

/// The reason no provider key lives on `threads`: one conversation can be carried by email and by
/// Slack at the same time, and each side keys it its own way.
#[tokio::test]
async fn one_thread_is_reachable_through_several_bindings() {
    let Some(fixture) = Fixture::new("thread_bindings").await else {
        return;
    };

    let rfc = format!("<multi-{}@partner.test>", fixture.suffix);
    fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&rfc),
            "Over mail",
        ))
        .await
        .unwrap();

    // A Slack binding on the same channel binds the same thread under its own conversation key.
    let slack = fixture.slack_binding(&format!("C{}", fixture.suffix)).await;
    let slack_key = ExternalThreadKey::parse("1712345678.000100").unwrap();
    let mut connection = fixture.pool.acquire().await.unwrap();
    external::upsert_external_thread(
        &mut connection,
        fixture.company_id,
        slack,
        &slack_key,
        fixture.thread.id,
    )
    .await
    .unwrap();

    let bindings: Vec<Uuid> =
        sqlx::query_scalar("SELECT DISTINCT binding_id FROM external_threads WHERE thread_id = $1")
            .bind(fixture.thread.id)
            .fetch_all(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(bindings.len(), 2, "one thread, two provider interfaces");
    assert!(
        bindings.contains(&fixture.email_binding().await),
        "the mail side is bound through the channel's own email interface"
    );
    assert!(bindings.contains(&slack.as_uuid()));

    // The other half of the invariant: within one binding a conversation key names exactly one
    // thread, so a second thread cannot claim it.
    let rival = fixture.extra_thread(fixture.channel_id, "Rival").await;
    external::upsert_external_thread(
        &mut connection,
        fixture.company_id,
        slack,
        &slack_key,
        rival.id,
    )
    .await
    .unwrap();
    let bound: Uuid = sqlx::query_scalar(
        "SELECT thread_id FROM external_threads WHERE binding_id = $1 AND external_thread_key = $2",
    )
    .bind(slack.as_uuid())
    .bind(slack_key.as_str())
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        bound, fixture.thread.id,
        "a conversation key keeps the thread it was first bound to"
    );

    drop(connection);
    fixture.cleanup().await;
}

/// Redelivery is the ordinary case and must be free; a *changed* redelivery is a fault and must be
/// loud, because the alternative is rewriting a message an agent has already answered.
#[tokio::test]
async fn identical_redelivery_is_reused_and_changed_content_collides() {
    let Some(fixture) = Fixture::new("redelivery").await else {
        return;
    };

    let rfc = format!("<redeliver-{}@partner.test>", fixture.suffix);
    let first = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&rfc),
            "Original body",
        ))
        .await
        .unwrap();

    let again = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&rfc),
            "Original body",
        ))
        .await
        .unwrap();
    assert_eq!(again.canonical_id, first.canonical_id);
    assert_eq!(again.id, first.id, "and the same association");

    // The quoted-history strip differs per thread, so a different *clean* body is still the same
    // delivered payload -- this is precisely why `clean_text_body` is not hashed.
    let restripped = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&rfc),
            "Original body\n\n> quoted",
        ))
        .await
        .unwrap();
    assert_eq!(restripped.canonical_id, first.canonical_id);

    // A changed raw body under a key the provider has already used is not a redelivery.
    let changed = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            EmailMessageMetadata::new(MessageId::from(rfc.clone()))
                .raw_bodies(Some("tampered".into()), None),
            "Original body",
        ))
        .await;
    assert!(
        matches!(changed, Err(AppError::Conflict(_))),
        "expected a typed collision, got {changed:?}"
    );

    let canonical_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE company_id = $1")
            .bind(fixture.company_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(canonical_rows, 1);

    fixture.cleanup().await;
}

/// Mail arrives out of order all the time. The reply names the same conversation as the root it
/// answers, so whichever lands first creates the binding and the other joins it.
#[tokio::test]
async fn a_reply_arriving_before_its_root_creates_the_thread_the_root_joins() {
    let Some(fixture) = Fixture::new("reply_before_root").await else {
        return;
    };

    let root_id = format!("<root-{}@partner.test>", fixture.suffix);
    let reply_id = format!("<reply-{}@partner.test>", fixture.suffix);

    fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&reply_id)
                .in_reply_to(Some(MessageId::from(root_id.clone())))
                .references(vec![MessageId::from(root_id.clone())]),
            "The reply",
        ))
        .await
        .unwrap();

    // The root now arrives. Its own id is its conversation key, which is the key the reply already
    // registered -- so thread resolution finds the reply's thread.
    let found = fixture
        .persistence
        .find_thread_by_message_ids(fixture.channel_id, &[MessageId::from(root_id.clone())])
        .await
        .unwrap()
        .expect("the reply's conversation must be findable by the root's id");
    assert_eq!(found.id, fixture.thread.id);

    fixture
        .persistence
        .create_message(&inbound_email(
            found.id,
            email_metadata(&root_id),
            "The root",
        ))
        .await
        .unwrap();

    let conversations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM external_threads WHERE company_id = $1 AND thread_id = $2",
    )
    .bind(fixture.company_id)
    .bind(fixture.thread.id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(conversations, 1, "one conversation, not one per message");

    let joined: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM thread_messages WHERE thread_id = $1")
            .bind(fixture.thread.id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(joined, 2, "both messages are in the one thread");

    fixture.cleanup().await;
}

/// A provider key means nothing on its own: it is a fact about one *interface*. The same
/// Message-ID reaching two of a company's channels is two facts, so it dedups within each binding
/// and collides across none -- which is what lets one channel's agent mail another without the two
/// halves of that exchange fighting over one row.
#[tokio::test]
async fn provider_message_keys_collide_only_inside_one_binding() {
    let Some(fixture) = Fixture::new("key_scope").await else {
        return;
    };

    let rfc = format!("<shared-{}@partner.test>", fixture.suffix);
    let second_channel = fixture.extra_channel("sibling").await;
    let second_thread = fixture.extra_thread(second_channel, "Subject").await;

    let first = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&rfc),
            "Addressed to both",
        ))
        .await
        .unwrap();
    let second = fixture
        .persistence
        .create_message(&inbound_email(
            second_thread.id,
            email_metadata(&rfc),
            "Addressed to both",
        ))
        .await
        .unwrap();

    assert_ne!(
        first.canonical_id, second.canonical_id,
        "a different binding is a different provider fact, so the key dedups within it only"
    );

    let mappings: Vec<(Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT binding_id, message_id FROM external_messages
           WHERE company_id = $1 AND external_message_key = $2"#,
    )
    .bind(fixture.company_id)
    .bind(&rfc)
    .fetch_all(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        mappings.len(),
        2,
        "the same key on two bindings is two facts, not a collision"
    );
    assert_ne!(mappings[0].0, mappings[1].0, "one row per binding");
    assert_ne!(
        mappings[0].1, mappings[1].1,
        "each naming the message that binding actually carried"
    );

    // Each channel resolves the key to its own thread.
    assert_eq!(
        fixture
            .persistence
            .find_thread_by_message_ids(fixture.channel_id, &[MessageId::from(rfc.clone())])
            .await
            .unwrap()
            .unwrap()
            .id,
        fixture.thread.id
    );
    assert_eq!(
        fixture
            .persistence
            .find_thread_by_message_ids(second_channel, &[MessageId::from(rfc.clone())])
            .await
            .unwrap()
            .unwrap()
            .id,
        second_thread.id
    );

    // And the constraint itself: a second mapping for the same key on the same binding is refused.
    let duplicate = sqlx::query(
        r#"INSERT INTO external_messages (
               id, company_id, binding_id, external_message_key, message_id
           ) VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(Uuid::new_v4())
    .bind(fixture.company_id)
    .bind(mappings[0].0)
    .bind(&rfc)
    .bind(mappings[0].1)
    .execute(&fixture.pool)
    .await;
    assert!(duplicate.is_err());

    fixture.cleanup().await;
}

/// Stored JSON is untrusted input, read back long after it was written. A payload this build does
/// not understand has to reach the caller as an error, never as a panic in a request handler.
#[tokio::test]
async fn unreadable_stored_json_surfaces_as_an_application_error() {
    let Some(fixture) = Fixture::new("bad_json").await else {
        return;
    };

    let stored = fixture
        .persistence
        .create_message(
            &inbound_email(
                fixture.thread.id,
                email_metadata(&format!("<attach-{}@partner.test>", fixture.suffix)),
                "With an attachment",
            )
            .with_attachments(vec![AttachmentMetadata {
                filename: "invoice.pdf".into(),
                content_type: "application/pdf".into(),
                sha256_hash: "abc123".into(),
                size_bytes: 4096,
                storage_key: None,
            }]),
        )
        .await
        .unwrap();
    assert_eq!(stored.attachments.as_ref().map(Vec::len), Some(1));

    // The database refuses an envelope of the wrong shape outright.
    let rejected = sqlx::query("UPDATE messages SET attachments = $2 WHERE id = $1")
        .bind(stored.canonical_id.as_uuid())
        .bind(serde_json::json!([{ "filename": "loose.pdf" }]))
        .execute(&fixture.pool)
        .await;
    assert!(
        rejected.is_err(),
        "a bare array is not a versioned envelope"
    );

    // A *structurally* valid envelope from a newer writer passes the constraint and must then fail
    // the decode rather than the process.
    sqlx::query("UPDATE messages SET attachments = $2 WHERE id = $1")
        .bind(stored.canonical_id.as_uuid())
        .bind(serde_json::json!({ "version": "1", "items": [{ "filename": "loose.pdf" }] }))
        .execute(&fixture.pool)
        .await
        .unwrap();

    let read = fixture
        .persistence
        .list_messages_by_thread_id(fixture.thread.id)
        .await;
    assert!(
        matches!(read, Err(AppError::Internal(_))),
        "expected an application error, got {read:?}"
    );

    fixture.cleanup().await;
}

/// The author of a message is a principal, and the addresses a reader renders are a projection
/// over the handles that principal was named by.
#[tokio::test]
async fn a_message_records_its_author_as_a_principal_and_projects_its_recipients() {
    let Some(fixture) = Fixture::new("author_projection").await else {
        return;
    };

    let stored = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&format!("<author-{}@partner.test>", fixture.suffix)),
            "Body",
        ))
        .await
        .unwrap();

    assert_eq!(
        stored.author.email_address(),
        Some(EmailAddress::from("sender@partner.test"))
    );
    assert_eq!(
        stored.email_recipients(MessageParticipantKind::To),
        vec![EmailAddress::from("primary@example.com")]
    );
    assert_eq!(
        stored.email_recipients(MessageParticipantKind::Cc),
        vec![EmailAddress::from("watcher@partner.test")]
    );
    assert_eq!(
        stored.rfc_message_id().map(MessageId::as_str),
        Some(format!("<author-{}@partner.test>", fixture.suffix).as_str())
    );

    let kind: String =
        sqlx::query_scalar("SELECT kind FROM principals WHERE company_id = $1 AND id = $2")
            .bind(fixture.company_id)
            .bind(stored.author.principal_id.as_uuid())
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(kind, "external", "a stranger's mailbox is an outside actor");

    fixture.cleanup().await;
}

/// A schedule prompt, an approval note and an agent's answer are complete messages with no mail
/// behind them at all -- no address, no Message-ID, no recipients.
#[tokio::test]
async fn a_message_no_transport_carried_needs_no_email_fields() {
    let Some(fixture) = Fixture::new("internal_message").await else {
        return;
    };

    let note = fixture
        .persistence
        .create_message(&MessageWrite::internal(
            fixture.thread.id,
            MessageAuthorWrite::Platform,
            "[HITL Granted]: Refund",
            "Human approval GRANTED.",
            MessageDirection::Inbound,
            MessageRole::System,
            CorrelationId::new(),
        ))
        .await
        .unwrap();

    assert!(note.email.is_none());
    assert!(note.participants.is_empty());
    assert_eq!(note.sender_email(), None);
    assert_eq!(note.author.display(), "System");

    let kind: String =
        sqlx::query_scalar("SELECT kind FROM principals WHERE company_id = $1 AND id = $2")
            .bind(fixture.company_id)
            .bind(note.author.principal_id.as_uuid())
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(kind, "system");

    // The platform is one actor per company, however many notes it writes.
    let second = fixture
        .persistence
        .create_message(&MessageWrite::internal(
            fixture.thread.id,
            MessageAuthorWrite::Platform,
            "[HITL Rejected]: Refund",
            "Human approval REJECTED.",
            MessageDirection::Inbound,
            MessageRole::System,
            CorrelationId::new(),
        ))
        .await
        .unwrap();
    assert_eq!(second.author.principal_id, note.author.principal_id);

    // Nothing correlated it, so nothing claims a provider key for it either.
    let mapped: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM external_messages WHERE company_id = $1")
            .bind(fixture.company_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(mapped, 0);

    fixture.cleanup().await;
}

/// The idempotency guard the agent worker runs on: an answer already in the thread means the run
/// happened, and an outreach's own outbound mail is not that answer.
#[tokio::test]
async fn find_outbound_reply_after_sees_answers_and_ignores_outreach_mail() {
    let Some(fixture) = Fixture::new("outbound_reply").await else {
        return;
    };

    let trigger = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&format!("<trigger-{}@partner.test>", fixture.suffix)),
            "Please answer",
        ))
        .await
        .unwrap();

    assert!(
        fixture
            .persistence
            .find_outbound_reply_after(fixture.thread.id, trigger.canonical_id)
            .await
            .unwrap()
            .is_none(),
        "nothing has answered yet"
    );

    let outreach_rfc = format!("<outreach-{}@partner.test>", fixture.suffix);
    let outreach = fixture
        .persistence
        .create_message(&MessageWrite {
            thread_id: fixture.thread.id,
            author: observed("primary@example.com", IdentityProvenance::Agent),
            subject: "Outreach".into(),
            clean_text_body: "Asking a third party".into(),
            attachments: Vec::new(),
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            correlation_id: CorrelationId::new(),
            participants: vec![participant(
                MessageParticipantKind::To,
                "target@partner.test",
            )],
            correlation: MessageCorrelation::Email(email_metadata(&outreach_rfc)),
            created_at: Utc::now(),
        })
        .await
        .unwrap();

    // Wire the outreach bookkeeping that marks this send as "the agent asking", not "the answer".
    let task_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO background_tasks (id, company_id, channel_id, thread_id, correlation_id, \
         task_type, status, payload) \
         VALUES ($1, $2, $3, $4, gen_random_uuid(), 'email_agent_dispatch', \
                 'waiting_for_third_party_reply', '{}')",
    )
    .bind(task_id)
    .bind(fixture.company_id)
    .bind(fixture.channel_id)
    .bind(fixture.thread.id)
    .execute(&fixture.pool)
    .await
    .unwrap();
    let outreach_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO task_outreaches (
                id, task_id, outreach_key, status, required_threshold_percent,
                expires_at, subject, body
           ) VALUES ($1, $2, $3, 'waiting', 100.0, $4, 'Outreach', 'Outreach body')"#,
    )
    .bind(outreach_id)
    .bind(task_id)
    .bind(&fixture.suffix)
    .bind(Utc::now() + chrono::Duration::hours(1))
    .execute(&fixture.pool)
    .await
    .unwrap();
    let outbox_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO email_outbox (
                id, company_id, channel_id, task_id, correlation_id, idempotency_key, payload,
                status, provider_message_id
           ) VALUES ($1, $2, $3, $4, gen_random_uuid(), $5, '{}', 'sent', $6)"#,
    )
    .bind(outbox_id)
    .bind(fixture.company_id)
    .bind(fixture.channel_id)
    .bind(task_id)
    .bind(format!("outreach:{}:target:0", fixture.suffix))
    .bind(&outreach_rfc)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO task_outreach_targets (outreach_id, email, outbox_id) \
         VALUES ($1, 'target@partner.test', $2)",
    )
    .bind(outreach_id)
    .bind(outbox_id)
    .execute(&fixture.pool)
    .await
    .unwrap();

    assert!(
        fixture
            .persistence
            .find_outbound_reply_after(fixture.thread.id, trigger.canonical_id)
            .await
            .unwrap()
            .is_none(),
        "an outreach's own mail is the agent asking, not the agent answering"
    );

    let answer = fixture
        .persistence
        .create_message(&MessageWrite {
            thread_id: fixture.thread.id,
            author: observed("primary@example.com", IdentityProvenance::Agent),
            subject: "Re: Subject".into(),
            clean_text_body: "Here is the answer".into(),
            attachments: Vec::new(),
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            correlation_id: CorrelationId::new(),
            participants: vec![participant(
                MessageParticipantKind::To,
                "sender@partner.test",
            )],
            correlation: MessageCorrelation::Email(email_metadata(&format!(
                "<answer-{}@partner.test>",
                fixture.suffix
            ))),
            created_at: Utc::now() + chrono::Duration::seconds(1),
        })
        .await
        .unwrap();

    let found = fixture
        .persistence
        .find_outbound_reply_after(fixture.thread.id, trigger.canonical_id)
        .await
        .unwrap()
        .expect("the answer must be found");
    assert_eq!(found.canonical_id, answer.canonical_id);
    assert_ne!(found.canonical_id, outreach.canonical_id);

    fixture.cleanup().await;
}

/// The last association of a message is what a live column marks a thread by.
#[tokio::test]
async fn a_deleted_association_takes_an_orphaned_payload_with_it() {
    let Some(fixture) = Fixture::new("orphan_cleanup").await else {
        return;
    };

    let second_channel = fixture.extra_channel("mirror").await;
    let second_thread = fixture.extra_thread(second_channel, "Subject").await;
    let stored = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&format!("<orphan-{}@partner.test>", fixture.suffix)),
            "Body",
        ))
        .await
        .unwrap();
    fixture
        .persistence
        .associate_message(second_thread.id, stored.canonical_id)
        .await
        .unwrap();

    sqlx::query("DELETE FROM thread_messages WHERE id = $1")
        .bind(stored.id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    let surviving: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE id = $1")
        .bind(stored.canonical_id.as_uuid())
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(surviving, 1, "another thread still holds it");

    sqlx::query("DELETE FROM thread_messages WHERE message_id = $1")
        .bind(stored.canonical_id.as_uuid())
        .execute(&fixture.pool)
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE id = $1")
        .bind(stored.canonical_id.as_uuid())
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0, "the last association takes the payload");

    fixture.cleanup().await;
}
