//! Genuine scheduling facts retain every historical row and exact scope linkage.
use super::*;

pub(super) fn exact_event(
    before: &Value,
    after: &Value,
    normalized: &mut Value,
    scope: ActivationRequest,
    kind: &str,
) {
    let event = append(before, after, "workflow_run_events", 1)[0];
    let company = json!(scope.company.as_uuid());
    let run = json!(scope.run.as_uuid());
    let sequence = rows(before, "workflow_run_events")
        .iter()
        .filter(|old| old["company_id"] == company && old["run_id"] == run)
        .map(|old| old["sequence"].as_u64().unwrap())
        .max()
        .unwrap_or(0)
        + 1;
    assert_eq!(event["company_id"], company);
    assert_eq!(event["run_id"], run);
    assert_eq!(event["execution_id"], json!(scope.execution.as_uuid()));
    assert_eq!(
        event["actor_id"],
        by_id(before, "workflow_runs", scope.run.as_uuid())["actor_id"]
    );
    assert_eq!(event["event_kind"], kind);
    assert_eq!(event["sequence"], json!(sequence));
    normalized["workflow_run_events"] = before["workflow_run_events"].clone();
}

pub(super) fn state_witness(
    before: &Value,
    after: &Value,
    normalized: &mut Value,
    scope: ActivationRequest,
) -> Value {
    let witnesses = append(before, after, "workflow_action_state_witnesses", 1);
    let witness = witnesses[0];
    let old = by_id(before, "workflow_runs", scope.run.as_uuid());
    assert_eq!(witness["company_id"], json!(scope.company.as_uuid()));
    assert_eq!(witness["run_id"], json!(scope.run.as_uuid()));
    assert_eq!(witness["initial_state"], old["state"]);
    assert_eq!(witness["initial_waiting_reason"], old["waiting_reason"]);
    assert!(witness["transaction_id"].is_string());
    normalized["workflow_action_state_witnesses"] =
        before["workflow_action_state_witnesses"].clone();
    witness.clone()
}

pub(super) fn scheduled_facts(
    before: &Value,
    after: &Value,
    normalized: &mut Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    job: &Value,
) {
    let job_id: Uuid = job["id"].as_str().unwrap().parse().unwrap();
    let scope = ActivationRequest {
        company: command.scope.company,
        run: command.scope.run,
        execution: command.scope.execution,
        job: WorkflowJobId(job_id),
    };
    let state = state_witness(before, after, normalized, scope);
    assert_eq!(state["initial_state"], "waiting");
    assert_eq!(state["initial_waiting_reason"], "reconciliation");
    let episode = append(before, after, "workflow_action_claim_episodes", 1)[0];
    let schedule = append(before, after, "workflow_action_schedule_witnesses", 1)[0];
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "job_id",
        "retired_attempt",
    ] {
        assert_eq!(episode[field], schedule[field]);
    }
    assert_eq!(
        episode["company_id"],
        json!(command.scope.company.as_uuid())
    );
    assert_eq!(episode["run_id"], json!(command.scope.run.as_uuid()));
    assert_eq!(
        episode["execution_id"],
        json!(command.scope.execution.as_uuid())
    );
    assert_eq!(episode["job_id"], job["id"]);
    assert_eq!(
        episode["retired_attempt"],
        by_id(before, "background_tasks", job_id)["retry_count"]
    );
    assert_eq!(episode["command_key"], json!(command.command_key.as_str()));
    assert_eq!(schedule["transaction_id"], state["transaction_id"]);
    let receipt = by_command(after, command);
    assert_eq!(receipt["scheduled_job_id"], job["id"]);
    assert_eq!(receipt["result_revision"], json!(result.revision.0));
    for table in [
        "workflow_action_claim_episodes",
        "workflow_action_schedule_witnesses",
    ] {
        normalized[table] = before[table].clone();
    }
}
