use super::*;
use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};

async fn parked() -> GuardFixture {
    let people = Box::pin(PeopleFixture::new(1)).await;
    let mut source: Value =
        serde_json::from_str(&registry::example("wait.event").unwrap().source).unwrap();
    source["input_schema"] = json!(true);
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()});
    let version = republish(&people, source).await;
    let f = Box::pin(GuardFixture::admit(people, Some(policy()), Some(version))).await;
    assert!(f.p().park_wait(f.scope).await.unwrap().is_some());
    assert_eq!(
        f.p().resume_wait(f.scope).await.unwrap(),
        WaitProgress::Pending
    );
    f
}

async fn overdue(f: &GuardFixture) {
    sqlx::query(
        "UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds', \
         deadline=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND run_id=$2 AND state='waiting'",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .execute(f.p().pool())
    .await
    .unwrap();
}

#[tokio::test]
async fn workflow_escalation_trigger_wait_deadline_preserves_real_source_and_delivers_once() {
    let f = Box::pin(parked()).await;
    overdue(&f).await;
    assert_eq!(f.p().sweep_waits(128).await.unwrap(), 1);
    let source: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('run',run_id,'execution',execution_id,'step',step_id, \
         'reason',reason_code,'department',origin_department_id) FROM workflow_escalation_cases WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(
        source,
        json!({"run":f.scope.run.as_uuid(),"execution":f.scope.execution.as_uuid(),
        "step":"start","reason":"workflow.wait_deadline","department":f.people.role.department})
    );
    assert_eq!(f.p().sweep_waits(128).await.unwrap(), 0);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    assert_eq!(
        f.state().await,
        json!({"state":"failed","runs":1,"cases":1,"controls":2,"hops":1,"notices":1,"guard_events":0})
    );
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_escalation_person_receipts WHERE company_id=$1 AND outcome='email_queued'",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(receipts, 1);
}

#[tokio::test]
async fn workflow_escalation_trigger_wait_deadline_case_failure_rolls_back_run_and_wait() {
    let f = Box::pin(parked()).await;
    overdue(&f).await;
    let before = f.state().await;
    sqlx::raw_sql(
        "CREATE FUNCTION reject_wait_case_test() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN PERFORM 1/0; RETURN NEW; END $$; \
         CREATE TRIGGER reject_wait_case_test BEFORE INSERT ON workflow_escalation_cases \
         FOR EACH ROW EXECUTE FUNCTION reject_wait_case_test()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    let error = f.p().sweep_waits(128).await.unwrap_err();
    assert!(
        format!("{error:?}").contains("division by zero"),
        "{error:?}"
    );
    assert_eq!(f.state().await, before);
    let waiting: bool = sqlx::query_scalar(
        "SELECT state='waiting' FROM workflow_waits WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert!(
        waiting,
        "the same source transaction rolled back the wait's expiry"
    );
    sqlx::raw_sql(
        "DROP TRIGGER reject_wait_case_test ON workflow_escalation_cases; DROP FUNCTION reject_wait_case_test()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    assert_eq!(f.p().sweep_waits(128).await.unwrap(), 1);
}
