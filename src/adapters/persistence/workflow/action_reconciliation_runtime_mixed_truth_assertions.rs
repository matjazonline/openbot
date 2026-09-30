//! Complete public inventories with only exact owner rows and scoped append facts allowed.
use super::*;

pub(super) fn rows<'a>(state: &'a Value, table: &str) -> &'a [Value] {
    state[table].as_array().unwrap()
}
pub(super) fn by_id<'a>(state: &'a Value, table: &str, id: Uuid) -> &'a Value {
    let found: Vec<_> = rows(state, table)
        .iter()
        .filter(|row| row["id"] == json!(id))
        .collect();
    assert_eq!(found.len(), 1, "exact {table} identity");
    found[0]
}
pub(super) fn by_command<'a>(state: &'a Value, command: &ReconcileActionCommand) -> &'a Value {
    let found: Vec<_> = rows(state, "workflow_action_evidence_commands")
        .iter()
        .filter(|row| row["command_key"] == json!(command.command_key.as_str()))
        .collect();
    assert_eq!(found.len(), 1);
    found[0]
}
pub(super) fn append<'a>(
    before: &Value,
    after: &'a Value,
    table: &str,
    count: usize,
) -> Vec<&'a Value> {
    let old = rows(before, table);
    let new = rows(after, table);
    assert_eq!(new.len(), old.len() + count, "exact {table} inventory");
    for row in old {
        assert!(new.contains(row), "every old {table} row retains its bytes");
    }
    new.iter().filter(|row| !old.contains(row)).collect()
}
pub(super) fn scope(row: &Value, request: &ActionDispatchRequest) {
    assert_eq!(row["company_id"], json!(request.scope().company.as_uuid()));
    assert_eq!(row["run_id"], json!(request.scope().run.as_uuid()));
    assert_eq!(
        row["execution_id"],
        json!(request.scope().execution.as_uuid())
    );
    assert_eq!(
        row["invocation_id"],
        json!(request.subject.invocation.as_uuid())
    );
    assert_eq!(
        row["argument_digest"],
        json!(request.subject.argument_digest.as_str())
    );
}
pub(super) fn owner(
    normalized: &mut Value,
    before: &Value,
    table: &str,
    id: Uuid,
    fields: &[&str],
) {
    let old = by_id(before, table, id);
    let current = by_id(normalized, table, id);
    let mut restored = current.clone();
    for field in fields {
        restored[*field] = old[*field].clone();
    }
    assert_eq!(
        old, &restored,
        "only intended exact {table} owner fields change"
    );
    let current = normalized[table]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == json!(id))
        .unwrap();
    *current = old.clone();
}
pub(super) fn inventory(before: &Value, after: &Value, normalized: Value) {
    assert_eq!(
        before.as_object().unwrap().keys().collect::<Vec<_>>(),
        after.as_object().unwrap().keys().collect::<Vec<_>>(),
        "every public table is present"
    );
    for (table, old) in before.as_object().unwrap() {
        assert_eq!(
            *old, normalized[table],
            "complete public inventory: {table}"
        );
    }
}

