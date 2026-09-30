//! Audit facts append without replacing any canonical truth or earlier public row.
use super::*;

pub(super) struct TruthChange<'a> {
    pub disposition: &'a str,
    pub recovered: Option<Value>,
    pub receipt: bool,
    pub contradiction: Option<ActionEvidenceId>,
}

fn command_scope(row: &Value, command: &ReconcileActionCommand) {
    assert_eq!(row["company_id"], json!(command.scope.company.as_uuid()));
    assert_eq!(row["run_id"], json!(command.scope.run.as_uuid()));
    assert_eq!(
        row["execution_id"],
        json!(command.scope.execution.as_uuid())
    );
    assert_eq!(
        row["invocation_id"],
        json!(command.subject.invocation.as_uuid())
    );
    assert_eq!(
        row["argument_digest"],
        json!(command.subject.argument_digest.as_str())
    );
}

fn evidence_and_coverage<'a>(
    before: &Value,
    after: &'a Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    change: &TruthChange<'_>,
) -> &'a Value {
    let evidence_id = result.evidence.unwrap().as_uuid();
    let fact = append(before, after, "workflow_action_evidence", 1)[0];
    command_scope(fact, command);
    assert_eq!(fact["id"], json!(evidence_id));
    assert_eq!(fact["dispatch_id"], json!(command.marker.as_uuid()));
    assert_eq!(fact["actor_id"], json!(command.actor.user_id()));
    assert_eq!(fact["command_key"], json!(command.command_key.as_str()));
    assert_eq!(fact["disposition"], change.disposition);
    if let EvidenceInput::VerifiedReference {
        registration,
        reference,
    } = &command.input
    {
        assert_eq!(fact["registration"], json!(registration.as_str()));
        assert_eq!(fact["authoritative_reference"], json!(reference.as_str()));
        assert!(fact["verifier_version"].is_string());
        assert!(fact["provider"].is_string());
        assert!(fact["operation_signature"].is_string());
    } else {
        panic!("retained truth is verified by the registered service");
    }
    assert_eq!(
        fact["grant_eligible"],
        change.disposition == "final_not_applied"
    );
    let covered: Vec<_> = rows(before, "workflow_action_remote_entries")
        .iter()
        .filter(|entry| entry["invocation_id"] == fact["invocation_id"])
        .collect();
    let coverage = append(
        before,
        after,
        "workflow_action_evidence_coverage",
        covered.len(),
    );
    for entry in covered {
        let linked: Vec<_> = coverage
            .iter()
            .filter(|row| row["remote_entry_id"] == entry["id"])
            .collect();
        assert_eq!(linked.len(), 1);
        command_scope(linked[0], command);
        assert_eq!(linked[0]["evidence_id"], fact["id"]);
        assert_eq!(linked[0]["dispatch_id"], fact["dispatch_id"]);
    }
    fact
}

fn command_and_audit(
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    fact: &Value,
) {
    let saved = append(before, after, "workflow_action_evidence_commands", 1)[0];
    command_scope(saved, command);
    assert_eq!(saved, by_command(after, command));
    assert_eq!(saved["id"], fact["command_id"]);
    assert_eq!(saved["evidence_id"], fact["id"]);
    assert_eq!(saved["outcome"], json!(result.outcome));
    assert_eq!(saved["result_revision"], json!(result.revision.0));
    assert!(saved["scheduled_job_id"].is_null());
    let event = append(before, after, "workflow_run_events", 1)[0];
    for field in ["company_id", "run_id", "execution_id", "actor_id"] {
        assert_eq!(event[field], fact[field]);
    }
    assert_eq!(event["event_kind"], "action_reconciled");
    assert_eq!(event["sequence"], saved["audit_sequence"]);
    let previous = rows(before, "workflow_run_events")
        .iter()
        .filter(|row| row["run_id"] == fact["run_id"])
        .map(|row| row["sequence"].as_i64().unwrap())
        .max()
        .unwrap();
    assert_eq!(event["sequence"], json!(previous + 1));
}

fn receipt_and_conflict(
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    fact: &Value,
    change: &TruthChange<'_>,
    normalized: &mut Value,
) {
    if change.receipt {
        let receipt = append(before, after, "workflow_action_receipts", 1)[0];
        command_scope(receipt, command);
        assert_eq!(receipt["reconciliation_evidence_id"], fact["id"]);
        assert_eq!(receipt["dispatch_id"], fact["dispatch_id"]);
        assert_eq!(receipt["result"], json!(change.recovered));
        assert_eq!(receipt["effect_kind"], "remote");
        assert!(receipt["remote_entry_id"].is_null());
        normalized["workflow_action_receipts"] = before["workflow_action_receipts"].clone();
    }
    if let Some(prior) = change.contradiction {
        let conflict = append(before, after, "workflow_action_evidence_conflicts", 1)[0];
        command_scope(conflict, command);
        assert_eq!(conflict["dispatch_id"], fact["dispatch_id"]);
        assert_eq!(conflict["evidence_id"], fact["id"]);
        assert_eq!(
            conflict["contradictory_evidence_id"],
            json!(prior.as_uuid())
        );
        assert_eq!(conflict["reason"], "evidence_disagreement");
        assert!(conflict["remote_entry_id"].is_null());
        assert!(conflict["actual_observation_id"].is_null());
        normalized["workflow_action_evidence_conflicts"] =
            before["workflow_action_evidence_conflicts"].clone();
    }
}

pub(super) fn truth_boundary(
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    change: TruthChange<'_>,
) {
    let mut normalized = after.clone();
    let fact = evidence_and_coverage(before, after, command, result, &change);
    command_and_audit(before, after, command, result, fact);
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_commands",
        "workflow_run_events",
    ] {
        normalized[table] = before[table].clone();
    }
    receipt_and_conflict(before, after, command, fact, &change, &mut normalized);
    let run_id = command.scope.run.as_uuid();
    let old_revision = by_id(before, "workflow_runs", run_id)["revision"]
        .as_u64()
        .unwrap();
    assert_eq!(
        by_id(after, "workflow_runs", run_id)["revision"],
        json!(result.revision.0)
    );
    assert!(result.revision.0 > old_revision);
    owner(
        &mut normalized,
        before,
        "workflow_runs",
        run_id,
        &["revision"],
    );
    inventory(before, after, normalized);
}
