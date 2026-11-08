use super::*;
use crate::adapters::persistence::workflow::{channel_reply_refusal, completion_job};
use crate::domain::workflow::KnowledgeRefusal;
use crate::entities::access_level::AccessLevel;

#[tokio::test]
async fn channel_reply_knowledge_pending_owner_keeps_cause_after_real_token_denial() {
    let f = Box::pin(configured()).await;
    let p = f.model.native.fixture.persistence();
    // Existing native token fixture supplies a pending descendant beside its ancestor.
    // This boundary case proves owner priority; the complete release journey is separate.
    let scope = refusal::child(&f, f.model.write.fence.scope).await;
    exceed(&f).await;
    p.submit_decision(command(p, f.model.write.fence.scope, f.reviewer, "reject").await)
        .await
        .unwrap();
    let before = job(&f, scope).await;
    let mut tx = p.pool().begin().await.unwrap();
    let gate = crate::adapters::persistence::workflow::token_budget::progression_on(&mut tx, scope)
        .await
        .unwrap();
    assert_eq!(gate.failure_code(), Some("workflow.tokens_rejected"));
    sqlx::query("SELECT run.id FROM workflow_runs AS run JOIN workflow_executions AS execution
        ON execution.company_id=run.company_id AND execution.run_id=run.id
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id
        WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3 AND job.id=$4 FOR UPDATE OF run,execution,job")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0)
        .fetch_one(&mut *tx).await.unwrap();
    channel_reply_refusal::settle_on(
        &mut tx,
        scope,
        completion_job::CompletionOwner::Pure,
        Some(KnowledgeRefusal {
            content: AccessLevel::L5,
            destination: AccessLevel::PUBLIC,
        }),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let after = job(&f, scope).await;
    assert_eq!(after["status"], "failed");
    for key in ["retry_count", "attempt_ordinal", "continuation_count"] {
        assert_eq!(before[key], after[key]);
    }
    let codes: Vec<String> = sqlx::query_scalar(
        "SELECT event_kind FROM workflow_run_events WHERE company_id=$1 AND run_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert!(
        codes
            .iter()
            .any(|code| code == "knowledge.audience_below_content")
    );
    assert!(!codes.iter().any(|code| code == "workflow.tokens_rejected"));
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1")
        .bind(scope.job.0)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 0);
}
