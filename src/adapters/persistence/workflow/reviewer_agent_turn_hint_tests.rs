//! Actual ordered agent preparation parks and reopens a job, without a final result.
use super::*;
use crate::adapters::persistence::workflow::tests::message_tests::support::member;
use crate::application::ports::account_deletion::HostAccountDeletion;
use crate::application::workflow::channel_reply::{ChannelReplies, ReplyApprovalPolicy};

struct QueuedTurn {
    history: HistoryThread,
    owner: ClaimedWorkflow,
    queued: ClaimedWorkflow,
}

async fn queued_turn() -> QueuedTurn {
    let history = Box::pin(HistoryThread::new()).await;
    let retired = member(&history.s.fixture, "admin").await;
    let principal = history.s.fixture.principal(retired).await;
    let mut reply =
        super::super::super::channel_reply_tests::selection(&history.s, history.binding, None)
            .await;
    reply.approval_policy = Some(ReplyApprovalPolicy::new(vec![principal], 60).unwrap());
    let p = history.s.p();
    p.configure_channel_reply(reply).await.unwrap();
    let owner = Box::pin(history.next()).await;
    let queued = Box::pin(history.next()).await;
    assert!(matches!(
        p.prepare_agent(&owner, policy()).await.unwrap(),
        AgentPreparationResult::Ready(_)
    ));
    let AgentPreparationResult::Parked(wait) = p.prepare_agent(&queued, policy()).await.unwrap()
    else {
        panic!("the real second agent turn must park behind the first");
    };
    assert!(p.confirm_suspension(queued.fence, wait).await.unwrap());
    p.delete_user_account(retired).await.unwrap();
    QueuedTurn {
        history,
        owner,
        queued,
    }
}

async fn snapshot(f: &QueuedTurn) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'runs',(SELECT jsonb_agg(to_jsonb(run) ORDER BY id) FROM workflow_runs AS run WHERE company_id=$1), \
         'executions',(SELECT jsonb_agg(to_jsonb(execution) ORDER BY id) FROM workflow_executions AS execution WHERE company_id=$1), \
         'jobs',(SELECT jsonb_agg(to_jsonb(job) ORDER BY id) FROM background_tasks AS job WHERE company_id=$1), \
         'waits',(SELECT jsonb_agg(to_jsonb(wait) ORDER BY id) FROM workflow_waits AS wait WHERE company_id=$1), \
         'turns',(SELECT jsonb_agg(to_jsonb(exchange) ORDER BY id) FROM agent_history_exchanges AS exchange WHERE company_id=$1), \
         'conversations',(SELECT jsonb_agg(to_jsonb(conversation) ORDER BY id) FROM agent_history_conversations AS conversation WHERE company_id=$1), \
         'events',(SELECT jsonb_agg(to_jsonb(event) ORDER BY run_id,sequence) FROM workflow_run_events AS event WHERE company_id=$1), \
         'proposals',(SELECT jsonb_agg(to_jsonb(proposal) ORDER BY id) FROM channel_reply_proposals AS proposal WHERE company_id=$1), \
         'receipts',(SELECT jsonb_agg(to_jsonb(receipt) ORDER BY run_id,execution_id,resource,reservation_key) FROM workflow_budget_receipts AS receipt WHERE company_id=$1), \
         'usage',(SELECT jsonb_agg(to_jsonb(usage) ORDER BY root_run_id) FROM workflow_root_budget_usage AS usage WHERE company_id=$1))",
    ).bind(f.queued.fence.scope.company.as_uuid()).fetch_one(f.history.s.pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_reviewer_follow_pending_agent_turn_ignores_unused_retired_final_reviewers() {
    let f = Box::pin(queued_turn()).await;
    let before = snapshot(&f).await;
    for _ in 0..2 {
        assert_eq!(
            f.history
                .s
                .p()
                .resume_wait(f.queued.fence.scope)
                .await
                .unwrap(),
            WaitProgress::Pending
        );
        assert_eq!(snapshot(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_ready_agent_turn_reopens_with_actor_account_locked() {
    let f = Box::pin(queued_turn()).await;
    Box::pin(cancel(
        &f.history,
        &f.owner,
        "release-unused-final-policy-turn",
    ))
    .await;
    let p = f.history.s.p();
    let scope = f.queued.fence.scope;
    let mut actor = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.history.s.owner().user_id())
        .execute(&mut *actor)
        .await
        .unwrap();
    let progress = tokio::time::timeout(Duration::from_secs(2), p.resume_wait(scope))
        .await
        .expect("an actual queued-turn continuation must finish while actor UPDATE is held")
        .unwrap();
    assert_eq!(progress, WaitProgress::Continued);
    let stored = snapshot(&f).await;
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(pointer(&f.history).await, Some(scope.execution.as_uuid()));
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Refused);
    assert_eq!(snapshot(&f).await, stored);
    actor.rollback().await.unwrap();
    let resumed = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), policy())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        p.prepare_agent(&resumed, policy()).await.unwrap(),
        AgentPreparationResult::Ready(_)
    ));
    let counts: (i32, i16) = sqlx::query_as(
        "SELECT retry_count,continuation_count FROM background_tasks WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.job.0)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        counts,
        (0, 1),
        "one continuation without a business retry or final result"
    );
}
