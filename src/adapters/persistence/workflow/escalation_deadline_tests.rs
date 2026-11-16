use super::*;
use crate::application::workflow::polling::WorkflowPolling;

async fn overdue(f: &GuardFixture) {
    sqlx::query(
        "UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes', \
         deadline=clock_timestamp()-interval '1 minute' WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .execute(f.p().pool())
    .await
    .unwrap();
}

#[tokio::test]
async fn workflow_escalation_trigger_deadline_requires_actual_expiry_and_delivers_one_notice() {
    let f = Box::pin(GuardFixture::new(Some(policy()))).await;
    let before = f.state().await;
    assert!(!f.p().expire_run(f.scope).await.unwrap());
    assert_eq!(
        f.state().await,
        before,
        "a future deadline does not escalate"
    );
    overdue(&f).await;
    assert!(f.p().expire_run(f.scope).await.unwrap());
    let source: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('run',run_id,'execution',execution_id,'step',step_id,'reason',reason_code) \
         FROM workflow_escalation_cases WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(
        source,
        json!({"run":f.scope.run.as_uuid(),"execution":f.scope.execution.as_uuid(),
        "step":"start","reason":"workflow.run_deadline"})
    );
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    let after = f.state().await;
    assert_eq!(
        after,
        json!({"state":"failed","runs":1,"cases":1,"controls":2,"hops":1,"notices":1,"guard_events":0})
    );
    assert!(!f.p().expire_run(f.scope).await.unwrap());
    assert_eq!(f.state().await, after);
    let facts: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object( \
          'audit',(SELECT count(*) FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND event_kind='run_deadline_expired'), \
          'outbox',(SELECT count(*) FROM workflow_escalation_person_receipts WHERE company_id=$1 AND outcome='email_queued'))",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(facts, json!({"audit":1,"outbox":1}));
}

#[tokio::test]
async fn workflow_escalation_competing_deadline_owners_create_one_terminal_source() {
    let f = Box::pin(GuardFixture::new(Some(policy()))).await;
    overdue(&f).await;
    let barrier = tokio::sync::Barrier::new(3);
    let first = async {
        barrier.wait().await;
        f.p().expire_run(f.scope).await.unwrap()
    };
    let second = async {
        barrier.wait().await;
        f.p().expire_run(f.scope).await.unwrap()
    };
    let (first, second, _) = tokio::join!(first, second, barrier.wait());
    assert_ne!(
        first, second,
        "one actual deadline owner commits the failure"
    );
    assert_eq!(
        f.state().await,
        json!({"state":"failed","runs":1,"cases":1,"controls":1,"hops":0,"notices":0,"guard_events":0})
    );
}

#[tokio::test]
async fn workflow_escalation_trigger_deadline_case_failure_rolls_back_expiry_and_job() {
    let f = Box::pin(GuardFixture::new(Some(policy()))).await;
    overdue(&f).await;
    let before = f.state().await;
    sqlx::raw_sql(
        "CREATE FUNCTION reject_deadline_case_test() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN PERFORM 1/0; RETURN NEW; END $$; \
         CREATE TRIGGER reject_deadline_case_test BEFORE INSERT ON workflow_escalation_cases \
         FOR EACH ROW EXECUTE FUNCTION reject_deadline_case_test()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    let error = f.p().expire_run(f.scope).await.unwrap_err();
    assert!(
        format!("{error:?}").contains("division by zero"),
        "{error:?}"
    );
    assert_eq!(f.state().await, before);
    let pending: bool = sqlx::query_scalar(
        "SELECT status='pending' AND NOT EXISTS(SELECT 1 FROM workflow_run_events \
         WHERE company_id=$1 AND run_id=$2 AND event_kind='run_deadline_expired') \
         FROM background_tasks WHERE company_id=$1 AND id=$3",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .bind(f.scope.job.0)
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert!(
        pending,
        "a case error rolls back both job expiry and its audit"
    );
    sqlx::raw_sql(
        "DROP TRIGGER reject_deadline_case_test ON workflow_escalation_cases; DROP FUNCTION reject_deadline_case_test()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    assert!(f.p().expire_run(f.scope).await.unwrap());
}