pub(super) fn attempt(state: &Value, fence: WorkflowFence) -> &Value {
    let found: Vec<_> = rows(state, "task_attempts")
        .iter()
        .filter(|row| {
            row["task_id"] == json!(fence.scope.job.0)
                && row["attempt_number"] == json!(fence.attempt.0)
        })
        .collect();
    assert_eq!(found.len(), 1, "exact attempt identity");
    found[0]
}
pub(super) fn live_fence(state: &Value, fence: WorkflowFence) {
    let job = by_id(state, "background_tasks", fence.scope.job.0);
    assert_eq!(job["status"], "processing");
    assert_eq!(job["worker_id"], json!(fence.worker.0));
    assert_eq!(job["execution_generation"], json!(fence.generation.0));
    assert_eq!(
        job["retry_count"].as_i64().unwrap() + 1,
        i64::from(fence.attempt.0)
    );
    let entry = attempt(state, fence);
    assert_eq!(entry["status"], "processing");
    assert_eq!(entry["worker_id"], job["worker_id"]);
    assert_eq!(entry["execution_generation"], job["execution_generation"]);
    assert!(entry["finished_at"].is_null());
}
pub(super) fn parked_fence(before: &Value, after: &Value, request: &ActionDispatchRequest) {
    let fence = request.fence;
    let job = by_id(after, "background_tasks", fence.scope.job.0);
    assert_eq!(job["status"], "failed");
    for field in [
        "worker_id",
        "execution_generation",
        "locked_at",
        "lock_expires_at",
    ] {
        assert!(job[field].is_null());
    }
    let closed = attempt(after, fence);
    assert_eq!(closed["status"], "failed");
    assert_eq!(closed["workflow_retry_safety"], "unknown");
    assert_eq!(closed["workflow_retirement"], "live");
    assert_eq!(closed["workflow_failure_class"], "retryable");
    assert_eq!(closed["workflow_failure_code"], "workflow.interrupted");
    assert!(closed["stop_reason"].is_null());
    assert_eq!(closed["worker_id"], json!(fence.worker.0));
    assert_eq!(closed["execution_generation"], json!(fence.generation.0));
    assert!(closed["finished_at"].is_string());
    assert_eq!(
        by_id(after, "workflow_runs", fence.scope.run.as_uuid())["state"],
        "waiting"
    );
    assert_eq!(
        by_id(after, "workflow_runs", fence.scope.run.as_uuid())["waiting_reason"],
        "reconciliation"
    );
    let mut normalized = after.clone();
    owner(
        &mut normalized,
        before,
        "background_tasks",
        fence.scope.job.0,
        &[
            "status",
            "retry_count",
            "run_at",
            "updated_at",
            "worker_id",
            "execution_generation",
            "locked_at",
            "lock_expires_at",
        ],
    );
    owner(
        &mut normalized,
        before,
        "workflow_runs",
        fence.scope.run.as_uuid(),
        &["state", "waiting_reason", "revision"],
    );
    parked_attempt(before, after, &mut normalized, fence);
    parked_facts(before, after, &mut normalized, fence);
    inventory(before, after, normalized);
}

fn parked_attempt(before: &Value, after: &Value, normalized: &mut Value, fence: WorkflowFence) {
    let closed = attempt(after, fence);
    let mut restored = closed.clone();
    for field in [
        "status",
        "finished_at",
        "workflow_failure_class",
        "workflow_failure_code",
        "workflow_retry_safety",
        "workflow_retirement",
        "stop_reason",
    ] {
        restored[field] = attempt(before, fence)[field].clone();
    }
    assert_eq!(restored, *attempt(before, fence));
    for old in rows(before, "task_attempts")
        .iter()
        .filter(|row| *row != attempt(before, fence))
    {
        assert!(
            rows(after, "task_attempts").contains(old),
            "all prior and unrelated attempts retain bytes"
        );
    }
    normalized["task_attempts"] = before["task_attempts"].clone();
    assert_eq!(
        rows(before, "task_attempts").len(),
        rows(after, "task_attempts").len()
    );
}

