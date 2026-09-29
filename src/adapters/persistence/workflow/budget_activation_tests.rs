use super::*;
use crate::adapters::persistence::workflow::activation::activate_on;
use crate::application::workflow::polling::WorkflowPolling;

async fn limited(kind: &str, limit: u32) -> (AdmissionFixture, ActivationRequest) {
    let mut source = example(kind);
    source["limits"]["root_budget"] = json!({"activations":limit,"model_calls":2,"repetitions":2});
    fixture(source).await
}
async fn accounting(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('usage',(SELECT jsonb_agg(to_jsonb(usage) ORDER BY root_run_id) FROM workflow_root_budget_usage AS usage),'receipts',(SELECT COALESCE(jsonb_agg(to_jsonb(receipt) ORDER BY run_id,execution_id,resource,reservation_key),'[]') FROM workflow_budget_receipts AS receipt))").fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_budget_activation_competing_descendants_replay_and_retirement() {
    let (f, root) = limited("data.map", 2).await;
    let child = admit_child(&f, root, "start", "child").await;
    let grandchild = admit_child(&f, child, "start", "grandchild").await;
    let barrier = Barrier::new(3);
    let activate = |scope| {
        let f = &f;
        let barrier = &barrier;
        async move {
            barrier.wait().await;
            f.persistence().activate(scope).await
        }
    };
    let (a, b, c) = tokio::join!(activate(root), activate(child), activate(grandchild));
    let results = [(root, a), (child, b), (grandchild, c)];
    assert_eq!(results.iter().filter(|(_, r)| r.is_ok()).count(), 2);
    let before = accounting(&f).await;
    assert_eq!(before["usage"][0]["activations"], 2);
    assert_eq!(before["receipts"].as_array().unwrap().len(), 3);
    for (scope, result) in results {
        if let Ok(saved) = result {
            assert_eq!(f.persistence().activate(scope).await.unwrap(), saved);
        } else {
            assert!(f.persistence().activate(scope).await.is_err());
            let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
                .bind(scope.run.as_uuid())
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
            assert_eq!(state, "failed");
            let safe: bool = sqlx::query_scalar("SELECT workflow_control_retry_safe($1,$2,$3)")
                .bind(scope.company.as_uuid())
                .bind(scope.run.as_uuid())
                .bind(scope.job.0)
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
            assert!(!safe);
            for _ in 0..2 {
                assert!(
                    f.persistence()
                        .poll_work(None, 128)
                        .await
                        .unwrap()
                        .candidates
                        .iter()
                        .all(|c| c.scope.run != scope.run)
                );
            }
        }
    }
    assert_eq!(accounting(&f).await, before);
}

#[tokio::test]
async fn workflow_budget_activation_rollback_and_fresh_handle_replay() {
    let (f, scope) = limited("data.map", 1).await;
    let before = accounting(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let saved = activate_on(&mut tx, scope).await.unwrap().unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(accounting(&f).await, before);
    let options = f.persistence().pool().connect_options();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*options).clone())
        .await
        .unwrap();
    let restarted = PostgresPersistence::new(pool.clone());
    assert_eq!(restarted.activate(scope).await.unwrap(), saved);
    let charged = accounting(&f).await;
    assert_eq!(charged["usage"][0]["activations"], 1);
    assert_eq!(f.persistence().activate(scope).await.unwrap(), saved);
    assert_eq!(accounting(&f).await, charged);
    pool.close().await;
}

