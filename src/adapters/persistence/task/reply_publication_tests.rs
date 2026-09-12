use super::*;
use crate::{
    adapters::persistence::test_support::OwnDatabase,
    entities::{
        cursor::ThreadCursor,
        message::{CanonicalMessageId, ThreadEntryKind},
    },
};

#[path = "reply_publication_test_support.rs"]
mod support;
use support::*;

#[derive(sqlx::FromRow)]
struct ThreadUpdate {
    id: Uuid,
    updated_at: DateTime<Utc>,
}

#[tokio::test]
async fn an_earlier_transaction_publishing_later_advances_every_thread_cursor() {
    let Some(fixture) = PublicationFixture::new().await else {
        return;
    };
    let mut earlier = fixture.persistence.pool().begin().await.unwrap();
    // Force a transaction timestamp older than the other pipeline's publication.
    sqlx::query("SELECT CURRENT_TIMESTAMP")
        .execute(&mut *earlier)
        .await
        .unwrap();
    commit(&fixture, &fixture.support).await;
    let before: Vec<ThreadUpdate> =
        sqlx::query_as("SELECT id, updated_at FROM threads WHERE company_id = $1")
            .bind(fixture.company_id)
            .fetch_all(fixture.persistence.pool())
            .await
            .unwrap();
    crate::adapters::persistence::thread::publish_task_reply_on(
        &mut earlier,
        crate::adapters::persistence::thread::TaskReplyPublication {
            company_id: fixture.company_id,
            task_id: Some(fixture.billing.task.id),
            message: &fixture.billing.reply.message,
            also_in_threads: &fixture.billing.reply.also_in_threads,
        },
    )
    .await
    .unwrap();
    earlier.commit().await.unwrap();
    for previous in before {
        let current = fixture
            .persistence
            .get_thread_by_id(previous.id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            current.updated_at > previous.updated_at,
            "publication time, rather than transaction start, advances every affected thread"
        );
    }
}

#[tokio::test]
async fn published_replies_cross_file_without_answering_sibling_tasks() {
    let Some(fixture) = PublicationFixture::new().await else {
        return;
    };
    publish_support_and_check_sibling(&fixture).await;
    commit(&fixture, &fixture.billing).await;
    for thread_id in [
        fixture.support.reply.message.thread_id,
        fixture.sales.thread_id,
    ] {
        assert_entry(
            &fixture,
            thread_id,
            fixture.billing.reply.message.id,
            ThreadEntryKind::Delegation,
        )
        .await;
        assert_entry(
            &fixture,
            thread_id,
            fixture.support.reply.message.id,
            ThreadEntryKind::Conversation,
        )
        .await;
    }
    assert_entry(
        &fixture,
        fixture.billing.reply.message.thread_id,
        fixture.billing.reply.message.id,
        ThreadEntryKind::Conversation,
    )
    .await;
    assert_eq!(
        fixture
            .delegation_count(fixture.support.reply.message.id)
            .await,
        2
    );
    assert_eq!(
        fixture
            .delegation_count(fixture.billing.reply.message.id)
            .await,
        3
    );
    let task_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM background_tasks WHERE company_id = $1")
            .bind(fixture.company_id)
            .fetch_one(fixture.persistence.pool())
            .await
            .unwrap();
    assert_eq!(
        task_count, 2,
        "the filed-only channel never receives a task"
    );
}

#[tokio::test]
async fn competing_sibling_publications_use_one_thread_lock_order() {
    let Some(fixture) = PublicationFixture::new().await else {
        return;
    };
    use crate::adapters::persistence::thread_lock_test_support::*;
    let names = ["support-publication", "billing-publication"];
    let support = contender(fixture.persistence.pool(), names[0]).await;
    let billing = contender(fixture.persistence.pool(), names[1]).await;
    // Block the first writer's own thread: without prelocking, the second writer would own
    // its primary thread while waiting here, creating the opposite lock order on release.
    let blocker = hold_first_thread(
        fixture.persistence.pool(),
        &[fixture.support.reply.message.thread_id],
    )
    .await;
    tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            commit_with(&support, &fixture.support),
            async {
                wait_until_blocked(fixture.persistence.pool(), &[names[0]]).await;
                commit_with(&billing, &fixture.billing).await;
            },
            release_when_both_wait(blocker, fixture.persistence.pool(), &names),
        );
    })
    .await
    .expect("sibling publication must not deadlock");
    assert_entry(
        &fixture,
        fixture.support.reply.message.thread_id,
        fixture.billing.reply.message.id,
        ThreadEntryKind::Delegation,
    )
    .await;
    assert_entry(
        &fixture,
        fixture.billing.reply.message.thread_id,
        fixture.support.reply.message.id,
        ThreadEntryKind::Delegation,
    )
    .await;
    assert_eq!(
        fixture
            .delegation_count(fixture.support.reply.message.id)
            .await,
        2
    );
    assert_eq!(
        fixture
            .delegation_count(fixture.billing.reply.message.id)
            .await,
        3
    );
}