fn parked_facts(before: &Value, after: &Value, normalized: &mut Value, fence: WorkflowFence) {
    // Only markers without a canonical receipt acquire first-uncertainty
    // observations during genuine retirement.
    let unresolved: Vec<_> = rows(before, "workflow_action_dispatches")
        .iter()
        .filter(|marker| {
            marker["execution_id"] == json!(fence.scope.execution.as_uuid())
                && !rows(before, "workflow_action_receipts")
                    .iter()
                    .any(|receipt| receipt["invocation_id"] == marker["invocation_id"])
        })
        .collect();
    for observation in append(
        before,
        after,
        "workflow_action_reconciliations",
        unresolved.len(),
    ) {
        let markers: Vec<_> = unresolved
            .iter()
            .filter(|marker| marker["id"] == observation["dispatch_id"])
            .collect();
        assert_eq!(markers.len(), 1);
        for field in [
            "company_id",
            "run_id",
            "execution_id",
            "invocation_id",
            "argument_digest",
        ] {
            assert_eq!(observation[field], markers[0][field]);
        }
        assert_eq!(
            observation["company_id"],
            json!(fence.scope.company.as_uuid())
        );
        assert_eq!(observation["run_id"], json!(fence.scope.run.as_uuid()));
        assert_eq!(
            observation["execution_id"],
            json!(fence.scope.execution.as_uuid())
        );
        assert_eq!(observation["effect_kind"], "remote");
        assert_eq!(observation["failure_code"], "workflow.interrupted");
        assert!(observation["observed_at"].is_string());
        assert!(
            !rows(after, "workflow_action_receipts")
                .iter()
                .any(|receipt| receipt["invocation_id"] == observation["invocation_id"])
        );
    }
    normalized["workflow_action_reconciliations"] =
        before["workflow_action_reconciliations"].clone();
    state_witness(before, after, normalized, fence.scope);
    exact_event(
        before,
        after,
        normalized,
        fence.scope,
        &format!("attempt_failed:{}", fence.attempt.0),
    );
}

pub(super) fn retrieval_boundary(before: &Value, after: &Value, request: &ActionDispatchRequest) {
    let mut normalized = after.clone();
    let old: Vec<_> = rows(before, "fixture_evidence_ledger")
        .iter()
        .filter(|row| row["invocation_id"] == json!(request.subject.invocation.as_uuid()))
        .collect();
    assert_eq!(old.len(), 1);
    let changed = normalized["fixture_evidence_ledger"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["entry_id"] == old[0]["entry_id"])
        .unwrap();
    assert_eq!(
        changed["result"],
        good(),
        "same authentic durable result becomes retrievable"
    );
    assert_eq!(changed["recover_result"], true);
    changed["recover_result"] = old[0]["recover_result"].clone();
    changed["observed_at"] = old[0]["observed_at"].clone();
    inventory(before, after, normalized);
}

pub(super) fn claim_boundary(before: &Value, after: &Value, fence: WorkflowFence) {
    live_fence(after, fence);
    let mut normalized = after.clone();
    assert_eq!(
        by_id(after, "workflow_runs", fence.scope.run.as_uuid())["revision"]
            .as_u64()
            .unwrap(),
        by_id(before, "workflow_runs", fence.scope.run.as_uuid())["revision"]
            .as_u64()
            .unwrap()
            + 1
    );
    owner(
        &mut normalized,
        before,
        "workflow_runs",
        fence.scope.run.as_uuid(),
        &["revision"],
    );
    owner(
        &mut normalized,
        before,
        "background_tasks",
        fence.scope.job.0,
        &[
            "status",
            "worker_id",
            "execution_generation",
            "locked_at",
            "lock_expires_at",
            "updated_at",
        ],
    );
    let added = append(before, after, "task_attempts", 1);
    assert_eq!(added[0], attempt(after, fence));
    normalized["task_attempts"] = before["task_attempts"].clone();
    inventory(before, after, normalized);
}

pub(super) fn supported_boundary(before: &Value, after: &Value, request: &ActionDispatchRequest) {
    let mut normalized = after.clone();
    let entries = append(before, after, "workflow_action_remote_entries", 1);
    scope(entries[0], request);
    assert_eq!(entries[0]["worker_id"], json!(request.fence.worker.0));
    assert_eq!(
        entries[0]["execution_generation"],
        json!(request.fence.generation.0)
    );
    let receipts = append(before, after, "workflow_action_receipts", 1);
    scope(receipts[0], request);
    assert_eq!(receipts[0]["result"], good());
    assert_eq!(receipts[0]["remote_entry_id"], entries[0]["id"]);
    assert!(receipts[0]["reconciliation_evidence_id"].is_null());
    actual_receipt(before, after, request, entries[0], receipts[0]);
    let ledger = append(before, after, "fixture_evidence_ledger", 1);
    assert_eq!(ledger[0]["entry_id"], entries[0]["id"]);
    assert_eq!(ledger[0]["result"], good());
    let effects = append(before, after, "fixture_provider_effects", 1);
    assert_eq!(effects[0]["entry_id"], entries[0]["id"]);
    assert_eq!(effects[0]["result"], good());
    for table in [
        "workflow_action_remote_entries",
        "workflow_action_receipts",
        "workflow_action_actual_receipt_observations",
        "fixture_evidence_ledger",
        "fixture_provider_effects",
    ] {
        normalized[table] = before[table].clone();
    }
    inventory(before, after, normalized);
}