#[tokio::test]
async fn workflow_budget_activation_pure_io_wait_refusal_no_hot_poll() {
    for kind in ["data.map", "context.load", "wait.timer", "wait.event"] {
        let (f, root) = limited(kind, 1).await;
        let child = admit_child(&f, root, "start", "child").await;
        f.persistence().activate(root).await.unwrap();
        match kind {
            "data.map" => {
                assert_eq!(
                    f.persistence()
                        .advance_pure(child, budget(64))
                        .await
                        .unwrap()
                        .disposition,
                    BatchDisposition::Failed
                );
            }
            "context.load" => {
                assert!(
                    f.persistence()
                        .claim_io(
                            child,
                            WorkflowWorkerId(Uuid::new_v4()),
                            LeasePolicy::new(Duration::from_secs(3)).unwrap()
                        )
                        .await
                        .unwrap()
                        .is_none()
                );
            }
            _ => {
                assert!(f.persistence().park_wait(child).await.unwrap().is_none());
            }
        }
        let before = accounting(&f).await;
        assert_eq!(before["usage"][0]["activations"], 1);
        assert_eq!(before["receipts"].as_array().unwrap().len(), 2);
        for _ in 0..2 {
            assert!(
                f.persistence()
                    .poll_work(None, 128)
                    .await
                    .unwrap()
                    .candidates
                    .iter()
                    .all(|c| c.scope.run != child.run)
            );
        }
        assert_eq!(accounting(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_budget_activation_usage_wait_deadline_rolls_back() {
    let (f, scope) = limited("data.map", 1).await;
    let mut held = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT root_run_id FROM workflow_root_budget_usage FOR UPDATE")
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+interval '250 milliseconds' WHERE id=$1").bind(scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let before = accounting(&f).await;
    let activate = f.persistence().activate(scope);
    tokio::pin!(activate);
    tokio::select! { result=&mut activate=>panic!("must block on usage: {result:?}"), _=tokio::time::sleep(Duration::from_millis(350))=>{} }
    held.rollback().await.unwrap();
    assert!(activate.await.is_err());
    assert_eq!(accounting(&f).await, before);
}

#[tokio::test]
async fn workflow_budget_activation_mid_batch_refusal_commits_prior_results() {
    let mut definition = source();
    definition["limits"]["root_budget"] = json!({"activations":2,"model_calls":2,"repetitions":2});
    definition["steps"]["finish"]["routes"]["error"] = json!("recover");
    definition["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    let (f, scope) = fixture(definition).await;
    let done = f
        .persistence()
        .advance_pure(scope, budget(64))
        .await
        .unwrap();
    assert_eq!(done.disposition, BatchDisposition::Failed);
    let state = snapshot(&f).await;
    let executions = state["executions"].as_array().unwrap();
    assert_eq!(executions.len(), 3);
    assert!(executions[0]["completed_at"].is_string());
    assert!(executions[1]["completed_at"].is_string());
    assert!(executions[2]["activated_at"].is_null());
    assert!(executions[2]["committed_route"].is_null());
    assert_eq!(state["runs"][0]["state"], "failed");
    let saved = accounting(&f).await;
    assert_eq!(saved["usage"][0]["activations"], 2);
    assert_eq!(saved["receipts"].as_array().unwrap().len(), 3);
    for _ in 0..2 {
        assert!(
            f.persistence()
                .poll_work(None, 128)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
    }
    assert!(
        f.persistence()
            .advance_pure(done.last, budget(64))
            .await
            .is_err()
    );
    f.persistence()
        .advance_pure(scope, budget(64))
        .await
        .unwrap();
    assert_eq!(accounting(&f).await, saved);
    assert_eq!(snapshot(&f).await, state);
}

#[tokio::test]
async fn workflow_budget_activation_successful_io_and_wait_replay_charge_once() {
    let (f, scope) = limited("context.load", 1).await;
    let claimed = f
        .persistence()
        .claim_io(
            scope,
            WorkflowWorkerId(Uuid::new_v4()),
            LeasePolicy::new(Duration::from_secs(3)).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    let result = || FencedWorkflowResult {
        fence: claimed.fence,
        output: json!({"items":[],"token_count":0}),
    };
    f.persistence()
        .complete_io(result())
        .await
        .unwrap()
        .unwrap();
    let saved = accounting(&f).await;
    assert_eq!(saved["usage"][0]["activations"], 1);
    f.persistence()
        .complete_io(result())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(accounting(&f).await, saved);

    let mut definition = example("wait.timer");
    definition["limits"]["root_budget"] = json!({"activations":1,"model_calls":2,"repetitions":2});
    definition["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()-chrono::Duration::seconds(1)).to_rfc3339()});
    let (f, scope) = fixture(definition).await;
    f.persistence().park_wait(scope).await.unwrap().unwrap();
    let saved = accounting(&f).await;
    assert_eq!(saved["usage"][0]["activations"], 1);
    for expected in [CommitDisposition::Committed, CommitDisposition::Replayed] {
        assert!(
            matches!(f.persistence().resume_wait(scope).await.unwrap(), WaitProgress::Completed(saved) if saved.disposition == expected)
        );
    }
    assert_eq!(accounting(&f).await, saved);
}

#[tokio::test]
async fn workflow_budget_activation_child_ignores_held_ancestor_locks() {
    let (f, root) = limited("data.map", 2).await;
    let child = admit_child(&f, root, "start", "child").await;
    let grandchild = admit_child(&f, child, "start", "grandchild").await;
    let mut held = f.persistence().pool().begin().await.unwrap();
    for ancestor in [root, child] {
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
            .bind(ancestor.run.as_uuid())
            .execute(&mut *held)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM workflow_executions WHERE id=$1 FOR UPDATE")
            .bind(ancestor.execution.as_uuid())
            .execute(&mut *held)
            .await
            .unwrap();
    }
    tokio::time::timeout(
        Duration::from_secs(2),
        f.persistence().advance_pure(grandchild, budget(4)),
    )
    .await
    .unwrap()
    .unwrap();
    let saved = accounting(&f).await;
    assert_eq!(saved["usage"][0]["activations"], 1);
    held.rollback().await.unwrap();
    f.persistence()
        .advance_pure(grandchild, budget(4))
        .await
        .unwrap();
    assert_eq!(accounting(&f).await, saved);
}

#[tokio::test]
async fn workflow_budget_activation_cancelled_owner_never_spends() {
    let (f, scope) = limited("data.map", 1).await;
    let before = accounting(&f).await;
    let mut cancel = f.persistence().pool().begin().await.unwrap();
    sqlx::query("UPDATE workflow_runs SET state='cancelled' WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(&mut *cancel)
        .await
        .unwrap();
    let activate = f.persistence().activate(scope);
    tokio::pin!(activate);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut activate)
            .await
            .is_err()
    );
    cancel.commit().await.unwrap();
    assert!(activate.await.is_err());
    assert_eq!(accounting(&f).await, before);
}

#[tokio::test]
async fn workflow_budget_activation_deferred_commit_failure_preserves_freeze_and_refusal() {
    for consumed in [false, true] {
        let (f, root) = limited("data.map", 1).await;
        let child = admit_child(&f, root, "start", "child").await;
        if consumed {
            f.persistence().activate(root).await.unwrap();
        }
        sqlx::query("CREATE FUNCTION activation_commit_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'activation commit fault'; END $$").execute(f.persistence().pool()).await.unwrap();
        sqlx::query("CREATE CONSTRAINT TRIGGER activation_commit_fault AFTER INSERT ON workflow_budget_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION activation_commit_fault()").execute(f.persistence().pool()).await.unwrap();
        let before = accounting(&f).await;
        let state = snapshot(&f).await;
        assert!(f.persistence().activate(child).await.is_err());
        assert_eq!(accounting(&f).await, before);
        assert_eq!(snapshot(&f).await, state);
        sqlx::query("DROP TRIGGER activation_commit_fault ON workflow_budget_receipts")
            .execute(f.persistence().pool())
            .await
            .unwrap();
        assert_eq!(f.persistence().activate(child).await.is_ok(), !consumed);
        let after = accounting(&f).await;
        assert_eq!(after["usage"][0]["activations"], 1);
        assert_eq!(
            after["receipts"].as_array().unwrap().len(),
            if consumed { 2 } else { 1 }
        );
    }
}
