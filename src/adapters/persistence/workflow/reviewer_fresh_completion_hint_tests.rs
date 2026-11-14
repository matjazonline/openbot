//! Handler results only need final reviewers when their actual completion creates that effect.
use super::*;
use crate::domain::workflow::{StepId, TransitionTarget};

enum ResultRoute {
    Final,
    Next,
}

struct FreshIo {
    configured: ConfiguredFinal,
    result: FencedWorkflowResult,
}

fn source(route: ResultRoute) -> Value {
    let mut source = completion_source(CompletionFamily::Fenced);
    if matches!(route, ResultRoute::Next) {
        source["steps"]["start"]["routes"]["success"] = json!("next");
        let mut next: Value =
            serde_json::from_str(&registry::example("channel.no_reply").unwrap().source).unwrap();
        next["steps"]["start"]["with"] = json!({
            "value":{"literal":{"finished":true}},"output_schema":{"literal":true}
        });
        source["steps"]["next"] = next["steps"]["start"].clone();
    }
    source
}

async fn fresh_io(route: ResultRoute) -> FreshIo {
    let configured = Box::pin(configured_final(source(route))).await;
    let f = &configured.fixture;
    let policy = LeasePolicy::new(std::time::Duration::from_secs(60)).unwrap();
    let claim =
        f.p.claim_io(f.scope, WorkflowWorkerId(Uuid::new_v4()), policy)
            .await
            .unwrap()
            .unwrap();
    f.p.delete_user_account(configured.reviewer).await.unwrap();
    FreshIo {
        configured,
        result: FencedWorkflowResult {
            fence: claim.fence,
            output: json!({"items":[],"token_count":0,"omitted":0}),
            policy,
        },
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_nonterminal_io_completion_ignores_unused_retired_final_reviewers()
{
    let io = Box::pin(fresh_io(ResultRoute::Next)).await;
    let f = &io.configured.fixture;
    let before = state(f).await;
    let mut stale = io.result.clone();
    stale.fence.generation =
        crate::application::workflow::lease::WorkflowGeneration(Uuid::new_v4());
    assert!(f.p.complete_io(stale).await.unwrap().is_none());
    assert_eq!(
        state(f).await,
        before,
        "a stale result must not change any stored result or ownership"
    );
    let done = f.p.complete_io(io.result.clone()).await.unwrap().unwrap();
    assert_eq!(done.disposition, CommitDisposition::Committed);
    assert_eq!(done.output, io.result.output);
    assert_eq!(
        done.target,
        TransitionTarget::Step(StepId::parse("next").unwrap())
    );
    assert!(done.successor.is_some());
    let after = state(f).await;
    assert_eq!(after["run"]["state"], "running");
    assert_eq!(after["proposals"], Value::Null);
    assert_eq!(after["reply"], Value::Null);
    let mut expected = done;
    expected.disposition = CommitDisposition::Replayed;
    assert_eq!(
        f.p.complete_io(io.result.clone()).await.unwrap().unwrap(),
        expected
    );
    assert_eq!(state(f).await, after);
}

#[tokio::test]
async fn workflow_reviewer_follow_invalid_io_result_retires_without_unused_final_reviewers() {
    let mut io = Box::pin(fresh_io(ResultRoute::Final)).await;
    let f = &io.configured.fixture;
    io.result.output = json!({"invalid":"received output"});
    assert!(f.p.complete_io(io.result.clone()).await.unwrap().is_none());
    let after = state(f).await;
    assert_eq!(after["run"]["state"], "failed");
    assert_eq!(after["proposals"], Value::Null);
    assert_eq!(after["reply"], Value::Null);
    let failure: String = sqlx::query_scalar(
        "SELECT workflow_failure_code FROM task_attempts WHERE task_id=$1 AND attempt_number=$2 AND execution_generation=$3",
    )
    .bind(io.result.fence.scope.job.0).bind(io.result.fence.attempt.0).bind(io.result.fence.generation.0)
    .fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(failure, "workflow.invalid_result");
    assert!(f.p.complete_io(io.result.clone()).await.unwrap().is_none());
    assert_eq!(state(f).await, after);
}

#[tokio::test]
async fn workflow_reviewer_follow_fresh_final_io_completion_requires_current_final_reviewers() {
    let io = Box::pin(fresh_io(ResultRoute::Final)).await;
    let f = &io.configured.fixture;
    let before = state(f).await;
    assert!(matches!(
        f.p.complete_io(io.result.clone()).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(state(f).await, before);
    assert!(
        f.p.validate_io(io.result.fence, io.result.policy)
            .await
            .unwrap(),
        "current-authority refusal must preserve the live original fence"
    );
    assert_eq!(state(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_stale_final_io_result_ignores_unused_retired_final_reviewers() {
    let io = Box::pin(fresh_io(ResultRoute::Final)).await;
    let f = &io.configured.fixture;
    let before = state(f).await;
    let mut stale = io.result.clone();
    stale.fence.generation =
        crate::application::workflow::lease::WorkflowGeneration(Uuid::new_v4());
    assert!(f.p.complete_io(stale).await.unwrap().is_none());
    assert_eq!(state(f).await, before);
    assert!(
        f.p.validate_io(io.result.fence, io.result.policy)
            .await
            .unwrap()
    );
    assert_eq!(
        state(f).await,
        before,
        "the live owner and stored result must remain unchanged"
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_expired_final_io_result_ignores_unused_retired_final_reviewers() {
    let io = Box::pin(fresh_io(ResultRoute::Final)).await;
    let f = &io.configured.fixture;
    // Lease expiry is mutable runtime ownership, unlike the immutable admission deadline.
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()-interval '1 microsecond' WHERE company_id=$1 AND id=$2 AND worker_id=$3 AND execution_generation=$4")
        .bind(f.scope.company.as_uuid()).bind(f.scope.job.0).bind(io.result.fence.worker.0).bind(io.result.fence.generation.0)
        .execute(f.p.pool()).await.unwrap();
    let expired: bool = sqlx::query_scalar("SELECT lock_expires_at<clock_timestamp() FROM background_tasks WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.job.0).fetch_one(f.p.pool()).await.unwrap();
    assert!(expired);
    let before = state(f).await;
    assert!(f.p.complete_io(io.result.clone()).await.unwrap().is_none());
    assert_eq!(
        state(f).await,
        before,
        "expired ownership cannot commit any new effect"
    );
}
