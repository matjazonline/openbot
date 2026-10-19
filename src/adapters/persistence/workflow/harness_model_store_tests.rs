use super::*;
use crate::application::workflow::checkpoints::*;
use crate::services::harness::runs::WriteOutcome;

#[path = "harness_token_tests.rs"]
mod tokens;

fn model_request(reservation: &ModelReservation) -> WorkflowModelRequest {
    WorkflowModelRequest {
        reservation: reservation.clone(),
        fingerprint: [7; 32],
    }
}

struct ModelFixture {
    native: NativeFixture,
    run: RunCheckpoint,
    write: WorkflowCheckpointWrite,
}
async fn opened() -> ModelFixture {
    let native = native_row().await;
    open_native(native).await
}

async fn open_native(native: NativeFixture) -> ModelFixture {
    let initial: RunCheckpoint = serde_json::from_value(native.row["checkpoint"].clone()).unwrap();
    let spec = WorkflowCheckpointSpec {
        identity: initial.identity,
        initial_prompt: "untrusted context".into(),
        policy: initial.policy,
        prepared: native.row["workflow_prepared"].clone(),
    };
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let WriteOutcome::Applied(run) = native
        .fixture
        .persistence()
        .open_workflow_checkpoint(native.fence, &spec, policy)
        .await
        .unwrap()
    else {
        panic!("open failed")
    };
    let write = WorkflowCheckpointWrite {
        fence: native.fence,
        checkpoint: run.run_id,
        expected_revision: run.revision,
        lease_policy: policy,
    };
    let WriteOutcome::Applied(run) = native
        .fixture
        .persistence()
        .reserve_workflow_execution(write, 1000)
        .await
        .unwrap()
    else {
        panic!("execution failed")
    };
    let write = WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..write
    };
    ModelFixture { native, run, write }
}
async fn accounting(f: &ModelFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('calls',usage.model_calls,'rows',
        (SELECT COALESCE(jsonb_agg(to_jsonb(call) ORDER BY request_id),'[]'::jsonb) FROM model_usage_calls AS call WHERE call.company_id=usage.company_id))
        FROM workflow_root_budget_usage AS usage WHERE usage.company_id=$1")
        .bind(f.native.fence.scope.company.as_uuid()).fetch_one(f.native.fixture.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_harness_model_store_competing_admission_and_settlement() {
    let f = opened().await;
    let p = f.native.fixture.persistence();
    let request = reservation();
    let (left, right) = tokio::join!(
        p.reserve_workflow_model(f.write, model_request(&request.clone()), None),
        p.reserve_workflow_model(f.write, model_request(&request.clone()), None)
    );
    let (left, right) = (left.unwrap(), right.unwrap());
    let run = match (left, right) {
        (
            WorkflowModelAdmission::New(permit),
            WorkflowModelAdmission::PreviouslyAdmitted(saved),
        )
        | (
            WorkflowModelAdmission::PreviouslyAdmitted(saved),
            WorkflowModelAdmission::New(permit),
        ) => {
            assert_eq!(permit.checkpoint(), saved.as_ref());
            permit.consume()
        }
        result => panic!("wrong admission: {result:?}"),
    };
    let write = WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..f.write
    };
    assert!(matches!(
        p.reserve_workflow_model(write, model_request(&reservation()), None)
            .await
            .unwrap(),
        WorkflowModelAdmission::ActiveRequest
    ));
    let before = accounting(&f).await;
    assert_eq!(before["calls"], 1);
    assert_eq!(before["rows"].as_array().unwrap().len(), 1);
    let row = &before["rows"][0];
    assert_eq!(row["run_id"], json!(f.native.fence.scope.run.as_uuid()));
    assert_eq!(
        row["execution_id"],
        json!(f.native.fence.scope.execution.as_uuid())
    );
    assert_eq!(row["task_id"], json!(f.native.fence.scope.job.0));
    assert!(row["settled_at"].is_null());
    let turn = response(&request);
    let (left, right) = tokio::join!(
        p.settle_workflow_model(write, turn.clone(), None),
        p.settle_workflow_model(write, turn, None)
    );
    let (WorkflowModelSettlement::Saved(left), WorkflowModelSettlement::Saved(right)) =
        (left.unwrap(), right.unwrap())
    else {
        panic!("not settled")
    };
    assert!(matches!(
        (left.as_ref(), right.as_ref()),
        (WriteOutcome::Applied(_), WriteOutcome::AlreadyApplied(_))
            | (WriteOutcome::AlreadyApplied(_), WriteOutcome::Applied(_))
    ));
    let after = accounting(&f).await;
    assert_eq!(after["calls"], 1);
    assert_eq!(after["rows"][0]["input_tokens"], 70);
    assert_eq!(after["rows"][0]["output_tokens"], 50);
    assert!(stored(&f.native.fixture, run.run_id.0).await["model_request_id"].is_null());
}

#[tokio::test]
async fn workflow_harness_model_store_projection_failure_rolls_back_all_admission() {
    let f = opened().await;
    let p = f.native.fixture.persistence();
    let before = stored(&f.native.fixture, f.run.run_id.0).await;
    let usage = accounting(&f).await;
    sqlx::raw_sql("CREATE FUNCTION fail_native_model_projection() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture'; END; $$;
        CREATE TRIGGER fail_native_model_projection BEFORE INSERT ON model_usage_calls FOR EACH ROW EXECUTE FUNCTION fail_native_model_projection();")
        .execute(p.pool()).await.unwrap();
    assert!(
        p.reserve_workflow_model(f.write, model_request(&reservation()), None)
            .await
            .is_err()
    );
    assert_eq!(stored(&f.native.fixture, f.run.run_id.0).await, before);
    assert_eq!(accounting(&f).await, usage);
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_budget_receipts WHERE resource='model_call'",
    )
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(receipts, 0);
}

#[tokio::test]
async fn workflow_harness_model_store_lost_request_stays_charged_and_cannot_reenter() {
    let mut f = opened().await;
    let p = f.native.fixture.persistence();
    let request = reservation();
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(f.write, model_request(&request.clone()), None)
        .await
        .unwrap()
    else {
        panic!("not admitted")
    };
    let run = permit.consume();
    let before = accounting(&f).await;
    let next = reclaim(&f.native.fixture, f.native.fence, f.write.lease_policy).await;
    f.write.fence = next;
    f.write.expected_revision = run.revision;
    let WriteOutcome::Applied(run) = p.reserve_workflow_execution(f.write, 1000).await.unwrap()
    else {
        panic!("not resumed")
    };
    f.write.expected_revision = run.revision;
    assert!(matches!(
        p.settle_workflow_model(f.write, response(&request), None)
            .await
            .unwrap(),
        WorkflowModelSettlement::Saved(outcome) if matches!(*outcome, WriteOutcome::OwnershipLost)
    ));
    assert!(matches!(
        p.reserve_workflow_model(f.write, model_request(&request), None)
            .await
            .unwrap(),
        WorkflowModelAdmission::PreviouslyAdmitted(_)
    ));
    assert_eq!(accounting(&f).await, before);
    assert!(matches!(
        p.reserve_workflow_model(f.write, model_request(&reservation()), None)
            .await
            .unwrap(),
        WorkflowModelAdmission::New(_)
    ));
    let after = accounting(&f).await;
    assert_eq!(after["calls"], 2);
    assert_eq!(after["rows"].as_array().unwrap().len(), 2);
    assert!(
        after["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["settled_at"].is_null())
    );
}
