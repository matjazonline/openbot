use super::*;
use crate::application::workflow::storage::{RetentionRoot, WorkflowStorageMaintenance};

async fn retire_candidate(f: &ModelFixture) -> RetentionRoot {
    let scope = f.native.fence.scope;
    let p = f.native.fixture.persistence();
    let command = CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: f.native.fixture.binding.target.actor,
        command_key: IdempotencyKey::parse("checkpoint-retention").unwrap(),
        expected_revision: p
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    assert!(matches!(
        p.cancel(command).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    let finished_at = sqlx::query_scalar(
        "UPDATE workflow_runs SET created_at=clock_timestamp()-interval '100 days',
        finished_at=clock_timestamp()-interval '91 days' WHERE id=$1 RETURNING finished_at",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(p.pool())
        .await
        .unwrap();
    RetentionRoot {
        finished_at,
        company: scope.company,
        run: scope.run,
    }
}
async fn snapshot(p: &PostgresPersistence) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object(
        'runs',(SELECT jsonb_agg(to_jsonb(row) ORDER BY id) FROM workflow_runs AS row),
        'steps',(SELECT jsonb_agg(to_jsonb(row) ORDER BY id) FROM workflow_executions AS row),
        'jobs',(SELECT jsonb_agg(to_jsonb(row) ORDER BY id) FROM background_tasks AS row),
        'attempts',(SELECT jsonb_agg(to_jsonb(row) ORDER BY id) FROM task_attempts AS row),
        'checkpoints',(SELECT jsonb_agg(to_jsonb(row) ORDER BY id) FROM task_harness_runs AS row),
        'calls',(SELECT jsonb_agg(to_jsonb(row) ORDER BY request_id) FROM model_usage_calls AS row),
        'history',(SELECT jsonb_agg(to_jsonb(row) ORDER BY request_id) FROM model_usage_history AS row))")
        .fetch_one(p.pool()).await.unwrap()
}
async fn open_protected(f: &ModelFixture) -> RunCheckpoint {
    let admission = &f.native.fixture;
    let command = admission
        .prepare(admission.request("protected-checkpoint", admission.manual()))
        .await;
    let p = admission.persistence();
    p.admit(&command).await.unwrap();
    let scope =
        crate::adapters::persistence::workflow::tests::admission_tests::child_support::entry(
            admission,
            command.proposed_run_id(),
        )
        .await;
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    let prepared =
        crate::application::workflow::agent::AgentPreparation::prepare_agent(p, &claim, policy)
            .await
            .unwrap()
            .expect_ready()
            .prepared;
    let spec = prepared.spec(claim.fence).unwrap();
    let WriteOutcome::Applied(run) = p
        .open_workflow_checkpoint(claim.fence, &spec, policy)
        .await
        .unwrap()
    else {
        panic!("protected checkpoint open")
    };
    let write = WorkflowCheckpointWrite {
        fence: claim.fence,
        checkpoint: run.run_id,
        expected_revision: run.revision,
        lease_policy: policy,
    };
    let WriteOutcome::Applied(run) = p.reserve_workflow_execution(write, 1000).await.unwrap()
    else {
        panic!("protected execution")
    };
    run
}

async fn release_history_pin(f: &ModelFixture, candidate: RetentionRoot) {
    let p = f.native.fixture.persistence();
    let before = snapshot(p).await;
    assert!(
        p.retire_tree(candidate).await.unwrap().is_none(),
        "independently retained input must pin its source"
    );
    assert_eq!(snapshot(p).await, before);
    // Only the history payload expires here; source metadata remains until retire_tree.
    crate::adapters::persistence::workflow::agent_history_retention_fixture::expire(
        p,
        f.native.fence.scope,
    )
    .await;
}

async fn case(fault: bool) {
    let f = Box::pin(opened()).await;
    let p = f.native.fixture.persistence();
    let request = reservation();
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(f.write, model_request(&request), None)
        .await
        .unwrap()
    else {
        panic!("actual model reservation")
    };
    let checkpoint = permit.consume();
    assert_eq!(checkpoint.executions.len(), 1);
    // An unrelated live root/checkpoint in the SAME company and owned database.
    let protected = open_protected(&f).await;
    let candidate = retire_candidate(&f).await;
    release_history_pin(&f, candidate).await;
    let before = snapshot(p).await;
    assert_eq!(before["checkpoints"].as_array().unwrap().len(), 2);
    assert_eq!(before["calls"].as_array().unwrap().len(), 1);
    let protected_before: Value =
        sqlx::query_scalar("SELECT to_jsonb(row) FROM task_harness_runs AS row WHERE id=$1")
            .bind(protected.run_id.0)
            .fetch_one(p.pool())
            .await
            .unwrap();
    if fault {
        let job = f.native.fence.scope.job.0;
        sqlx::raw_sql(&format!("CREATE FUNCTION checkpoint_retention_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'checkpoint retention between runs and jobs'; END $$;
            CREATE TRIGGER checkpoint_retention_fault BEFORE DELETE ON background_tasks FOR EACH ROW WHEN(OLD.id='{job}'::uuid) EXECUTE FUNCTION checkpoint_retention_fault()"))
            .execute(p.pool()).await.unwrap();
        let error = p.retire_tree(candidate).await.unwrap_err();
        assert!(
            format!("{error:?}").contains("checkpoint retention between runs and jobs"),
            "{error:?}"
        );
        assert_eq!(
            snapshot(p).await,
            before,
            "checkpoint, attempts, owners, usage and archive rolled back together"
        );
        sqlx::query("DROP TRIGGER checkpoint_retention_fault ON background_tasks")
            .execute(p.pool())
            .await
            .unwrap();
    }
    let retired = p.retire_tree(candidate).await.unwrap().unwrap();
    assert_eq!((retired.runs, retired.jobs, retired.calls), (1, 1, 1));
    let after = snapshot(p).await;
    assert_eq!(after["checkpoints"].as_array().unwrap().len(), 1);
    assert_eq!(after["runs"].as_array().unwrap().len(), 1);
    assert_eq!(after["jobs"].as_array().unwrap().len(), 1);
    assert!(after["calls"].is_null());
    assert_eq!(after["history"].as_array().unwrap().len(), 1);
    let protected_after: Value =
        sqlx::query_scalar("SELECT to_jsonb(row) FROM task_harness_runs AS row WHERE id=$1")
            .bind(protected.run_id.0)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(protected_after, protected_before);
}
#[tokio::test]
async fn run_retention_native_checkpoint_and_model_usage_delete_with_only_owned_jobs() {
    case(false).await;
}
#[tokio::test]
async fn run_retention_native_checkpoint_model_usage_and_tree_rollback_then_retry() {
    case(true).await;
}
