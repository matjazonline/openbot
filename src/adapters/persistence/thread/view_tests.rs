//! Database tests for the purpose-built message reads.
//!
//! Two questions, over and over. Does each projection work for a message *no mail carried* -- the
//! shape a Slack post, a schedule prompt and an agent's answer all share? And does the one
//! projection that does expose provider identifiers refuse an id from another tenant?
//!
//! The fixture lives in [`super::test_support`].

use super::test_support::*;
use super::*;
use crate::entities::{
    message::MessageDirection, message_view::THREAD_HISTORY_LIMIT, transport::TransportKind,
};
use crate::use_cases::thread::{AgentAuthor, MessageAuthorWrite, MessageWrite};

/// The shape every projection has to work for: no headers, no recipients, no address.
fn internal_message(thread_id: Uuid, body: &str, author: MessageAuthorWrite) -> MessageWrite {
    MessageWrite::internal(
        thread_id,
        author,
        "Nightly audit",
        body,
        MessageDirection::Outbound,
        MessageRole::Agent,
        crate::entities::correlation::CorrelationId::new(),
    )
}

/// The agent's own principal, created with the agent, is what an answer is attributed to -- not
/// the mailbox it happened to leave through.
async fn agent_author(fixture: &Fixture) -> MessageAuthorWrite {
    MessageAuthorWrite::Agent(AgentAuthor {
        agent_id: fixture.agent().await,
        display_label: "Triage Agent".into(),
    })
}

#[tokio::test]
async fn a_message_no_transport_carried_reads_back_through_every_projection() {
    let Some(fixture) = Fixture::new("view_internal").await else {
        return;
    };

    let stored = fixture
        .persistence
        .create_message(&internal_message(
            fixture.thread.id,
            "Audit complete.",
            agent_author(&fixture).await,
        ))
        .await
        .unwrap();

    // The page projection: a name, a body, a role -- and no email fields to be missing.
    let views = fixture
        .persistence
        .list_thread_message_views(fixture.thread.id)
        .await
        .unwrap();
    let view = views
        .iter()
        .find(|view| view.canonical_id == stored.canonical_id)
        .expect("the message is in its thread");
    assert_eq!(view.body, "Audit complete.");
    assert_eq!(view.author.display(), "Triage Agent");
    assert_eq!(view.author.handle, None, "no transport named a handle");
    assert_eq!(view.author.transport, None, "so there is no badge");
    assert!(view.is_agent());

    // Reachable one at a time too, which is what the diagnostics link and the live stream use.
    assert_eq!(
        fixture
            .persistence
            .get_thread_message_view(fixture.thread.id, stored.canonical_id)
            .await
            .unwrap()
            .map(|view| view.body),
        Some("Audit complete.".to_string())
    );

    // The prompt projection: role, author, topic, words.
    let history = fixture
        .persistence
        .list_agent_history(fixture.thread.id)
        .await
        .unwrap();
    let turn = history
        .iter()
        .find(|turn| turn.body == "Audit complete.")
        .expect("the answer is in the prompt history");
    assert_eq!(turn.role, MessageRole::Agent);
    assert_eq!(turn.author_display, "Triage Agent");
    assert_eq!(turn.subject, "Nightly audit");

    // The mail renderer's projection: present, and honest about having no headers to reply to.
    let reply_to = fixture
        .persistence
        .latest_email_reply_context(fixture.thread.id)
        .await
        .unwrap()
        .expect("the thread has a newest turn");
    assert_eq!(reply_to.canonical_id, stored.canonical_id);
    assert_eq!(reply_to.rfc_message_id, None);
    assert_eq!(reply_to.author_email, None);
    assert!(reply_to.references.is_empty());
    assert!(reply_to.cc.is_empty());

    fixture.cleanup().await;
}

/// Mail's headers stay reachable where they are actually needed -- one projection, for the one
/// caller that renders an envelope.
#[tokio::test]
async fn the_reply_context_carries_the_headers_mail_arrived_with() {
    let Some(fixture) = Fixture::new("view_reply_context").await else {
        return;
    };
    let rfc = format!("<arrived-{}@partner.test>", fixture.suffix);

    fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&rfc),
            "A question",
        ))
        .await
        .unwrap();

    let reply_to = fixture
        .persistence
        .latest_email_reply_context(fixture.thread.id)
        .await
        .unwrap()
        .expect("the thread has a newest turn");
    assert_eq!(
        reply_to.rfc_message_id.as_ref().map(|id| id.as_str()),
        Some(rfc.as_str())
    );
    assert_eq!(
        reply_to.author_email,
        Some(EmailAddress::from("sender@partner.test"))
    );
    assert_eq!(
        reply_to.cc,
        vec![EmailAddress::from("watcher@partner.test")]
    );

    fixture.cleanup().await;
}

