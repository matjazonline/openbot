use super::*;
use crate::services::harness::runs::{
    ModelRequestId, ModelReservation, Mutation, SavedModelTurn, TokenUsageSource,
};

fn reservation() -> ModelReservation {
    ModelReservation {
        request_id: ModelRequestId(Uuid::new_v4()),
        repair: None,
        input_tokens: 100,
        output_tokens: 100,
    }
}

fn response(request: &ModelReservation) -> SavedModelTurn {
    SavedModelTurn {
        request_id: request.request_id,
        token_usage_source: TokenUsageSource::Reported,
        invalid_response: None,
        text: "saved".into(),
        calls: vec![],
        continuation: vec![],
        input_tokens: 70,
        output_tokens: 50,
    }
}

fn reserved(row: &Value, request: &ModelReservation) -> RunCheckpoint {
    let run: RunCheckpoint = serde_json::from_value(row["checkpoint"].clone()).unwrap();
    run.apply(Mutation::Reserve(request.clone())).unwrap().0
}

async fn write_model(
    db: &mut sqlx::PgConnection,
    run: &RunCheckpoint,
    fence: WorkflowFence,
    request: Option<ModelRequestId>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE task_harness_runs SET revision=$2,checkpoint=$3,
        model_request_id=$4,model_generation=$5,model_worker_id=$6,model_attempt=$7,
        workflow_generation=$8,workflow_worker_id=$9,workflow_attempt=$10 WHERE id=$1",
    )
    .bind(run.run_id.0)
    .bind(run.revision.0 as i64)
    .bind(serde_json::to_value(run).unwrap())
    .bind(request.map(|id| id.0))
    .bind(request.map(|_| fence.generation.0))
    .bind(request.map(|_| fence.worker.0))
    .bind(request.map(|_| fence.attempt.0))
    .bind(fence.generation.0)
    .bind(fence.worker.0)
    .bind(fence.attempt.0)
    .execute(db)
    .await
    .map(|_| ())
}

