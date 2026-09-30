//! Exact evidence, command, coverage and receipt boundaries.
use super::*;

pub(super) fn actual_receipt(
    before: &Value,
    after: &Value,
    request: &ActionDispatchRequest,
    entry: &Value,
    receipt: &Value,
) {
    let observation = append(
        before,
        after,
        "workflow_action_actual_receipt_observations",
        1,
    )[0];
    scope(observation, request);
    let markers: Vec<_> = rows(before, "workflow_action_dispatches")
        .iter()
        .filter(|marker| marker["invocation_id"] == json!(request.subject.invocation.as_uuid()))
        .collect();
    assert_eq!(
        markers.len(),
        1,
        "original supported marker remains authoritative"
    );
    for row in [entry, receipt, observation] {
        assert_eq!(row["dispatch_id"], markers[0]["id"]);
    }
    assert_eq!(observation["remote_entry_id"], entry["id"]);
    assert_eq!(observation["remote_entry_id"], receipt["remote_entry_id"]);
    assert_eq!(observation["result"], good());
    assert_eq!(observation["result"], receipt["result"]);
    assert_eq!(receipt["effect_kind"], "remote");
    assert!(observation["id"].as_str().unwrap().parse::<Uuid>().is_ok());
    assert!(observation["created_at"].is_string());
    let digest = observation["result_digest"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

pub(super) enum EvidenceChange {
    Audit,
    Receipt,
    Schedule,
}
pub(super) fn evidence_boundary(
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    change: EvidenceChange,
) {
    let mut normalized = after.clone();
    let fact = evidence_fact(before, after, command, result);
    evidence_links(before, after, command, result, fact, &mut normalized);
    recovered_receipt(before, after, result, fact, &change, &mut normalized);
    evidence_owners(before, after, command, result, change, normalized);
}
fn evidence_fact<'a>(
    before: &Value,
    after: &'a Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
) -> &'a Value {
    let evidence_id = result.evidence.unwrap().as_uuid();
    let facts = append(before, after, "workflow_action_evidence", 1);
    let fact = facts[0];
    assert_eq!(fact["id"], json!(evidence_id));
    for field in ["company_id", "run_id", "execution_id"] {
        let expected = match field {
            "company_id" => command.scope.company.as_uuid(),
            "run_id" => command.scope.run.as_uuid(),
            _ => command.scope.execution.as_uuid(),
        };
        assert_eq!(fact[field], json!(expected));
    }
    assert_eq!(
        fact["invocation_id"],
        json!(command.subject.invocation.as_uuid())
    );
    assert_eq!(
        fact["argument_digest"],
        json!(command.subject.argument_digest.as_str())
    );
    assert_eq!(fact["dispatch_id"], json!(command.marker.as_uuid()));
    assert_eq!(fact["command_key"], json!(command.command_key.as_str()));
    assert_eq!(fact["disposition"], "applied");
    assert_eq!(fact["applied_request"], "remote_entry");
    let entry = by_id(
        after,
        "workflow_action_remote_entries",
        fact["applied_remote_entry_id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
    );
    assert_eq!(entry["invocation_id"], fact["invocation_id"]);
    let coverage = append(before, after, "workflow_action_evidence_coverage", 1);
    assert_eq!(coverage[0]["evidence_id"], json!(evidence_id));
    assert_eq!(
        coverage[0]["remote_entry_id"],
        fact["applied_remote_entry_id"]
    );
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
        "dispatch_id",
    ] {
        assert_eq!(coverage[0][field], fact[field]);
    }
    fact
}
fn evidence_links(
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    fact: &Value,
    normalized: &mut Value,
) {
    let evidence_id = result.evidence.unwrap().as_uuid();
    let commands = append(before, after, "workflow_action_evidence_commands", 1);
    assert_eq!(commands[0], by_command(after, command));
    assert_eq!(commands[0]["id"], fact["command_id"]);
    assert_eq!(commands[0]["evidence_id"], json!(evidence_id));
    assert_eq!(commands[0]["result_revision"], json!(result.revision.0));
    let events = append(before, after, "workflow_run_events", 1);
    assert_eq!(events[0]["company_id"], fact["company_id"]);
    assert_eq!(events[0]["run_id"], fact["run_id"]);
    assert_eq!(events[0]["execution_id"], fact["execution_id"]);
    assert_eq!(events[0]["actor_id"], fact["actor_id"]);
    assert_eq!(events[0]["event_kind"], "action_reconciled");
    assert_eq!(events[0]["sequence"], commands[0]["audit_sequence"]);
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_commands",
        "workflow_run_events",
    ] {
        normalized[table] = before[table].clone();
    }
}
fn recovered_receipt(
    before: &Value,
    after: &Value,
    result: &ReconciliationResult,
    fact: &Value,
    change: &EvidenceChange,
    normalized: &mut Value,
) {
    let evidence_id = result.evidence.unwrap().as_uuid();
    if matches!(change, EvidenceChange::Receipt | EvidenceChange::Schedule) {
        let receipts = append(before, after, "workflow_action_receipts", 1);
        for field in [
            "company_id",
            "run_id",
            "execution_id",
            "invocation_id",
            "argument_digest",
            "dispatch_id",
        ] {
            assert_eq!(receipts[0][field], fact[field]);
        }
        assert_eq!(
            receipts[0]["reconciliation_evidence_id"],
            json!(evidence_id)
        );
        assert!(receipts[0]["remote_entry_id"].is_null());
        assert_eq!(receipts[0]["result"], good());
        normalized["workflow_action_receipts"] = before["workflow_action_receipts"].clone();
    }
}
fn evidence_owners(
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    change: EvidenceChange,
    mut normalized: Value,
) {
    let run_id = command.scope.run.as_uuid();
    assert_eq!(
        by_id(after, "workflow_runs", run_id)["revision"],
        json!(result.revision.0)
    );
    if matches!(change, EvidenceChange::Schedule) {
        owner(
            &mut normalized,
            before,
            "workflow_runs",
            run_id,
            &["state", "waiting_reason", "revision"],
        );
        let jobs: Vec<_> = rows(after, "background_tasks")
            .iter()
            .filter(|row| row["workflow_execution_id"] == json!(command.scope.execution.as_uuid()))
            .collect();
        assert_eq!(jobs.len(), 1, "same existing job");
        assert_eq!(jobs[0]["status"], "pending");
        scheduled_facts(before, after, &mut normalized, command, result, jobs[0]);
        owner(
            &mut normalized,
            before,
            "background_tasks",
            jobs[0]["id"].as_str().unwrap().parse().unwrap(),
            &["status", "run_at", "updated_at"],
        );
        assert_eq!(by_id(after, "workflow_runs", run_id)["state"], "running");
        assert!(by_id(after, "workflow_runs", run_id)["waiting_reason"].is_null());
    } else {
        owner(
            &mut normalized,
            before,
            "workflow_runs",
            run_id,
            &["revision"],
        );
    }
    inventory(before, after, normalized);
}
