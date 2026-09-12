use super::*;
use crate::{entities::message::MessageDirection, use_cases::thread::MessageAuthorWrite};

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct StoredTaskTarget {
    channel_id: Uuid,
    thread_id: Uuid,
    recipient_role: String,
}

#[tokio::test]
async fn a_reverse_address_followup_and_sibling_publication_share_the_thread_lock_order() {
    let Some(fixture) = Fixture::isolated("reply_lock_order").await else {
        return;
    };
    let rfc = format!("<lock-order-{}@example.com>", fixture.suffix);
    let initial = two_pipeline_request(&fixture, &rfc).await;
    let first = fixture
        .persistence
        .commit_inbound(initial.clone())
        .await
        .unwrap();
    let followup_rfc = format!("<followup-{}@example.com>", fixture.suffix);
    let mut followup = request(&fixture, &followup_rfc, "One more question").await;
    let mut associations = initial.associations.into_inner();
    for (association, thread_id) in associations.iter_mut().zip(&first.thread_ids) {
        association.target = ThreadTarget::Existing(*thread_id);
    }
    associations.sort_by_key(|association| match association.target {
        ThreadTarget::Existing(id) => std::cmp::Reverse(id),
        ThreadTarget::Create { .. } => unreachable!(),
    });
    followup.associations = BoundedVec::parse("thread associations", associations).unwrap();
    followup.tasks = BoundedVec::empty();
    let reply = MessageWrite::internal(
        first.thread_ids[0],
        MessageAuthorWrite::Platform,
        "Answer",
        "An answer",
        MessageDirection::Outbound,
        MessageRole::Agent,
        CorrelationId::new(),
    )
    .external_conversation();
    use crate::adapters::persistence::thread_lock_test_support::*;
    let names = ["followup-ingress", "followup-publication"];
    let inbound = contender(&fixture.pool, names[0]).await;
    let publisher = contender(&fixture.pool, names[1]).await;
    let blocker = hold_first_thread(&fixture.pool, &first.thread_ids).await;
    let publication = async {
        let mut tx = publisher.pool().begin().await.unwrap();
        super::super::publish_task_reply_on(
            &mut tx,
            super::super::TaskReplyPublication {
                company_id: fixture.company_id,
                task_id: Some(first.task_ids[0]),
                message: &reply,
                also_in_threads: &[],
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    };
    let (followup, (), ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(
            async {
                wait_until_blocked(&fixture.pool, &[names[1]]).await;
                inbound.commit_inbound(followup).await
            },
            publication,
            release_when_both_wait(blocker, &fixture.pool, &names)
        )
    })
    .await
    .expect("reversed recipient order cannot deadlock with publication");
    let followup = followup.unwrap();
    assert_eq!(followup.thread_ids.len(), 2);
    assert_eq!(message_count(&fixture).await, 3);
    let sibling = fixture
        .persistence
        .get_thread_message(first.thread_ids[1], reply.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        sibling.entry_kind,
        crate::entities::message::ThreadEntryKind::Delegation
    );
    fixture.cleanup().await;
}

pub(super) async fn two_pipeline_request(fixture: &Fixture, rfc: &str) -> InboundCommitRequest {
    let second_channel = fixture.extra_channel("billing").await;
    let second_binding = fixture.email_binding_of(second_channel).await;
    let mut request = request(fixture, rfc, "Please answer from both teams").await;
    let mut associations = request.associations.into_inner();
    associations.push(ThreadAssociation {
        channel_id: second_channel,
        binding_id: second_binding,
        target: ThreadTarget::Create {
            subject: "Quick question".into(),
        },
        role: RecipientRole::Cc,
        step: PipelineStep::only(),
        principals: sender_principals(),
    });
    request.associations = BoundedVec::parse("thread associations", associations).unwrap();
    let mut tasks = request.tasks.into_inner();
    tasks.push(InboundTaskRequest {
        task_type: AGENT_DISPATCH.into(),
        targets: vec![InboundTaskTarget {
            channel_id: second_channel,
            role: RecipientRole::Cc,
        }],
    });
    request.tasks = BoundedVec::parse("inbound tasks", tasks).unwrap();
    request
}

#[tokio::test]
async fn two_pipeline_tasks_keep_their_targets_and_redelivery_order() {
    let Some(fixture) = Fixture::new("inbound_two_tasks").await else {
        return;
    };
    let rfc = format!("<two-tasks-{}@example.com>", fixture.suffix);
    let request = two_pipeline_request(&fixture, &rfc).await;
    let first = fixture
        .persistence
        .commit_inbound(request.clone())
        .await
        .unwrap();
    assert_eq!(first.task_ids.len(), 2);
    for (index, task_id) in first.task_ids.iter().enumerate() {
        let targets: Vec<StoredTaskTarget> = sqlx::query_as(
            "SELECT channel_id, thread_id, recipient_role FROM task_channel_targets \
             WHERE company_id = $1 AND task_id = $2 ORDER BY position",
        )
        .bind(fixture.company_id)
        .bind(task_id)
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(
            targets,
            vec![StoredTaskTarget {
                channel_id: request.tasks[index].targets[0].channel_id,
                thread_id: first.thread_ids[index],
                recipient_role: if index == 0 { "to" } else { "cc" }.to_owned(),
            }]
        );
    }
    let second = fixture.persistence.commit_inbound(request).await.unwrap();
    assert_eq!(second.disposition, CommitDisposition::Duplicate);
    assert_eq!(second.message_id, first.message_id);
    assert_eq!(second.thread_ids, first.thread_ids);
    assert_eq!(second.task_ids, first.task_ids);
    assert_eq!(task_count(&fixture).await, 2);
    assert_eq!(message_count(&fixture).await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn overlapping_task_channels_are_refused_before_any_inbound_write() {
    let Some(fixture) = Fixture::new("inbound_overlapping_tasks").await else {
        return;
    };
    let rfc = format!("<overlapping-tasks-{}@example.com>", fixture.suffix);
    let mut request = two_pipeline_request(&fixture, &rfc).await;
    // The repeated channel is a later target, so the first-channel unique key cannot catch it.
    let mut tasks = request.tasks.into_inner();
    let repeated = tasks[0].targets[0];
    tasks[1].targets.push(repeated);
    request.tasks = BoundedVec::parse("inbound tasks", tasks).unwrap();
    sqlx::query("DELETE FROM threads WHERE company_id = $1")
        .bind(fixture.company_id)
        .execute(&fixture.pool)
        .await
        .unwrap();

    assert!(fixture.persistence.commit_inbound(request).await.is_err());
    assert_eq!(message_count(&fixture).await, 0);
    assert_eq!(task_count(&fixture).await, 0);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM threads WHERE company_id = $1"
        )
        .await,
        0
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn enqueue_message_deduplicates_per_channel() {
    let Some(fixture) = Fixture::new("enqueue_per_channel").await else {
        return;
    };
    let second_channel = fixture.extra_channel("billing").await;
    let rfc = format!("<enqueue-{}@example.com>", fixture.suffix);
    let mut request = request(&fixture, &rfc, "Please answer").await;
    request.tasks = BoundedVec::empty();
    let committed = fixture.persistence.commit_inbound(request).await.unwrap();
    let new_task = |channel_id| NewTask {
        company_id: fixture.company_id,
        channel_id,
        thread_id: None,
        task_type: AGENT_DISPATCH.into(),
        payload: serde_json::json!({}),
        source: crate::entities::task::TaskSource::Message(committed.message_id),
        correlation_id: CorrelationId::new(),
        targets: Vec::new(),
    };
    let first = fixture
        .persistence
        .enqueue_task(new_task(fixture.channel_id))
        .await
        .unwrap();
    let duplicate = fixture
        .persistence
        .enqueue_task(new_task(fixture.channel_id))
        .await
        .unwrap();
    let other = fixture
        .persistence
        .enqueue_task(new_task(second_channel))
        .await
        .unwrap();
    assert_eq!(first.id, duplicate.id);
    assert_eq!(first.correlation_id, duplicate.correlation_id);
    assert_ne!(first.id, other.id);
    assert_eq!(task_count(&fixture).await, 2);
    fixture.cleanup().await;
}
