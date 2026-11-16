//! A pure batch's actual route and boundary determine its final-response accounts.
use super::*;
use crate::app_error::AppResult;
use crate::domain::workflow::ResponseDisposition;
use std::time::Duration;
use std::{future::Future, pin::Pin};

async fn snapshot(f: &FinalFixture) -> Value {
    let mut snapshot = state(f).await;
    snapshot["budgets"] = sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'receipts',(SELECT jsonb_agg(to_jsonb(receipt) ORDER BY execution_id,resource,reservation_key) FROM workflow_budget_receipts AS receipt WHERE company_id=$1 AND run_id=$2), \
         'usage',(SELECT to_jsonb(usage) FROM workflow_root_budget_usage AS usage WHERE company_id=$1 AND root_run_id=(SELECT root_run_id FROM workflow_run_budgets WHERE company_id=$1 AND run_id=$2)))",
    )
    .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid())
    .fetch_one(f.p.pool()).await.unwrap();
    snapshot
}

#[tokio::test]
async fn workflow_reviewer_follow_nonfinal_pure_batch_ignores_unused_retired_final_reviewers() {
    let mut source = completion_source(CompletionFamily::Pure);
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"data.map","with":{
        "value":{"literal":43},"output_schema":{"literal":true}},
        "routes":{"success":"$end"}});
    let configured = Box::pin(configured_final(source)).await;
    let f = &configured.fixture;
    f.p.delete_user_account(configured.reviewer).await.unwrap();
    let budget = BatchBudget::new(1, Duration::from_secs(1)).unwrap();
    let done = f.p.advance_pure(f.scope, budget).await.unwrap();
    assert_eq!(done.disposition, BatchDisposition::Yielded);
    assert_eq!(done.completed, 1);
    let successor = done.continuation.unwrap();
    let stored = snapshot(f).await;
    assert_eq!(stored["run"]["state"], "running");
    assert_eq!(
        stored["executions"][0]["committed_output"],
        json!({"result":42})
    );
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(stored["reply"], Value::Null);
    let replay = f.p.advance_pure(f.scope, budget).await.unwrap();
    assert_eq!(replay.disposition, BatchDisposition::Replay);
    assert_eq!(replay.continuation, Some(successor));
    assert_eq!(snapshot(f).await, stored);
    assert!(matches!(
        f.p.advance_pure(successor, budget).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(snapshot(f).await, stored);
}

#[tokio::test]
async fn workflow_reviewer_follow_suppressed_pure_batch_ignores_unused_retired_final_reviewers() {
    let configured = Box::pin(configured_final(final_source("channel.no_reply"))).await;
    let f = &configured.fixture;
    f.p.delete_user_account(configured.reviewer).await.unwrap();
    let done = f.advance().await;
    assert_eq!(done.disposition, BatchDisposition::Completed);
    assert_eq!(done.completed, 1);
    assert_eq!(
        done.response_disposition,
        Some(ResponseDisposition::Suppressed)
    );
    let stored = snapshot(f).await;
    assert_eq!(stored["run"]["state"], "succeeded");
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(stored["reply"]["state"], "suppressed");
    assert_eq!(
        stored["executions"][0]["response_disposition"],
        "suppressed"
    );
    assert_eq!(f.advance().await.disposition, BatchDisposition::Replay);
    assert_eq!(snapshot(f).await, stored);
}

#[tokio::test]
async fn workflow_reviewer_follow_io_boundary_batch_ignores_unused_retired_final_reviewers() {
    let configured = Box::pin(configured_final(completion_source(
        CompletionFamily::Fenced,
    )))
    .await;
    let f = &configured.fixture;
    f.p.delete_user_account(configured.reviewer).await.unwrap();
    let before = snapshot(f).await;
    for _ in 0..2 {
        let boundary = f.advance().await;
        assert_eq!(boundary.disposition, BatchDisposition::Boundary);
        assert_eq!(boundary.completed, 0);
        assert_eq!(boundary.continuation, Some(f.scope));
        assert_eq!(snapshot(f).await, before);
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_fresh_final_pure_batch_requires_current_final_reviewers() {
    let configured = Box::pin(configured_final(completion_source(CompletionFamily::Pure))).await;
    let f = &configured.fixture;
    f.p.delete_user_account(configured.reviewer).await.unwrap();
    let before = snapshot(f).await;
    assert!(matches!(
        f.p.advance_pure(
            f.scope,
            BatchBudget::new(4, Duration::from_secs(1)).unwrap()
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(snapshot(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_later_final_refusal_rolls_back_all_pure_preparation() {
    let mut source = completion_source(CompletionFamily::Pure);
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"data.map","with":{
        "value":{"literal":43},"output_schema":{"literal":true}},
        "routes":{"success":"$end"}});
    let configured = Box::pin(configured_final(source)).await;
    let f = &configured.fixture;
    f.p.delete_user_account(configured.reviewer).await.unwrap();
    let before = snapshot(f).await;
    assert!(matches!(
        f.p.advance_pure(
            f.scope,
            BatchBudget::new(4, Duration::from_secs(1)).unwrap()
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        snapshot(f).await,
        before,
        "no earlier pure output, successor, event, receipt or debit may survive a required final-authority refusal"
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_final_batch_replanning_commits_one_activation_and_proposal() {
    let configured = Box::pin(configured_final(completion_source(CompletionFamily::Pure))).await;
    let f = &configured.fixture;
    let parked = f.advance().await;
    assert_eq!(parked.disposition, BatchDisposition::Deferred);
    assert_eq!(parked.completed, 0);
    let waiting = snapshot(f).await;
    assert_eq!(waiting["run"]["state"], "waiting");
    assert_eq!(waiting["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(waiting["budgets"]["usage"]["activations"], 1);
    assert_eq!(waiting["budgets"]["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(f.advance().await.disposition, BatchDisposition::Deferred);
    assert_eq!(snapshot(f).await, waiting);
    let mut command = f.command("approve").await;
    command.actor = WorkflowActor::authenticated(configured.reviewer).unwrap();
    f.p.submit_decision(command).await.unwrap();
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
    let stored = snapshot(f).await;
    assert_eq!(stored["run"]["state"], "succeeded");
    assert_eq!(stored["reply"]["state"], "pending");
    assert_eq!(stored["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(stored["budgets"], waiting["budgets"]);
    assert_eq!(f.advance().await.disposition, BatchDisposition::Replay);
    assert_eq!(snapshot(f).await, stored);
}

struct PreparationOwners {
    batch: i32,
    retirement: i32,
}

async fn queued_retirement<Batch, Retirement>(
    pool: &sqlx::PgPool,
    blocker: i32,
    mut batch: Pin<&mut Batch>,
    mut retirement: Pin<&mut Retirement>,
) -> PreparationOwners
where
    Batch: Future<Output = AppResult<BatchResult>>,
    Retirement: Future,
{
    let batch_pid = tokio::select! {
        _ = batch.as_mut() => panic!("the actual batch escaped its execution gate"),
        pid = blocked_by(pool, blocker, "%FROM workflow_executions%FOR UPDATE%") => pid,
    };
    let retirement_pid = tokio::select! {
        _ = batch.as_mut() => panic!("preparation must still own company before its execution gate"),
        _ = retirement.as_mut() => panic!("actual retirement bypassed the preparation's company ownership"),
        pid = blocked_by(pool, batch_pid, "%companies%FOR NO KEY UPDATE%") => pid,
    };
    PreparationOwners {
        batch: batch_pid,
        retirement: retirement_pid,
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_final_batch_rolls_back_company_before_retiring_reviewer_lock() {
    let configured = Box::pin(configured_final(completion_source(CompletionFamily::Pure))).await;
    let f = &configured.fixture;
    let before = snapshot(f).await;
    let mut blocker = f.p.pool().begin().await.unwrap();
    let blocker_pid = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.execution.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let batch = f.p.advance_pure(
        f.scope,
        BatchBudget::new(4, Duration::from_secs(1)).unwrap(),
    );
    let retirement = f.p.delete_user_account(configured.reviewer);
    tokio::pin!(batch, retirement);
    // Box concurrent observation to preserve the stock test-thread stack budget.
    let owners = Box::pin(queued_retirement(
        f.p.pool(),
        blocker_pid,
        batch.as_mut(),
        retirement.as_mut(),
    ))
    .await;
    assert_ne!(owners.batch, owners.retirement);
    blocker.rollback().await.unwrap();
    let (outcome, retired) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(batch.as_mut(), retirement.as_mut())
    })
    .await
    .expect("preparation must release company before waiting on the retiring account");
    retired.unwrap();
    assert!(matches!(outcome, Err(AppError::NotFound(_))), "{outcome:?}");
    let present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
        .bind(configured.reviewer)
        .fetch_one(f.p.pool())
        .await
        .unwrap();
    assert!(!present, "actual account retirement must commit");
    assert_eq!(
        snapshot(f).await,
        before,
        "no preparation receipt, output, event or proposal may survive refusal"
    );
}
