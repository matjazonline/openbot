//! Database tests for reading threads: provider-key resolution, paging, live streams, and the
//! notification that drives them.
//!
//! The fixture and the message-shaped helpers live in [`super::test_support`].

use super::test_support::*;
use super::*;
use crate::entities::email_message::EmailMessageMetadata;
use crate::entities::value_objects::MessageId;
use crate::use_cases::thread::MessageWrite;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};

fn mapi_thread_index(response_levels: usize) -> ThreadIndex {
    let mut bytes: Vec<u8> = (0..22).map(|index| index as u8).collect();
    bytes[0] = 0x01;
    for level in 0..response_levels {
        bytes.extend((0..5).map(|offset| (22 + level * 5 + offset) as u8));
    }
    ThreadIndex::parse(&BASE64_STANDARD.encode(bytes)).unwrap()
}

fn indexed_message(thread_id: Uuid, thread_index: ThreadIndex) -> MessageWrite {
    inbound_email(
        thread_id,
        EmailMessageMetadata::new(MessageId::from(format!("<{}@example.com>", Uuid::new_v4())))
            .thread_index(Some(thread_index))
            .raw_bodies(Some("Body".into()), None),
        "Body",
    )
}

#[tokio::test]
async fn thread_index_lookup_uses_binary_ancestors_and_channel_scope() {
    let Some(fixture) = Fixture::new("thread_index_lookup").await else {
        return;
    };
    let root = mapi_thread_index(0);
    let direct_reply = mapi_thread_index(1);
    let third_reply = mapi_thread_index(3);

    fixture
        .persistence
        .create_message(&indexed_message(fixture.thread.id, root.clone()))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .persistence
            .find_thread_by_thread_index(fixture.channel_id, &direct_reply)
            .await
            .unwrap()
            .unwrap()
            .id,
        fixture.thread.id,
        "a 27-byte direct reply must find its padded 22-byte root"
    );

    let nearer = fixture
        .extra_thread(fixture.channel_id, "Nearer ancestor")
        .await;
    fixture
        .persistence
        .create_message(&indexed_message(nearer.id, direct_reply.clone()))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .persistence
            .find_thread_by_thread_index(fixture.channel_id, &third_reply)
            .await
            .unwrap()
            .unwrap()
            .id,
        nearer.id,
        "the longest stored ancestor wins even when the next ancestor is absent"
    );

    let foreign_channel = fixture.extra_channel("other").await;
    let foreign_thread = fixture
        .extra_thread(foreign_channel, "Same index, other channel")
        .await;
    fixture
        .persistence
        .create_message(&indexed_message(foreign_thread.id, root))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .persistence
            .find_thread_by_thread_index(foreign_channel, &third_reply)
            .await
            .unwrap()
            .unwrap()
            .id,
        foreign_thread.id
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn invalid_thread_index_returns_before_database_access() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
        .unwrap();
    let persistence = PostgresPersistence::new(pool);

    let found = persistence
        .find_thread_by_thread_index(Uuid::new_v4(), &ThreadIndex::from("not-base64"))
        .await
        .unwrap();
    assert!(found.is_none());
}

/// One message in `thread`, distinguishable by its body and pinned to `created_at` so the cursor's
/// tie-break can be exercised deliberately.
fn streamed_message(thread_id: Uuid, body: &str, created_at: DateTime<Utc>) -> MessageWrite {
    inbound_email(
        thread_id,
        email_metadata(&format!("<{}@example.com>", Uuid::new_v4())),
        body,
    )
    .created_at(created_at)
}

fn bodies(messages: &[ThreadMessageView]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message.body.as_str())
        .collect()
}

#[tokio::test]
async fn thread_message_views_read_forward_from_a_cursor() {
    let Some(fixture) = Fixture::new("stream_forward").await else {
        return;
    };
    let base = Utc::now();

    let mut saved = Vec::new();
    for (offset, body) in [(0, "first"), (1, "second"), (2, "third")] {
        saved.push(
            fixture
                .persistence
                .create_message(&streamed_message(
                    fixture.thread.id,
                    body,
                    base + chrono::Duration::seconds(offset),
                ))
                .await
                .unwrap(),
        );
    }

    // No cursor: a reader joining an empty pane gets the whole thread, oldest first.
    let all = fixture
        .persistence
        .list_thread_message_views_after(fixture.thread.id, None, 50)
        .await
        .unwrap();
    assert_eq!(bodies(&all), ["first", "second", "third"]);

    // Resuming excludes the message the cursor names -- it has already been rendered.
    let after_first = fixture
        .persistence
        .list_thread_message_views_after(fixture.thread.id, Some(saved[0].cursor()), 50)
        .await
        .unwrap();
    assert_eq!(bodies(&after_first), ["second", "third"]);

    // A reader who is up to date gets nothing, rather than the thread again.
    let after_last = fixture
        .persistence
        .list_thread_message_views_after(fixture.thread.id, Some(saved[2].cursor()), 50)
        .await
        .unwrap();
    assert!(after_last.is_empty());

    // The batch limit is what stops one wake-up loading an unbounded backlog.
    let limited = fixture
        .persistence
        .list_thread_message_views_after(fixture.thread.id, None, 2)
        .await
        .unwrap();
    assert_eq!(bodies(&limited), ["first", "second"]);

    fixture.cleanup().await;
}

