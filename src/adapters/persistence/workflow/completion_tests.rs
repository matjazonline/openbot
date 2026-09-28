use super::*;
use crate::application::workflow::{batch::*, completion::*, supervise::*};

fn output() -> Value {
    json!({"items":[],"token_count":0})
}
fn result(fence: WorkflowFence) -> FencedWorkflowResult {
    FencedWorkflowResult {
        fence,
        output: output(),
    }
}
fn chain() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"data.map","with":{"value":{"ref":"/steps/start/output/token_count"},"output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"$end"}});
    source
}
async fn claimed(source: Value) -> (AdmissionFixture, ClaimedWorkflow) {
    let (f, scope) = fixture_source(source).await;
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    (f, claim)
}
async fn state(f: &AdmissionFixture) -> Value {
    let mut state = snapshot(f).await;
    state["events"] = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(event) ORDER BY sequence) FROM workflow_run_events AS event",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    state
}

#[tokio::test]
async fn workflow_completion_competing_results_lost_ack_and_saved_replay() {
    let (f, claim) = claimed(chain()).await;
    let barrier = Barrier::new(2);
    let compete = || async {
        barrier.wait().await;
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .unwrap()
            .unwrap()
    };
    let (a, b) = tokio::join!(compete(), compete());
    assert_ne!(a.disposition, b.disposition);
    assert_eq!(a.successor, b.successor);
    let next = a.successor.unwrap();
    let saved = state(&f).await;
    assert_eq!(saved["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(saved["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(saved["attempts"][0]["status"], "completed");
    assert_eq!(
        saved["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["event_kind"] == "io_step_completed")
            .count(),
        1
    );
    let job = saved["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == json!(claim.fence.scope.job.0))
        .unwrap();
    assert_eq!(job["status"], "completed");
    assert_eq!(job["lock_expires_at"], Value::Null);
    assert_eq!(job["retry_count"], 0);
    let mut different = result(claim.fence);
    different.output["token_count"] = json!(1);
    assert!(
        f.persistence()
            .complete_io(different)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&f).await, saved);
    let done = f
        .persistence()
        .advance_pure(
            next,
            BatchBudget::new(2, Duration::from_millis(100)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(done.disposition, BatchDisposition::Completed);
    // A new connection after lost commit acknowledgement observes the saved result
    // even though the successor has already finished the run.
    let options = f.persistence().pool().connect_options();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*options).clone())
        .await
        .unwrap();
    let reconnect = PostgresPersistence::new(pool.clone());
    let before = state(&f).await;
    let replay = reconnect
        .complete_io(result(claim.fence))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.output, output());
    assert_eq!(replay.successor, Some(next));
    assert_eq!(state(&f).await, before);
    pool.close().await;
}

#[tokio::test]
async fn workflow_completion_pure_io_supervision_and_terminal_io() {
    let mut source = chain();
    source["entry"] = json!("prepare");
    source["steps"]["prepare"] = json!({"type":"data.map","with":{"value":{"literal":10},"output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"start"}});
    let (f, scope) = fixture_source(source).await;
    let boundary = f
        .persistence()
        .advance_pure(
            scope,
            BatchBudget::new(10, Duration::from_millis(100)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(boundary.disposition, BatchDisposition::Boundary);
    let claim = f
        .persistence()
        .claim_io(boundary.continuation.unwrap(), worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let work = async { Ok(output()) };
    let supervised = supervise_io(
        f.persistence(),
        claim,
        policy(),
        Instant::now() + Duration::from_secs(2),
        work,
        std::future::pending(),
    )
    .await;
    let SupervisedResult::Ready(result) = supervised else {
        panic!("handler failed")
    };
    let saved = f.persistence().complete_io(result).await.unwrap().unwrap();
    f.persistence()
        .advance_pure(
            saved.successor.unwrap(),
            BatchBudget::new(10, Duration::from_millis(100)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(state(&f).await["runs"][0]["state"], "succeeded");
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert!(
        f.persistence()
            .complete_io(super::completion_tests::result(claim.fence))
            .await
            .unwrap()
            .unwrap()
            .successor
            .is_none()
    );
    assert_eq!(state(&f).await["runs"][0]["state"], "succeeded");
}

#[tokio::test]
async fn workflow_completion_rolls_back_every_write_boundary() {
    for (table, condition) in [
        ("workflow_executions", "TG_OP='INSERT'"),
        ("background_tasks", "TG_OP='INSERT'"),
        ("workflow_executions", "NEW.completed_at IS NOT NULL"),
        ("workflow_runs", "true"),
        ("workflow_run_events", "true"),
        ("task_attempts", "NEW.status='completed'"),
        ("background_tasks", "NEW.status='completed'"),
    ] {
        let (f, claim) = claimed(chain()).await;
        let before = state(&f).await;
        sqlx::raw_sql(&format!("CREATE FUNCTION completion_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {condition} THEN RAISE EXCEPTION 'injected completion failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER completion_fault AFTER INSERT OR UPDATE ON {table} FOR EACH ROW EXECUTE FUNCTION completion_fault();")).execute(f.persistence().pool()).await.unwrap();
        assert!(
            f.persistence()
                .complete_io(result(claim.fence))
                .await
                .is_err(),
            "{table} {condition}"
        );
        assert_eq!(state(&f).await, before, "{table} {condition}");
        sqlx::raw_sql(&format!(
            "DROP TRIGGER completion_fault ON {table}; DROP FUNCTION completion_fault();"
        ))
        .execute(f.persistence().pool())
        .await
        .unwrap();
        assert!(
            f.persistence()
                .complete_io(result(claim.fence))
                .await
                .unwrap()
                .is_some()
        );
    }
}

#[tokio::test]
async fn workflow_completion_rejects_wrong_fences() {
    let (f, claim) = claimed(chain()).await;
    let before = state(&f).await;
    for wrong in [
        WorkflowFence {
            worker: worker(),
            ..claim.fence
        },
        WorkflowFence {
            generation: WorkflowGeneration(Uuid::new_v4()),
            ..claim.fence
        },
        WorkflowFence {
            attempt: WorkflowAttempt(2),
            ..claim.fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                run: RunId::new(Uuid::new_v4()),
                ..claim.fence.scope
            },
            ..claim.fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                job: WorkflowJobId(Uuid::new_v4()),
                ..claim.fence.scope
            },
            ..claim.fence
        },
    ] {
        assert!(
            f.persistence()
                .complete_io(result(wrong))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(state(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_completion_reclaimed_generation_refuses() {
    let (f, claim) = claimed(chain()).await;
    expire(&f, claim.fence.scope).await;
    assert!(
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.persistence()
            .claim_io(claim.fence.scope, claim.fence.worker, policy())
            .await
            .unwrap()
            .is_none()
    );
    due(&f, claim.fence.scope).await;
    let replacement = f
        .persistence()
        .claim_io(claim.fence.scope, claim.fence.worker, policy())
        .await
        .unwrap()
        .unwrap();
    let before = state(&f).await;
    assert!(
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&f).await, before);
    f.persistence()
        .complete_io(result(replacement.fence))
        .await
        .unwrap()
        .unwrap();
    assert!(
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .unwrap()
            .is_none()
    );
    let state = state(&f).await;
    assert_eq!(
        state["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["status"] == "completed")
            .count(),
        1
    );
    assert_eq!(
        state["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["status"] == "failed")
            .count(),
        1
    );
}

#[tokio::test]
async fn workflow_completion_invalid_outputs_and_activation_budget_roll_back() {
    let (f, claim) = claimed(chain()).await;
    let before = state(&f).await;
    for output in [
        Value::Null,
        json!({"items":[],"token_count":-1}),
        json!({"items":[],"token_count":0,"extra":"x".repeat(70000)}),
    ] {
        assert!(
            f.persistence()
                .complete_io(FencedWorkflowResult {
                    fence: claim.fence,
                    output
                })
                .await
                .is_err()
        );
        assert_eq!(state(&f).await, before);
    }
    sqlx::query("UPDATE workflow_runs SET max_steps=1 WHERE id=$1")
        .bind(claim.fence.scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let before = state(&f).await;
    assert!(
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .is_err()
    );
    assert_eq!(state(&f).await, before);
}

#[path = "completion_race_tests.rs"]
mod race_tests;

#[path = "completion_refusal_tests.rs"]
mod refusal_tests;