pub(super) fn completed_boundary(before: &Value, after: &Value, fence: WorkflowFence) {
    let mut normalized = after.clone();
    let run = by_id(after, "workflow_runs", fence.scope.run.as_uuid());
    assert_eq!(
        run["revision"].as_u64().unwrap(),
        by_id(before, "workflow_runs", fence.scope.run.as_uuid())["revision"]
            .as_u64()
            .unwrap()
            + 3
    );
    assert!(run["waiting_reason"].is_null());
    let execution = by_id(
        after,
        "workflow_executions",
        fence.scope.execution.as_uuid(),
    );
    assert_eq!(execution["committed_output"], good());
    assert_eq!(execution["committed_route"], "success");
    assert_eq!(execution["route_target"], "$end");
    assert_eq!(
        by_id(after, "workflow_runs", fence.scope.run.as_uuid())["terminal_execution_id"],
        json!(fence.scope.execution.as_uuid())
    );
    assert!(execution["successor_execution_id"].is_null());
    assert!(execution["completed_at"].is_string());
    assert_eq!(
        by_id(after, "workflow_runs", fence.scope.run.as_uuid())["state"],
        "succeeded"
    );
    assert_eq!(
        by_id(after, "background_tasks", fence.scope.job.0)["status"],
        "completed"
    );
    for field in [
        "worker_id",
        "execution_generation",
        "locked_at",
        "lock_expires_at",
    ] {
        assert!(by_id(after, "background_tasks", fence.scope.job.0)[field].is_null());
    }
    completed_owners(before, &mut normalized, fence);
    let mut closed = attempt(after, fence).clone();
    assert_eq!(closed["status"], "completed");
    assert_eq!(closed["stop_reason"], "completed");
    assert!(closed["finished_at"].is_string());
    for field in ["status", "finished_at", "stop_reason"] {
        closed[field] = attempt(before, fence)[field].clone();
    }
    assert_eq!(closed, *attempt(before, fence));
    assert_eq!(
        rows(before, "task_attempts").len(),
        rows(after, "task_attempts").len()
    );
    for old in rows(before, "task_attempts")
        .iter()
        .filter(|row| *row != attempt(before, fence))
    {
        assert!(rows(after, "task_attempts").contains(old));
    }
    normalized["task_attempts"] = before["task_attempts"].clone();
    state_witness(before, after, &mut normalized, fence.scope);
    exact_event(
        before,
        after,
        &mut normalized,
        fence.scope,
        "io_step_completed",
    );
    inventory(before, after, normalized);
}

fn completed_owners(before: &Value, normalized: &mut Value, fence: WorkflowFence) {
    owner(
        normalized,
        before,
        "workflow_runs",
        fence.scope.run.as_uuid(),
        &[
            "state",
            "waiting_reason",
            "revision",
            "terminal_execution_id",
        ],
    );
    owner(
        normalized,
        before,
        "background_tasks",
        fence.scope.job.0,
        &[
            "status",
            "updated_at",
            "worker_id",
            "execution_generation",
            "locked_at",
            "lock_expires_at",
        ],
    );
    owner(
        normalized,
        before,
        "workflow_executions",
        fence.scope.execution.as_uuid(),
        &[
            "committed_output",
            "committed_route",
            "route_target",
            "successor_execution_id",
            "completed_at",
        ],
    );
}