/// A thread is appended to by everyone who can reach the channel, so the newest-N window is a
/// bound rather than a preference -- and it is the *newest* N, because a page and a prompt both
/// want the end of a conversation.
#[tokio::test]
async fn history_keeps_role_and_order_within_a_bounded_newest_window() {
    let Some(fixture) = Fixture::new("view_history_bound").await else {
        return;
    };
    let author = agent_author(&fixture).await;
    let overflow = 5;
    let base = Utc::now() - chrono::Duration::seconds(THREAD_HISTORY_LIMIT as i64 + 10);

    for index in 0..THREAD_HISTORY_LIMIT + overflow {
        let mut write = internal_message(
            fixture.thread.id,
            &format!("turn {index}"),
            if index % 2 == 0 {
                author.clone()
            } else {
                MessageAuthorWrite::Platform
            },
        );
        write.role = if index % 2 == 0 {
            MessageRole::Agent
        } else {
            MessageRole::System
        };
        write.created_at = base + chrono::Duration::seconds(index as i64);
        fixture.persistence.create_message(&write).await.unwrap();
    }

    let history = fixture
        .persistence
        .list_agent_history(fixture.thread.id)
        .await
        .unwrap();
    assert_eq!(history.len(), THREAD_HISTORY_LIMIT);
    assert_eq!(history.first().unwrap().body, format!("turn {overflow}"));
    assert_eq!(
        history.last().unwrap().body,
        format!("turn {}", THREAD_HISTORY_LIMIT + overflow - 1)
    );
    // Roles survive the projection: an agent's turn and a system note are not interchangeable in
    // a prompt, and the alternation pins that they came back attached to the right bodies.
    for (offset, turn) in history.iter().enumerate() {
        let index = overflow + offset;
        assert_eq!(
            turn.role,
            if index % 2 == 0 {
                MessageRole::Agent
            } else {
                MessageRole::System
            },
            "turn {index}"
        );
    }

    // The page read is bounded by the same window.
    let views = fixture
        .persistence
        .list_thread_message_views(fixture.thread.id)
        .await
        .unwrap();
    assert_eq!(views.len(), THREAD_HISTORY_LIMIT);

    // And a caller's own limit cannot raise it.
    let streamed = fixture
        .persistence
        .list_thread_message_views_after(fixture.thread.id, None, 10_000)
        .await
        .unwrap();
    assert_eq!(streamed.len(), THREAD_HISTORY_LIMIT);

    fixture.cleanup().await;
}

/// The one projection that exposes provider keys qualifies each by the interface it belongs to.
///
/// It has to: the same key text can be one message on one binding and a different message on
/// another, so an unqualified key names neither.
#[tokio::test]
async fn the_audit_view_qualifies_provider_keys_by_their_interface() {
    let Some(fixture) = Fixture::new("view_audit").await else {
        return;
    };
    let rfc = format!("<audited-{}@partner.test>", fixture.suffix);

    let stored = fixture
        .persistence
        .create_message(&inbound_email(
            fixture.thread.id,
            email_metadata(&rfc),
            "A question",
        ))
        .await
        .unwrap();

    let audit = fixture
        .persistence
        .get_message_audit(fixture.company_id, stored.id)
        .await
        .unwrap()
        .expect("the message is auditable in its own company");
    assert_eq!(audit.canonical_id, stored.canonical_id);
    assert_eq!(audit.thread_id, fixture.thread.id);
    assert_eq!(audit.channel_id, fixture.channel_id);
    assert_eq!(audit.external_keys.len(), 1);
    assert_eq!(audit.external_keys[0].transport, TransportKind::Email);
    assert_eq!(audit.external_keys[0].key.as_str(), rfc);
    assert_eq!(
        audit.external_keys[0].binding_id,
        fixture.email_binding_of(fixture.channel_id).await
    );

    fixture.cleanup().await;
}

/// A *real* association id, read under another company: the id exists, so nothing about the
/// request is malformed, and the answer still has to be "no such message".
#[tokio::test]
async fn the_audit_view_refuses_a_valid_id_read_under_another_company() {
    let Some(fixture) = Fixture::new("view_audit_tenancy").await else {
        return;
    };

    let stored = fixture
        .persistence
        .create_message(&internal_message(
            fixture.thread.id,
            "Audit complete.",
            agent_author(&fixture).await,
        ))
        .await
        .unwrap();

    let foreign = fixture.foreign_company().await;
    assert!(
        fixture
            .persistence
            .get_message_audit(foreign, stored.id)
            .await
            .unwrap()
            .is_none(),
        "a valid association id from another company must not resolve"
    );
    // And the same id under its own company does, so the refusal above is tenancy rather than the
    // read simply not working.
    assert!(
        fixture
            .persistence
            .get_message_audit(fixture.company_id, stored.id)
            .await
            .unwrap()
            .is_some()
    );

    fixture.cleanup().await;
}

/// The thread-scoped reads are the other half of the same guard: a message is reachable through
/// the thread it is in, and through no other.
#[tokio::test]
async fn a_message_is_not_readable_through_a_thread_that_does_not_hold_it() {
    let Some(fixture) = Fixture::new("view_thread_scope").await else {
        return;
    };

    let stored = fixture
        .persistence
        .create_message(&internal_message(
            fixture.thread.id,
            "Audit complete.",
            agent_author(&fixture).await,
        ))
        .await
        .unwrap();

    let other = fixture.extra_thread(fixture.channel_id, "Elsewhere").await;
    assert!(
        fixture
            .persistence
            .get_thread_message_view(other.id, stored.canonical_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        fixture
            .persistence
            .list_thread_message_views(other.id)
            .await
            .unwrap()
            .is_empty()
    );

    fixture.cleanup().await;
}