/// Messages saved in one transaction share a timestamp, so a timestamp-only cursor would skip or
/// repeat them. This is the case the `(created_at, id)` comparison exists for.
#[tokio::test]
async fn thread_message_views_break_timestamp_ties_by_id() {
    let Some(fixture) = Fixture::new("stream_ties").await else {
        return;
    };
    let shared = Utc::now();

    let mut saved = Vec::new();
    for body in ["one", "two", "three"] {
        saved.push(
            fixture
                .persistence
                .create_message(&streamed_message(fixture.thread.id, body, shared))
                .await
                .unwrap(),
        );
    }
    saved.sort_by_key(|message| message.cursor());

    let all = fixture
        .persistence
        .list_thread_message_views_after(fixture.thread.id, None, 50)
        .await
        .unwrap();
    assert_eq!(
        all.iter().map(|m| m.id).collect::<Vec<_>>(),
        saved.iter().map(|m| m.id).collect::<Vec<_>>(),
        "same instant, so ordering falls to the id"
    );

    let rest = fixture
        .persistence
        .list_thread_message_views_after(fixture.thread.id, Some(saved[0].cursor()), 50)
        .await
        .unwrap();
    assert_eq!(
        rest.iter().map(|m| m.id).collect::<Vec<_>>(),
        saved[1..].iter().map(|m| m.id).collect::<Vec<_>>()
    );

    fixture.cleanup().await;
}

/// The mirror of the message stream, one level up: a thread whose message just landed must surface
/// in its channel's live column, and a column that reconnects must not replay threads it shows.
#[tokio::test]
async fn list_threads_updated_after_reads_forward_from_a_cursor() {
    let Some(fixture) = Fixture::new("stream_column").await else {
        return;
    };

    // `updated_at` is set by the database, and storing a message bumps it -- so the ordering here
    // is established the same way it is in production, not by writing timestamps.
    let mut threads = vec![fixture.thread.clone()];
    for subject in ["second", "third"] {
        threads.push(fixture.extra_thread(fixture.channel_id, subject).await);
    }

    let all = fixture
        .persistence
        .list_threads_updated_after(fixture.channel_id, None, 50)
        .await
        .unwrap();
    assert_eq!(all.len(), 3, "no cursor means the whole channel");
    assert!(
        all.windows(2)
            .all(|pair| pair[0].cursor() < pair[1].cursor()),
        "oldest first, so the newest is applied last and lands on top"
    );

    let after_first = fixture
        .persistence
        .list_threads_updated_after(fixture.channel_id, Some(all[0].cursor()), 50)
        .await
        .unwrap();
    assert_eq!(after_first.len(), 2);

    let caught_up = fixture
        .persistence
        .list_threads_updated_after(fixture.channel_id, Some(all[2].cursor()), 50)
        .await
        .unwrap();
    assert!(caught_up.is_empty());

    let oldest = &threads[0];
    fixture
        .persistence
        .create_message(&streamed_message(oldest.id, "bump", Utc::now()))
        .await
        .unwrap();

    let bumped = fixture
        .persistence
        .list_threads_updated_after(fixture.channel_id, Some(all[2].cursor()), 50)
        .await
        .unwrap();
    assert_eq!(
        bumped.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![oldest.id],
        "only the bumped thread is newer than what the column already showed"
    );

    fixture.cleanup().await;
}

