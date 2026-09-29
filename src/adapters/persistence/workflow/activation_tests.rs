use super::*;
use crate::adapters::persistence::workflow::activation::activate_on;
use crate::application::workflow::activation::*;
use tokio::sync::Barrier;

async fn fixture() -> (AdmissionFixture, ActivationRequest) {
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["input_schema"] =
        json!({"type":"object","required":["value"],"properties":{"value":true}});
    source["steps"]["start"]["with"]["value"] = json!({"ref":"/input/value"});
    source["steps"]["start"]["with"]["output_schema"] = json!({"literal":true});
    source["steps"]["start"]["routes"] = json!({"success":"next"});
    source["steps"]["next"] = source["steps"]["start"].clone();
    source["steps"]["next"]["with"]["value"] = json!({"ref":"/steps/start/output"});
    source["steps"]["next"]["routes"] = json!({"success":"$end"});
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(source).await).await;
    let command = f.prepare(f.request("activation", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let job =
        sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id = $1")
            .bind(command.first_execution_id().as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    let request = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    (f, request)
}

async fn activated(f: &AdmissionFixture, request: ActivationRequest) -> bool {
    sqlx::query_scalar("SELECT activated_at IS NOT NULL FROM workflow_executions WHERE id = $1")
        .bind(request.execution.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

async fn assert_charged(f: &AdmissionFixture, count: i64) {
    let actual: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(activations),0)::bigint FROM workflow_root_budget_usage",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_budget_receipts WHERE resource='activation'",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(actual, count);
    assert_eq!(receipts, count);
}

#[tokio::test]
async fn workflow_activation_competing_connections_freeze_once() {
    let (f, request) = fixture().await;
    let barrier = Barrier::new(2);
    let competitor = || async {
        let mut connection = f.persistence().pool().acquire().await.unwrap();
        let mut tx = sqlx::Connection::begin(&mut *connection).await.unwrap();
        barrier.wait().await;
        let result = activate_on(&mut tx, request).await.unwrap().unwrap();
        tx.commit().await.unwrap();
        result
    };
    let (first, second) = tokio::join!(competitor(), competitor());
    assert_eq!(first, second);
    assert_eq!(first.inputs["value"], 1);
    assert_eq!(first.ordinal, 1);
    assert!(activated(&f, request).await);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE run_id = $1")
            .bind(request.run.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
    assert_charged(&f, 1).await;
}

#[tokio::test]
async fn workflow_activation_abort_and_lost_ack_reuse_snapshot_across_attempts() {
    let (f, request) = fixture().await;
    // A worker dies with an uncommitted activation: dropping rolls back the write.
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let original = activate_on(&mut tx, request).await.unwrap().unwrap();
    drop(tx);
    assert!(!activated(&f, request).await);
    assert_charged(&f, 0).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(
        activate_on(&mut tx, request).await.unwrap().unwrap(),
        original
    );
    tx.commit().await.unwrap(); // Simulate losing the response after commit.
    for attempt in [1, 2] {
        sqlx::query("INSERT INTO task_attempts (id, task_id, attempt_number, status, execution_generation, worker_id, machine_id) VALUES (gen_random_uuid(), $1, $2, 'failed', gen_random_uuid(), gen_random_uuid(), 'activation-test')")
            .bind(request.job.0).bind(attempt).execute(f.persistence().pool()).await.unwrap();
    }
    // Reconnect independently and corrupt the original resolution source. Replay
    // must use the saved snapshot rather than rebuild inputs from source/bundle.
    sqlx::query("UPDATE workflow_runs SET bundle = $2, input = '{}' WHERE id = $1")
        .bind(request.run.as_uuid())
        .bind(vec![0_u8])
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let options = f.persistence().pool().connect_options();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*options).clone())
        .await
        .unwrap();
    let restarted = PostgresPersistence::new(pool.clone());
    assert_eq!(restarted.activate(request).await.unwrap(), original);
    pool.close().await;
}

async fn successor(
    f: &AdmissionFixture,
    request: ActivationRequest,
    step: &str,
    ordinal: i64,
) -> ActivationRequest {
    let next = ActivationRequest {
        execution: ExecutionId::new(Uuid::new_v4()),
        job: WorkflowJobId(Uuid::new_v4()),
        ..request
    };
    sqlx::query("INSERT INTO workflow_executions (company_id, run_id, id, step_id, activation) VALUES ($1,$2,$3,$4,$5)")
        .bind(next.company.as_uuid()).bind(next.run.as_uuid()).bind(next.execution.as_uuid())
        .bind(step).bind(ordinal).execute(f.persistence().pool()).await.unwrap();
    sqlx::query("INSERT INTO background_tasks (id,company_id,correlation_id,task_type,payload,queue_kind,workflow_execution_id) VALUES ($1,$2,gen_random_uuid(),'workflow_execution',$3,'workflow',$4)")
        .bind(next.job.0).bind(next.company.as_uuid()).bind(job_payload(next.execution))
        .bind(next.execution.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    next
}

async fn complete_fixture(f: &AdmissionFixture, request: ActivationRequest, value: Value) {
    sqlx::query("UPDATE workflow_executions SET committed_output = $2, completed_at = clock_timestamp() WHERE id = $1")
        .bind(request.execution.as_uuid()).bind(value).execute(f.persistence().pool()).await.unwrap();
}

#[tokio::test]
async fn workflow_activation_predecessor_order_repeats_and_saved_replay() {
    let (f, first) = fixture().await;
    let next = successor(&f, first, "next", 2).await;
    assert!(f.persistence().activate(next).await.is_err());
    assert!(!activated(&f, next).await);
    assert_charged(&f, 0).await;
    f.persistence().activate(first).await.unwrap();
    complete_fixture(&f, first, json!({"round":1})).await;
    let saved = f.persistence().activate(next).await.unwrap();
    assert_eq!(saved.inputs["value"], json!({"round":1}));
    let repeated = successor(&f, first, "start", 3).await;
    f.persistence().activate(repeated).await.unwrap();
    complete_fixture(&f, repeated, Value::Null).await;
    assert_eq!(f.persistence().activate(next).await.unwrap(), saved);
    let later = successor(&f, first, "next", 4).await;
    assert_eq!(
        f.persistence().activate(later).await.unwrap().inputs["value"],
        Value::Null
    );
}

#[tokio::test]
async fn workflow_activation_scopes_job_run_and_rejects_invalid_inputs() {
    let (f, request) = fixture().await;
    for bad in [
        ActivationRequest {
            company: CompanyId::new(Uuid::new_v4()),
            ..request
        },
        ActivationRequest {
            run: RunId::new(Uuid::new_v4()),
            ..request
        },
        ActivationRequest {
            execution: ExecutionId::new(Uuid::new_v4()),
            ..request
        },
        ActivationRequest {
            job: WorkflowJobId(Uuid::new_v4()),
            ..request
        },
    ] {
        assert!(f.persistence().activate(bad).await.is_err());
        assert!(!activated(&f, request).await);
        assert_charged(&f, 0).await;
    }
    let other = successor(&f, request, "next", 2).await;
    assert!(
        f.persistence()
            .activate(ActivationRequest {
                job: other.job,
                ..request
            })
            .await
            .is_err()
    );
    for input in [json!({}), json!({"value":"x".repeat(70000)})] {
        sqlx::query("UPDATE workflow_runs SET input = $2 WHERE id = $1")
            .bind(request.run.as_uuid())
            .bind(input)
            .execute(f.persistence().pool())
            .await
            .unwrap();
        assert!(f.persistence().activate(request).await.is_err());
        assert!(!activated(&f, request).await);
        assert_charged(&f, 0).await;
    }
}

#[tokio::test]
async fn workflow_activation_sql_immutable_and_bounded_facts() {
    let (f, request) = fixture().await;
    f.persistence().activate(request).await.unwrap();
    complete_fixture(&f, request, json!(null)).await;
    for statement in [
        "UPDATE workflow_executions SET frozen_inputs = '{}' WHERE id = $1",
        "UPDATE workflow_executions SET activated_at = NULL, frozen_inputs = NULL WHERE id = $1",
        "UPDATE workflow_executions SET committed_output = '1' WHERE id = $1",
        "UPDATE workflow_executions SET completed_at = NULL, committed_output = NULL WHERE id = $1",
        "UPDATE workflow_executions SET activation = 2 WHERE id = $1",
    ] {
        assert!(
            sqlx::query(statement)
                .bind(request.execution.as_uuid())
                .execute(f.persistence().pool())
                .await
                .is_err()
        );
    }
    let next = successor(&f, request, "next", 2).await;
    for statement in [
        "UPDATE workflow_executions SET frozen_inputs = '{}' WHERE id = $1",
        "UPDATE workflow_executions SET activated_at = clock_timestamp(), frozen_inputs = 'null' WHERE id = $1",
        "UPDATE workflow_executions SET completed_at = clock_timestamp(), committed_output = 'null' WHERE id = $1",
        "UPDATE workflow_executions SET activated_at = clock_timestamp(), frozen_inputs = jsonb_build_object('value',repeat('x',1048576)) WHERE id = $1",
    ] {
        assert!(
            sqlx::query(statement)
                .bind(next.execution.as_uuid())
                .execute(f.persistence().pool())
                .await
                .is_err()
        );
    }
    let duplicate = sqlx::query("INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation) VALUES($1,$2,gen_random_uuid(),'different',1)")
        .bind(request.company.as_uuid()).bind(request.run.as_uuid()).execute(f.persistence().pool()).await;
    assert!(duplicate.is_err());
}

#[tokio::test]
async fn workflow_activation_final_statement_failure_rolls_back_snapshot() {
    let (f, request) = fixture().await;
    // Force a statement failure after freezing, exercising the transaction seam
    // used by bounded batches rather than an ordinary preflight rejection.
    let mut tx = f.persistence().pool().begin().await.unwrap();
    activate_on(&mut tx, request).await.unwrap().unwrap();
    assert!(sqlx::query("SELECT 1 / 0").execute(&mut *tx).await.is_err());
    tx.commit().await.unwrap(); // PostgreSQL commits an aborted transaction as rollback.
    assert!(!activated(&f, request).await);
    assert_charged(&f, 0).await;
    assert_eq!(
        f.persistence().activate(request).await.unwrap().inputs["value"],
        1
    );
}

#[tokio::test]
async fn workflow_activation_deadline_ordinal_and_output_bounds() {
    let (f, first) = fixture().await;
    f.persistence().activate(first).await.unwrap();
    complete_fixture(&f, first, json!("x".repeat(70000))).await;
    let next = successor(&f, first, "next", 2).await;
    assert!(f.persistence().activate(next).await.is_err());
    assert!(!activated(&f, next).await);
    assert_charged(&f, 1).await;
    let over_limit = successor(&f, first, "start", 101).await;
    assert!(f.persistence().activate(over_limit).await.is_err());
    // Change created_at with deadline to preserve the existing run CHECK.
    sqlx::query("UPDATE workflow_runs SET created_at = clock_timestamp() - interval '2 hours', deadline = clock_timestamp() - interval '1 hour' WHERE id = $1")
        .bind(first.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let expired = successor(&f, first, "start", 3).await;
    assert!(f.persistence().activate(expired).await.is_err());
    assert!(!activated(&f, expired).await);
    assert_charged(&f, 1).await;
    // Reading already-frozen input is still not an ownership/execution grant.
    assert_eq!(
        f.persistence().activate(first).await.unwrap().inputs["value"],
        1
    );
}

#[tokio::test]
async fn workflow_activation_parent_metadata_comes_from_admitted_cause() {
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["value"] = json!({"ref":"/run/parent_id"});
    source["steps"]["start"]["with"]["output_schema"] = json!({"literal":true});
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(source).await).await;
    let parent = f.prepare(f.request("parent", f.manual())).await;
    f.persistence().admit(&parent).await.unwrap();
    let cause = ChildCause::Execution(ExecutionRef::new(
        parent.company_id(),
        parent.proposed_run_id(),
        parent.first_execution_id(),
        parent.entry().clone(),
    ));
    let trigger = TriggerRef::new(
        parent.company_id(),
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child { parent: cause },
    )
    .unwrap();
    let child = f.prepare(f.request("child", trigger)).await;
    f.persistence().admit(&child).await.unwrap();
    let job: Uuid =
        sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id = $1")
            .bind(child.first_execution_id().as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    let request = ActivationRequest {
        company: child.company_id(),
        run: child.proposed_run_id(),
        execution: child.first_execution_id(),
        job: WorkflowJobId(job),
    };
    assert_eq!(
        f.persistence().activate(request).await.unwrap().inputs["value"],
        json!(parent.proposed_run_id().as_uuid())
    );
}

#[tokio::test]
async fn workflow_activation_ignores_unreferenced_historical_outputs() {
    let (f, first) = fixture().await;
    f.persistence().activate(first).await.unwrap();
    complete_fixture(&f, first, json!("x".repeat(40000))).await;
    let next = successor(&f, first, "next", 2).await;
    f.persistence().activate(next).await.unwrap();
    complete_fixture(&f, next, json!("y".repeat(40000))).await;
    let independent = successor(&f, first, "start", 3).await;
    assert_eq!(
        f.persistence().activate(independent).await.unwrap().inputs["value"],
        1
    );
    // A dependent step loads only its one 40 KiB dependency, not both outputs.
    let dependent = successor(&f, first, "next", 4).await;
    assert_eq!(
        f.persistence().activate(dependent).await.unwrap().inputs["value"],
        json!("x".repeat(40000))
    );
}

#[tokio::test]
async fn workflow_activation_waiting_on_run_lock_cannot_cross_deadline() {
    let (f, request) = fixture().await;
    sqlx::query("UPDATE workflow_runs SET deadline = clock_timestamp() + interval '300 milliseconds' WHERE id = $1")
        .bind(request.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id = $1 FOR UPDATE")
        .bind(request.run.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let activation = f.persistence().activate(request);
    tokio::pin!(activation);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut activation)
            .await
            .is_err()
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    blocker.commit().await.unwrap();
    assert!(activation.await.is_err());
    assert!(!activated(&f, request).await);
    assert_charged(&f, 0).await;
}
