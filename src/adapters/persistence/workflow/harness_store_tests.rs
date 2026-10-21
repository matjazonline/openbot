use super::*;

fn specification(row: &Value) -> crate::application::workflow::checkpoints::WorkflowCheckpointSpec {
    let run: RunCheckpoint = serde_json::from_value(row["checkpoint"].clone()).unwrap();
    crate::application::workflow::checkpoints::WorkflowCheckpointSpec {
        identity: run.identity,
        initial_prompt: "untrusted context".into(),
        policy: run.policy,
        prepared: row["workflow_prepared"].clone(),
    }
}

fn checkpoint(
    outcome: crate::services::harness::runs::WriteOutcome<RunCheckpoint>,
) -> RunCheckpoint {
    match outcome {
        crate::services::harness::runs::WriteOutcome::Applied(run)
        | crate::services::harness::runs::WriteOutcome::AlreadyApplied(run) => run,
        other => panic!("unexpected checkpoint result {other:?}"),
    }
}

#[tokio::test]
async fn workflow_harness_owner_store_competing_open_and_execution_replay() {
    use crate::application::workflow::checkpoints::{
        WorkflowCheckpointStore, WorkflowCheckpointWrite,
    };
    use crate::services::harness::runs::WriteOutcome;
    let NativeFixture {
        fixture: f,
        fence,
        row,
    } = native_row().await;
    let spec = specification(&row);
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let (left, right) = tokio::join!(
        f.persistence()
            .open_workflow_checkpoint(fence, &spec, policy),
        f.persistence()
            .open_workflow_checkpoint(fence, &spec, policy)
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(
        matches!(left, WriteOutcome::Applied(_)),
        matches!(right, WriteOutcome::Applied(_))
    );
    let run = checkpoint(left);
    assert_eq!(run, checkpoint(right));
    let write = WorkflowCheckpointWrite {
        fence,
        checkpoint: run.run_id,
        expected_revision: run.revision,
        lease_policy: policy,
    };
    let (left, right) = tokio::join!(
        f.persistence().reserve_workflow_execution(write, 1000),
        f.persistence().reserve_workflow_execution(write, 1000)
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(
        matches!(left, WriteOutcome::Applied(_)),
        matches!(right, WriteOutcome::Applied(_))
    );
    let saved = checkpoint(left);
    assert_eq!(saved, checkpoint(right));
    assert_eq!(saved.executions.len(), 1);
    assert_eq!(saved.revision.0, 1);
    let reloaded = checkpoint(
        f.persistence()
            .load_workflow_checkpoint(fence, policy)
            .await
            .unwrap(),
    );
    assert_eq!(reloaded, saved);
    let mut changed = spec;
    changed.prepared["preamble"] = json!("changed");
    // A changed preparation can never match the immutable checkpoint: terminal, not retried.
    assert!(matches!(
        f.persistence()
            .open_workflow_checkpoint(fence, &changed, policy)
            .await,
        Err(AppError::BadRequest(_))
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM model_usage_calls")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "execution debit alone must never reserve a model request"
    );
}

#[tokio::test]
async fn workflow_harness_owner_store_fence_and_actor_revocation() {
    use crate::application::workflow::checkpoints::WorkflowCheckpointStore;
    use crate::services::harness::runs::WriteOutcome;
    let NativeFixture {
        fixture: f,
        fence,
        row,
    } = native_row().await;
    let spec = specification(&row);
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let original = checkpoint(
        f.persistence()
            .open_workflow_checkpoint(fence, &spec, policy)
            .await
            .unwrap(),
    );
    let mut stale = fence;
    stale.generation.0 = Uuid::new_v4();
    assert!(matches!(
        f.persistence()
            .load_workflow_checkpoint(stale, policy)
            .await
            .unwrap(),
        WriteOutcome::OwnershipLost
    ));
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND user_id=$2")
        .bind(fence.scope.company.as_uuid())
        .bind(f.binding.target.actor.user_id())
        .execute(&mut *tx)
        .await
        .unwrap();
    let revoke = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        tx.commit().await.unwrap();
    };
    let (_, resumed) = tokio::join!(
        revoke,
        f.persistence()
            .open_workflow_checkpoint(fence, &spec, policy)
    );
    assert!(matches!(resumed.unwrap(), WriteOutcome::OwnershipLost));
    let saved: Value = sqlx::query_scalar("SELECT checkpoint FROM task_harness_runs WHERE id=$1")
        .bind(original.run_id.0)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(saved, serde_json::to_value(original).unwrap());
}

#[tokio::test]
async fn workflow_harness_owner_store_new_claim_retains_checkpoint_and_debits_execution() {
    use crate::application::workflow::checkpoints::{
        WorkflowCheckpointStore, WorkflowCheckpointWrite,
    };
    use crate::services::harness::runs::WriteOutcome;
    let NativeFixture {
        fixture: f,
        fence,
        row,
    } = native_row().await;
    let spec = specification(&row);
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let run = checkpoint(
        f.persistence()
            .open_workflow_checkpoint(fence, &spec, policy)
            .await
            .unwrap(),
    );
    let write = WorkflowCheckpointWrite {
        fence,
        checkpoint: run.run_id,
        expected_revision: run.revision,
        lease_policy: policy,
    };
    let saved = checkpoint(
        f.persistence()
            .reserve_workflow_execution(write, 1000)
            .await
            .unwrap(),
    );
    expire(&f, fence.scope).await;
    assert!(
        f.persistence()
            .settled()
            .claim_io(fence.scope, worker(), policy)
            .await
            .unwrap()
            .is_none()
    );
    due(&f, fence.scope).await;
    let next = f
        .persistence()
        .settled()
        .claim_io(fence.scope, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    let loaded = checkpoint(
        f.persistence()
            .open_workflow_checkpoint(next.fence, &spec, policy)
            .await
            .unwrap(),
    );
    assert_eq!(loaded, saved);
    let write = WorkflowCheckpointWrite {
        fence: next.fence,
        expected_revision: saved.revision,
        ..write
    };
    let saved = checkpoint(
        f.persistence()
            .reserve_workflow_execution(write, 2000)
            .await
            .unwrap(),
    );
    assert_eq!(saved.executions.len(), 2);
    assert_eq!(saved.executions[0].allowance_ms, 1000);
    assert_eq!(saved.executions[1].allowance_ms, 2000);
    assert_eq!(saved.executions[1].generation, next.fence.generation.0);
    assert!(matches!(
        f.persistence()
            .reserve_workflow_execution(WorkflowCheckpointWrite { fence, ..write }, 1000)
            .await
            .unwrap(),
        WriteOutcome::OwnershipLost
    ));
    let stored: Value = sqlx::query_scalar("SELECT jsonb_build_object('generation',workflow_generation,'worker',workflow_worker_id,'attempt',workflow_attempt) FROM task_harness_runs WHERE id=$1")
        .bind(saved.run_id.0).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(
        stored,
        json!({"generation":next.fence.generation.0,"worker":next.fence.worker.0,"attempt":next.fence.attempt.0})
    );
}

#[tokio::test]
async fn workflow_harness_owner_store_deferred_failure_rolls_back_open_and_debit() {
    use crate::application::workflow::checkpoints::{
        WorkflowCheckpointStore, WorkflowCheckpointWrite,
    };
    let NativeFixture {
        fixture: f,
        fence,
        row,
    } = native_row().await;
    let spec = specification(&row);
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    sqlx::raw_sql(
        "CREATE FUNCTION checkpoint_fault() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'injected checkpoint failure'; END $$;
        CREATE CONSTRAINT TRIGGER checkpoint_fault AFTER INSERT OR UPDATE ON task_harness_runs
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION checkpoint_fault();",
    )
    .execute(f.persistence().pool())
    .await
    .unwrap();
    assert!(
        f.persistence()
            .open_workflow_checkpoint(fence, &spec, policy)
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM task_harness_runs")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::raw_sql("DROP TRIGGER checkpoint_fault ON task_harness_runs;")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let original = checkpoint(
        f.persistence()
            .open_workflow_checkpoint(fence, &spec, policy)
            .await
            .unwrap(),
    );
    sqlx::raw_sql(
        "CREATE CONSTRAINT TRIGGER checkpoint_fault AFTER UPDATE ON task_harness_runs
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION checkpoint_fault();",
    )
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let write = WorkflowCheckpointWrite {
        fence,
        checkpoint: original.run_id,
        expected_revision: original.revision,
        lease_policy: policy,
    };
    assert!(
        f.persistence()
            .reserve_workflow_execution(write, 1000)
            .await
            .is_err()
    );
    let reloaded = checkpoint(
        f.persistence()
            .load_workflow_checkpoint(fence, policy)
            .await
            .unwrap(),
    );
    assert_eq!(reloaded, original);
}
