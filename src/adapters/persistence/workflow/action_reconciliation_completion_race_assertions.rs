//! Complete public inventories, authentic conflict provenance and historical replay.
use super::*;

pub(super) fn truth(h: &HeldTruth, before: &Value, after: &Value) {
    assert_eq!(
        before["workflow_action_receipts"],
        after["workflow_action_receipts"]
    );
    append_only(
        &before["workflow_action_actual_receipt_observations"],
        &after["workflow_action_actual_receipt_observations"],
        1,
    );
    append_only(
        &before["workflow_action_evidence_conflicts"],
        &after["workflow_action_evidence_conflicts"],
        1,
    );
    let observation = after["workflow_action_actual_receipt_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["remote_entry_id"] == json!(h.entry_id))
        .unwrap();
    assert_eq!(observation["result"], h.result);
    let [conflict] = after["workflow_action_evidence_conflicts"]
        .as_array()
        .unwrap()
        .as_slice()
    else {
        panic!("one exact immutable finality breach");
    };
    let proof = &before["workflow_action_evidence"][0];
    assert_eq!(proof["disposition"], "final_not_applied");
    assert_eq!(conflict["evidence_id"], proof["id"]);
    assert_eq!(conflict["remote_entry_id"], json!(h.entry_id));
    assert_eq!(conflict["actual_observation_id"], observation["id"]);
    assert_eq!(conflict["reason"], "finality_breach");
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
        "dispatch_id",
    ] {
        assert_eq!(conflict[field], proof[field]);
        assert_eq!(observation[field], proof[field]);
        assert_eq!(after["workflow_action_receipts"][0][field], proof[field]);
    }
    assert_eq!(
        before["workflow_action_evidence_coverage"],
        after["workflow_action_evidence_coverage"]
    );
    assert_eq!(
        before["workflow_action_evidence_consumptions"],
        after["workflow_action_evidence_consumptions"]
    );
}

fn run_inventory(h: &HeldTruth, before: &Value, after: &Value, fields: &[&str]) {
    same_table_except(
        &before["workflow_runs"],
        &after["workflow_runs"],
        &json!(h.request.scope().run.as_uuid()),
        fields,
    );
    assert!(
        after["workflow_runs"][0]["revision"].as_i64().unwrap()
            > before["workflow_runs"][0]["revision"].as_i64().unwrap()
    );
}

fn job_inventory(h: &HeldTruth, before: &Value, after: &Value, status: &str) {
    same_table_except(
        &before["background_tasks"],
        &after["background_tasks"],
        &json!(h.request.fence.scope.job.0),
        &[
            "status",
            "retry_count",
            "worker_id",
            "execution_generation",
            "locked_at",
            "lock_expires_at",
            "updated_at",
        ],
    );
    let job = &after["background_tasks"][0];
    assert_eq!(job["id"], json!(h.request.fence.scope.job.0));
    assert_eq!(job["status"], status);
    let charged = if status == "failed" {
        h.request.fence.attempt.0
    } else {
        h.request.fence.attempt.0 - 1
    };
    assert_eq!(job["retry_count"], json!(charged));
    for field in [
        "worker_id",
        "execution_generation",
        "locked_at",
        "lock_expires_at",
    ] {
        assert!(job[field].is_null());
    }
}

fn attempts(h: &HeldTruth, before: &Value, after: &Value, status: &str) {
    let prior = before["task_attempts"].as_array().unwrap();
    let current = after["task_attempts"].as_array().unwrap();
    assert_eq!(prior.len(), 2);
    assert_eq!(current.len(), prior.len());
    for old in prior {
        let row = current.iter().find(|row| row["id"] == old["id"]).unwrap();
        if old["attempt_number"] != json!(h.request.fence.attempt.0) {
            assert_eq!(old, row);
            continue;
        }
        assert_eq!(row["task_id"], json!(h.request.fence.scope.job.0));
        assert_eq!(row["worker_id"], json!(h.request.fence.worker.0));
        assert_eq!(
            row["execution_generation"],
            json!(h.request.fence.generation.0)
        );
        assert_eq!(row["status"], status);
        assert!(!row["finished_at"].is_null());
        if status == "failed" {
            assert_eq!(row["workflow_failure_class"], "terminal");
            assert_eq!(row["workflow_failure_code"], "action.evidence_conflict");
            assert_eq!(row["workflow_retry_safety"], "unknown");
            assert_eq!(row["workflow_retirement"], "live");
        } else {
            assert_eq!(row["stop_reason"], "completed");
        }
        let fields: &[&str] = if status == "completed" {
            &["status", "finished_at", "stop_reason"]
        } else {
            &[
                "status",
                "finished_at",
                "workflow_failure_class",
                "workflow_failure_code",
                "workflow_retry_safety",
                "workflow_retirement",
            ]
        };
        same_except(old, row, fields);
    }
}