#[tokio::test]
async fn repeated_cross_filing_does_not_duplicate_or_refresh_sibling_context() {
    let Some(fixture) = PublicationFixture::new().await else {
        return;
    };
    commit(&fixture, &fixture.support).await;
    let before = fixture
        .persistence
        .get_thread_by_id(fixture.filed.thread_id)
        .await
        .unwrap()
        .unwrap();
    let mut replay = fixture.persistence.pool().begin().await.unwrap();
    crate::adapters::persistence::thread::file_in_sibling_threads_on(
        &mut replay,
        fixture.company_id,
        fixture.support.reply.message.id,
        &[
            fixture.billing.reply.message.thread_id,
            fixture.filed.thread_id,
        ],
    )
    .await
    .unwrap();
    replay.commit().await.unwrap();
    let after = fixture
        .persistence
        .get_thread_by_id(fixture.filed.thread_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .delegation_count(fixture.support.reply.message.id)
            .await,
        2
    );
    assert_eq!(
        before.updated_at, after.updated_at,
        "a replay produces no sidebar activity"
    );
    let stale = TaskLeaseRef {
        execution_generation: Uuid::new_v4(),
        ..fixture.support.lease
    };
    let outcome = fixture
        .persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease: stale,
            reply: &fixture.support.reply,
            deliveries: Vec::new(),
            review_candidate: None,
            payload: serde_json::json!({}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    assert_eq!(outcome, DispatchCommit::LeaseLost);
    assert_eq!(
        fixture
            .delegation_count(fixture.support.reply.message.id)
            .await,
        2
    );
}

#[tokio::test]
async fn one_address_and_sources_without_siblings_do_not_cross_file() {
    let Some(mut fixture) = PublicationFixture::new().await else {
        return;
    };
    // A single + address owns both remaining source associations.
    sqlx::query("DELETE FROM thread_messages WHERE message_id = $1 AND NOT thread_id = ANY($2)")
        .bind(fixture.source.as_uuid())
        .bind([
            fixture.support.reply.message.thread_id,
            fixture.sales.thread_id,
        ])
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
    commit(&fixture, &fixture.support).await;
    assert_eq!(
        fixture
            .delegation_count(fixture.support.reply.message.id)
            .await,
        0
    );
    // A selected-note cause lives only in its own thread.
    sqlx::query("DELETE FROM thread_messages WHERE message_id = $1 AND thread_id = $2")
        .bind(fixture.source.as_uuid())
        .bind(fixture.sales.thread_id)
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
    fixture.support.reply.also_in_threads.clear();
    fixture.support.reply.message.id = CanonicalMessageId::new(Uuid::new_v4());
    commit(&fixture, &fixture.support).await;
    assert_eq!(
        fixture
            .delegation_count(fixture.support.reply.message.id)
            .await,
        0
    );
    // Schedule-driven work has no source_message_uuid to find siblings through.
    sqlx::query("UPDATE background_tasks SET source_message_uuid = NULL WHERE id = $1")
        .bind(fixture.billing.task.id)
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
    commit(&fixture, &fixture.billing).await;
    assert_eq!(
        fixture
            .delegation_count(fixture.billing.reply.message.id)
            .await,
        0
    );
}

#[tokio::test]
async fn reviewed_reply_is_cross_filed_only_on_approval_and_once() {
    let Some(fixture) = PublicationFixture::new().await else {
        return;
    };
    let command = submit_support_for_review(&fixture).await;
    assert_eq!(
        fixture
            .delegation_count(fixture.support.reply.message.id)
            .await,
        0
    );
    assert!(
        fixture
            .persistence
            .get_thread_message(fixture.filed.thread_id, fixture.support.reply.message.id)
            .await
            .unwrap()
            .is_none()
    );
    let first = fixture
        .persistence
        .execute_review_command(command.clone())
        .await
        .unwrap();
    assert_eq!(
        fixture
            .delegation_count(fixture.support.reply.message.id)
            .await,
        2
    );
    assert_entry(
        &fixture,
        fixture.sales.thread_id,
        fixture.support.reply.message.id,
        ThreadEntryKind::Conversation,
    )
    .await;
    let before = fixture
        .persistence
        .get_thread_by_id(fixture.filed.thread_id)
        .await
        .unwrap()
        .unwrap();
    let second = fixture
        .persistence
        .execute_review_command(command)
        .await
        .unwrap();
    assert_eq!(first, second);
    let after = fixture
        .persistence
        .get_thread_by_id(fixture.filed.thread_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.updated_at, after.updated_at);
    assert_eq!(
        fixture
            .delegation_count(fixture.support.reply.message.id)
            .await,
        2
    );
}
