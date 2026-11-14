use super::*;
use crate::application::workflow::batch::{BatchBudget, BatchDisposition, WorkflowBatch};

#[tokio::test]
async fn workflow_escalation_trigger_pure_invalid_output_enqueues_one_actual_case() {
    let f = Box::pin(failed_pure()).await;
    let result = f
        .p()
        .advance_pure(
            f.scope,
            BatchBudget::new(8, std::time::Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.disposition, BatchDisposition::Failed);
    assert_eq!(f.state().await["state"], "failed");
    assert_eq!(
        f.state().await["cases"],
        1,
        "the terminal transaction owns the escalation event"
    );
    assert_eq!(f.state().await["controls"], 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    assert_eq!(f.state().await["cases"], 1);
    assert_eq!(f.state().await["hops"], 1);
    assert_eq!(f.state().await["notices"], 1);
}

#[tokio::test]
async fn workflow_escalation_trigger_pure_case_failure_rolls_back_terminal_and_job() {
    let f = Box::pin(failed_pure()).await;
    let before = f.state().await;
    sqlx::raw_sql(
        "CREATE FUNCTION reject_pure_case() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN PERFORM 1/0; RETURN NEW; END $$; \
        CREATE TRIGGER reject_pure_case BEFORE INSERT ON workflow_escalation_cases \
        FOR EACH ROW EXECUTE FUNCTION reject_pure_case();",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    let budget = BatchBudget::new(8, std::time::Duration::from_secs(1)).unwrap();
    let error = f.p().advance_pure(f.scope, budget).await.unwrap_err();
    assert!(
        format!("{error:?}").contains("division by zero"),
        "{error:?}"
    );
    assert_eq!(f.state().await, before);
    let pending: bool =
        sqlx::query_scalar("SELECT status='pending' FROM background_tasks WHERE id=$1")
            .bind(f.scope.job.0)
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    assert!(
        pending,
        "the terminal job belongs to the same rolled-back transaction"
    );
    sqlx::query("DROP TRIGGER reject_pure_case ON workflow_escalation_cases")
        .execute(f.p().pool())
        .await
        .unwrap();
    assert_eq!(
        f.p()
            .advance_pure(f.scope, budget)
            .await
            .unwrap()
            .disposition,
        BatchDisposition::Failed
    );
    assert_eq!(f.state().await["cases"], 1);
}

#[tokio::test]
async fn workflow_escalation_trigger_authored_pure_error_route_keeps_the_ladder_suppressed() {
    let mut source = invalid_pure_source();
    source["steps"]["start"]["routes"]["error"] = json!("$end");
    let f = Box::pin(pure_fixture(source)).await;
    let result = f
        .p()
        .advance_pure(
            f.scope,
            BatchBudget::new(8, std::time::Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.disposition, BatchDisposition::Failed);
    let route: String = sqlx::query_scalar(
        "SELECT committed_route FROM workflow_executions WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.execution.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(
        route, "final_error",
        "the actual authored handler accepted the failure"
    );
    assert_eq!(f.state().await["cases"], 0);
    assert_eq!(f.state().await["controls"], 0);
    assert_eq!(f.worker().drain_once().await.unwrap(), 0);
}

#[tokio::test]
async fn workflow_escalation_source_rejects_a_forged_cause_step_or_terminal_execution() {
    let f = Box::pin(failed_pure()).await;
    let result = f
        .p()
        .advance_pure(
            f.scope,
            BatchBudget::new(8, std::time::Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.disposition, BatchDisposition::Failed);
    assert_eq!(f.state().await["state"], "failed");
    let actual: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 \
         AND execution_id=$3 AND event_kind='workflow.invalid_output')",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .bind(f.scope.execution.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert!(
        actual,
        "the production batch persisted the actual typed terminal cause"
    );

    for field in ["reason", "step", "execution", "role"] {
        let error = insert_case(&f, Some(field)).await.unwrap_err();
        let error = error.as_database_error().unwrap();
        assert_eq!(error.code().as_deref(), Some("23514"), "{field}");
        assert_eq!(
            error.constraint(),
            Some("workflow_escalation_case_guard"),
            "{field}"
        );
    }
    insert_case(&f, None).await.unwrap();
    assert_eq!(f.state().await["cases"], 1);
}

async fn failed_pure() -> GuardFixture {
    Box::pin(pure_fixture(invalid_pure_source())).await
}

fn invalid_pure_source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["input_schema"] = json!(true);
    source["output_schema"] = json!({"type":"string"});
    source["steps"]["start"]["with"]["value"] = json!({"literal":1});
    source["steps"]["start"]["with"]["output_schema"] = json!({"literal":true});
    source["steps"]["start"]["routes"]
        .as_object_mut()
        .unwrap()
        .remove("error");
    source
}

async fn pure_fixture(source: Value) -> GuardFixture {
    let people = Box::pin(PeopleFixture::new(1)).await;
    let version = republish(&people, source).await;
    Box::pin(GuardFixture::admit(people, Some(policy()), Some(version))).await
}

async fn insert_case(
    f: &GuardFixture,
    field: Option<&str>,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    let reason = if field == Some("reason") {
        "workflow.forged_failure"
    } else {
        "workflow.invalid_output"
    };
    let step = if field == Some("step") {
        "not_the_step"
    } else {
        "start"
    };
    let execution = (field != Some("execution")).then_some(f.scope.execution.as_uuid());
    sqlx::query(
        "INSERT INTO workflow_escalation_cases \
         (company_id,id,source_kind,run_id,execution_id,binding_id,binding_revision, \
          origin_department_id,policy,reason_code,step_id,role) \
         SELECT run.company_id,gen_random_uuid(),'run',run.id,$3,run.binding_id,run.binding_revision, \
                manifest.role_context_department_id,run.escalation,$4,$5,$6 \
         FROM workflow_runs AS run JOIN run_dependency_manifests AS ownership \
           ON ownership.company_id=run.company_id AND ownership.run_id=run.id \
         JOIN agent_dependency_manifests AS manifest ON manifest.company_id=ownership.company_id AND manifest.id=ownership.manifest_id \
         WHERE run.company_id=$1 AND run.id=$2 \
         ON CONFLICT (company_id,run_id) WHERE run_id IS NOT NULL DO NOTHING",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .bind(execution)
    .bind(reason)
    .bind(step)
    .bind((field == Some("role")).then_some("reviewer"))
    .execute(f.p().pool())
    .await
}
