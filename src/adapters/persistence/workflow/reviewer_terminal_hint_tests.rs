//! Terminal owners keep their actual scope/fence guards without unused reply recipients.
use super::*;
use crate::application::workflow::guard::*;

async fn assert_failed_once(io: &ReplyIo, event: &str) {
    let f = &io.pending.fixture;
    let scope = io.claim.fence.scope;
    let state: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('run',run.state,'terminal',run.terminal_execution_id, \
         'job',(SELECT status FROM background_tasks WHERE company_id=run.company_id AND id=$3), \
         'audits',(SELECT count(*) FROM workflow_run_events WHERE company_id=run.company_id AND run_id=run.id AND event_kind=$4), \
         'finals',(SELECT count(*) FROM channel_reply_proposals WHERE company_id=run.company_id AND run_id=run.id)) \
         FROM workflow_runs AS run WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.job.0)
    .bind(event)
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(
        state,
        json!({"run":"failed","terminal":scope.execution.as_uuid(),
        "job":"failed","audits":1,"finals":0})
    );
    assert!(!f.p.validate_io(io.claim.fence, io.policy).await.unwrap());
}

#[tokio::test]
async fn workflow_reviewer_follow_reply_expiry_ignores_unused_retired_final_reviewers() {
    let io = Box::pin(reply_io_with_deadline(
        ReviewerRetirement::AfterClaim,
        ReplyDeadline::Short,
    ))
    .await;
    let f = &io.pending.fixture;
    let sealed: bool = sqlx::query_scalar("SELECT deadline=created_at+interval '2 seconds' AND EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='workflow_runs'::regclass AND tgname='workflow_run_snapshot_immutable' AND tgenabled='O') FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert!(
        sealed,
        "the short lifetime was admitted with the production snapshot guard enabled"
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT deadline<=clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
                .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
            if expired { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("the actual immutable database deadline passes");
    assert!(
        !f.p.expire_run(f.scope).await.unwrap(),
        "the completed business execution cannot expire the current IO owner"
    );
    assert!(f.p.expire_run(io.claim.fence.scope).await.unwrap());
    assert!(!f.p.expire_run(io.claim.fence.scope).await.unwrap());
    assert_failed_once(&io, "run_deadline_expired").await;
}

async fn rejected_activation_write(
    io: &ReplyIo,
) -> crate::application::app_error::WorkflowGuardViolation {
    let scope = io.claim.fence.scope;
    let error = sqlx::query(
        "UPDATE workflow_executions SET frozen_inputs=frozen_inputs || '{\"guard_probe\":true}'::jsonb \
         WHERE company_id=$1 AND run_id=$2 AND id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .execute(io.pending.fixture.p.pool())
    .await
    .expect_err("a real SQL guard rejects mutation of an activated execution");
    let database = error
        .as_database_error()
        .expect("actual database guard error");
    assert_eq!(database.code().as_deref(), Some("23514"));
    let guard = database
        .constraint()
        .unwrap()
        .strip_prefix("workflow_guard:")
        .unwrap()
        .to_owned();
    let AppError::WorkflowGuard(violation) = AppError::from(error) else {
        panic!("the actual database guard must enter the typed terminal path")
    };
    assert_eq!(violation.guard(), guard);
    violation
}

#[tokio::test]
async fn workflow_reviewer_follow_reply_guard_failure_ignores_unused_retired_final_reviewers() {
    let io = Box::pin(reply_io(ReviewerRetirement::AfterClaim)).await;
    let f = &io.pending.fixture;
    let violation = rejected_activation_write(&io).await;
    let event = format!("guard_violation:{}", violation.guard());
    let failure = GuardFailure {
        scope: io.claim.fence.scope,
        violation,
        site: GuardSite::Step,
        fence: Some(io.claim.fence),
    };
    let mut stale = failure.clone();
    stale.fence.as_mut().unwrap().generation = WorkflowGeneration(Uuid::new_v4());
    assert_eq!(
        f.p.fail_run_on_guard(stale).await.unwrap(),
        GuardFailureOutcome::Deferred
    );
    assert!(f.p.validate_io(io.claim.fence, io.policy).await.unwrap());
    assert_eq!(
        f.p.fail_run_on_guard(failure.clone()).await.unwrap(),
        GuardFailureOutcome::Failed
    );
    assert_eq!(
        f.p.fail_run_on_guard(failure).await.unwrap(),
        GuardFailureOutcome::Inactive
    );
    assert_failed_once(&io, &event).await;
}