async fn stored(f: &AdmissionFixture, id: Uuid) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(harness) FROM task_harness_runs AS harness WHERE id=$1")
        .bind(id)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn workflow_harness_model_owner_competing_admissions_keep_one_slot() {
    let NativeFixture {
        fixture: f,
        fence,
        row,
    } = native_row().await;
    insert(&mut f.persistence().pool().acquire().await.unwrap(), &row)
        .await
        .unwrap();
    let first = reservation();
    let second = reservation();
    let left = reserved(&row, &first);
    let right = reserved(&row, &second);
    let mut left_db = f.persistence().pool().acquire().await.unwrap();
    let mut right_db = f.persistence().pool().acquire().await.unwrap();
    let (a, b) = tokio::join!(
        write_model(&mut left_db, &left, fence, Some(first.request_id)),
        write_model(&mut right_db, &right, fence, Some(second.request_id))
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let saved = stored(&f, left.run_id.0).await;
    let winner = if a.is_ok() { first } else { second };
    assert_eq!(saved["model_request_id"], json!(winner.request_id.0));
    assert_eq!(saved["checkpoint"]["reservations"], json!([winner]));
    assert_eq!(saved["model_generation"], json!(fence.generation.0));
}

#[tokio::test]
async fn workflow_harness_model_owner_requires_reservation_and_exact_settlement() {
    let NativeFixture {
        fixture: f,
        fence,
        row,
    } = native_row().await;
    let mut db = f.persistence().pool().acquire().await.unwrap();
    insert(&mut db, &row).await.unwrap();
    let request = reservation();
    let run = reserved(&row, &request);
    assert!(write_model(&mut db, &run, fence, None).await.is_err());
    let mut absent = run.clone();
    absent.reservations.clear();
    assert!(
        write_model(&mut db, &absent, fence, Some(request.request_id))
            .await
            .is_err()
    );
    let mut wrong = fence;
    wrong.worker.0 = Uuid::new_v4();
    assert!(
        write_model(&mut db, &run, wrong, Some(request.request_id))
            .await
            .is_err()
    );
    write_model(&mut db, &run, fence, Some(request.request_id))
        .await
        .unwrap();
    let before = stored(&f, run.run_id.0).await;
    let mut cleared = run.clone();
    cleared.revision.0 += 1;
    assert!(write_model(&mut db, &cleared, fence, None).await.is_err());
    let other = reservation();
    let changed = run.apply(Mutation::Reserve(other.clone())).unwrap().0;
    assert!(
        write_model(&mut db, &changed, fence, Some(other.request_id))
            .await
            .is_err()
    );
    assert_eq!(stored(&f, run.run_id.0).await, before);
    let settled = run.apply(Mutation::Model(response(&request))).unwrap().0;
    assert!(
        write_model(&mut db, &settled, fence, Some(request.request_id))
            .await
            .is_err()
    );
    write_model(&mut db, &settled, fence, None).await.unwrap();
    assert!(stored(&f, run.run_id.0).await["model_request_id"].is_null());
    let mut reuse = settled.clone();
    reuse.revision.0 += 1;
    assert!(
        write_model(&mut db, &reuse, fence, Some(request.request_id))
            .await
            .is_err()
    );
    let fresh = settled.apply(Mutation::Reserve(other.clone())).unwrap().0;
    write_model(&mut db, &fresh, fence, Some(other.request_id))
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_harness_model_owner_new_claim_releases_slot_without_refunding() {
    use crate::application::workflow::checkpoints::{
        WorkflowCheckpointStore, WorkflowCheckpointWrite,
    };
    let NativeFixture {
        fixture: f,
        fence,
        row,
    } = native_row().await;
    let mut db = f.persistence().pool().acquire().await.unwrap();
    insert(&mut db, &row).await.unwrap();
    let request = reservation();
    let run = reserved(&row, &request);
    write_model(&mut db, &run, fence, Some(request.request_id))
        .await
        .unwrap();
    drop(db);
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let next = reclaim(&f, fence, policy).await;
    let before = stored(&f, run.run_id.0).await;
    let late = run.apply(Mutation::Model(response(&request))).unwrap().0;
    assert!(
        write_model(
            &mut f.persistence().pool().acquire().await.unwrap(),
            &late,
            next,
            None
        )
        .await
        .is_err()
    );
    assert_eq!(stored(&f, run.run_id.0).await, before);
    let write = WorkflowCheckpointWrite {
        fence: next,
        checkpoint: run.run_id,
        expected_revision: run.revision,
        lease_policy: policy,
    };
    let saved = f
        .persistence()
        .reserve_workflow_execution(write, 1000)
        .await
        .unwrap();
    let crate::services::harness::runs::WriteOutcome::Applied(saved) = saved else {
        panic!("not applied")
    };
    assert_eq!(saved.reservations, run.reservations);
    assert_eq!(saved.turns, run.turns);
    let after = stored(&f, run.run_id.0).await;
    for field in [
        "model_request_id",
        "model_generation",
        "model_worker_id",
        "model_attempt",
    ] {
        assert!(!before[field].is_null());
        assert!(after[field].is_null());
    }
    let mut old_id = saved.clone();
    old_id.revision.0 += 1;
    let mut db = f.persistence().pool().acquire().await.unwrap();
    assert!(
        write_model(&mut db, &old_id, next, Some(request.request_id))
            .await
            .is_err()
    );
    assert_eq!(stored(&f, run.run_id.0).await, after);
}

async fn reclaim(f: &AdmissionFixture, fence: WorkflowFence, policy: LeasePolicy) -> WorkflowFence {
    expire(f, fence.scope).await;
    assert!(
        f.persistence()
            .settled()
            .claim_io(fence.scope, worker(), policy)
            .await
            .unwrap()
            .is_none()
    );
    due(f, fence.scope).await;
    let next = f
        .persistence()
        .settled()
        .claim_io(fence.scope, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(next.fence.generation, fence.generation);
    assert!(next.fence.attempt.0 > fence.attempt.0);
    next.fence
}

#[path = "harness_model_store_tests.rs"]
mod store;