/// The first notification for `thread_id`, or `None` if none arrives within `timeout`.
///
/// `LISTEN` is database-wide, so a listener sees every thread's messages -- including those of
/// tests running beside this one. Filtering here is not test scaffolding: it is what the SSE
/// handler does with each broadcast event, for the same reason.
async fn notification_for(
    listener: &mut sqlx::postgres::PgListener,
    thread_id: Uuid,
    timeout: std::time::Duration,
) -> Option<serde_json::Value> {
    tokio::time::timeout(timeout, async {
        loop {
            let notification = listener.recv().await.unwrap();
            let payload: serde_json::Value =
                serde_json::from_str(notification.payload()).expect("payload should be JSON");
            if payload["thread_id"].as_str() == Some(&thread_id.to_string()) {
                return payload;
            }
        }
    })
    .await
    .ok()
}

/// The link between a committed message and an open mailbox. Without the trigger firing, nothing
/// else in the live path runs.
#[tokio::test]
async fn committing_a_message_notifies_listeners() {
    let Some(fixture) = Fixture::new("stream_notify").await else {
        return;
    };

    let mut listener = sqlx::postgres::PgListener::connect_with(&fixture.pool)
        .await
        .unwrap();
    listener.listen("thread_message").await.unwrap();

    fixture
        .persistence
        .create_message(&streamed_message(fixture.thread.id, "live", Utc::now()))
        .await
        .unwrap();

    let payload = notification_for(
        &mut listener,
        fixture.thread.id,
        std::time::Duration::from_secs(5),
    )
    .await
    .expect("expected a notification for this thread within 5s");

    assert_eq!(
        payload["channel_id"].as_str().unwrap(),
        fixture.channel_id.to_string()
    );
    assert_eq!(
        payload["company_id"].as_str().unwrap(),
        fixture.company_id.to_string()
    );

    fixture.cleanup().await;
}

/// The notification is bound to the writing transaction, so a rolled-back message must never be
/// announced -- a reader would query for it and find nothing.
#[tokio::test]
async fn a_rolled_back_message_is_not_announced() {
    let Some(fixture) = Fixture::new("stream_rollback").await else {
        return;
    };

    let mut listener = sqlx::postgres::PgListener::connect_with(&fixture.pool)
        .await
        .unwrap();
    listener.listen("thread_message").await.unwrap();

    let mut tx = fixture.pool.begin().await.unwrap();
    let inserted = insert_message_on(
        &mut tx,
        &streamed_message(fixture.thread.id, "rolled back", Utc::now()),
    )
    .await
    .unwrap();
    // Without this the rollback below would prove nothing: the row has to have really been written
    // for its absence afterwards to mean anything.
    assert!(!inserted.association_id.is_nil());

    tx.rollback().await.unwrap();

    assert!(
        notification_for(
            &mut listener,
            fixture.thread.id,
            std::time::Duration::from_secs(1)
        )
        .await
        .is_none(),
        "a rolled-back message must not notify"
    );

    fixture.cleanup().await;
}

/// A thread's parties are principals with explicit roles, and an ordered legacy address list does
/// not silently promote its first entry to author. The UI/email address list is a projection over
/// identities, not the stored key.
#[tokio::test]
async fn a_thread_records_its_parties_as_principals_and_projects_their_addresses() {
    let Some(fixture) = Fixture::new("thread_principals").await else {
        return;
    };

    let author = EmailAddress::from("Author@Partner.test");
    let thread = fixture
        .persistence
        .create_thread(fixture.channel_id, "Subject", std::slice::from_ref(&author))
        .await
        .unwrap();
    assert_eq!(thread.participant_principal_ids.len(), 1);
    assert_eq!(
        thread
            .participant_projection
            .subjects_for(TransportKind::Email),
        vec!["author@partner.test"],
        "the projection carries the normalized mailbox, not what the header said"
    );

    let roles: Vec<String> =
        sqlx::query_scalar("SELECT role FROM thread_principals WHERE thread_id = $1 ORDER BY role")
            .bind(thread.id)
            .fetch_all(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(roles, vec!["participant".to_string()]);

    // A later CC joins as a participant only, and re-adding the original address changes nothing.
    let joined = fixture
        .persistence
        .update_thread_participants(
            thread.id,
            &[author.clone(), EmailAddress::from("cc@partner.test")],
        )
        .await
        .unwrap();
    assert_eq!(joined.participant_principal_ids.len(), 2);
    assert_eq!(
        joined
            .participant_projection
            .subjects_for(TransportKind::Email),
        vec!["author@partner.test", "cc@partner.test"]
    );
    let authors: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM thread_principals WHERE thread_id = $1 AND role = 'author'",
    )
    .bind(thread.id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        authors, 0,
        "an ordered address list does not imply authorship"
    );

    fixture.cleanup().await;
}