pub(super) fn retired(h: &HeldTruth, before: &Value, after: &Value) {
    unchanged_tables(
        before,
        after,
        &[
            "workflow_runs",
            "background_tasks",
            "task_attempts",
            "workflow_run_events",
            "workflow_action_state_witnesses",
            "workflow_action_actual_receipt_observations",
            "workflow_action_evidence_conflicts",
        ],
    );
    run_inventory(h, before, after, &["state", "waiting_reason", "revision"]);
    assert_eq!(after["workflow_runs"][0]["state"], "waiting");
    assert_eq!(
        after["workflow_runs"][0]["waiting_reason"],
        "reconciliation"
    );
    job_inventory(h, before, after, "failed");
    attempts(h, before, after, "failed");
    append_only(
        &before["workflow_run_events"],
        &after["workflow_run_events"],
        1,
    );
    state_witnesses(before, after);
    assert_eq!(before["workflow_executions"], after["workflow_executions"]);
    for execution in after["workflow_executions"].as_array().unwrap() {
        for field in [
            "completed_at",
            "committed_output",
            "committed_route",
            "successor_execution_id",
        ] {
            assert!(execution[field].is_null(), "zero transition {field}");
        }
    }
}

pub(super) fn completed(h: &HeldTruth, before: &Value, after: &Value) {
    unchanged_tables(
        before,
        after,
        &[
            "workflow_runs",
            "background_tasks",
            "task_attempts",
            "workflow_executions",
            "workflow_run_events",
            "workflow_action_state_witnesses",
        ],
    );
    run_inventory(
        h,
        before,
        after,
        &["state", "terminal_execution_id", "revision"],
    );
    assert_eq!(after["workflow_runs"][0]["state"], "succeeded");
    assert_eq!(
        after["workflow_runs"][0]["terminal_execution_id"],
        json!(h.request.scope().execution.as_uuid())
    );
    assert!(after["workflow_runs"][0]["waiting_reason"].is_null());
    job_inventory(h, before, after, "completed");
    attempts(h, before, after, "completed");
    same_table_except(
        &before["workflow_executions"],
        &after["workflow_executions"],
        &json!(h.request.scope().execution.as_uuid()),
        &[
            "completed_at",
            "committed_output",
            "committed_route",
            "route_target",
            "successor_execution_id",
        ],
    );
    let execution = &after["workflow_executions"][0];
    assert!(!execution["completed_at"].is_null());
    assert_eq!(execution["committed_output"], h.result);
    assert_eq!(execution["committed_route"], "success");
    assert_eq!(execution["route_target"], "$end");
    assert!(execution["successor_execution_id"].is_null());
    append_only(
        &before["workflow_run_events"],
        &after["workflow_run_events"],
        1,
    );
    state_witnesses(before, after);
}

pub(super) async fn inert(h: &HeldTruth, saved: &Value, first: Winner) {
    assert!(!retry_safe(&h.fixture, &h.request).await);
    let (a, b) = tokio::join!(
        h.fixture
            .persistence()
            .claim_io(h.request.fence.scope, worker(), policy()),
        h.fixture
            .persistence()
            .claim_io(h.request.fence.scope, worker(), policy()),
    );
    assert!(a.unwrap().is_none() && b.unwrap().is_none());
    let provider = LedgerProvider::new(&h.fixture, Delivery::Pending);
    assert!(
        Box::pin(invoke(&h.fixture, &h.request, &provider))
            .await
            .is_err()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    let repeated = h
        .fixture
        .persistence()
        .complete_io(FencedWorkflowResult {
            fence: h.request.fence,
            output: h.result.clone(),
        })
        .await
        .unwrap();
    match first {
        Winner::Receipt => assert!(repeated.is_none()),
        Winner::Completion => {
            assert_eq!(repeated.unwrap().disposition, CommitDisposition::Replayed)
        }
    }
    assert_eq!(
        *saved,
        all_tables(&h.fixture).await,
        "follow-ups create no transition or audit"
    );
    assert_eq!(
        effects(&h.fixture).await,
        2,
        "every genuine provider effect retained"
    );
    assert_eq!(counts(&h.fixture).await, (1, 2, 1));
    assert_eq!(projection(&h.fixture).await, ("committed".into(), true));
}
