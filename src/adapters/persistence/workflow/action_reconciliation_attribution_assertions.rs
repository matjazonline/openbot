//! Exact native attribution links and every-public-row preservation.
use super::*;

struct NativeGuard {
    name: &'static str,
    relation: &'static str,
    migration: &'static str,
    timing: i16,
    deferred: bool,
}

fn guard_definitions() -> [NativeGuard; 5] {
    let binding =
        include_str!("../../../../migrations/20260930181000_workflow_action_evidence_binding.sql");
    let dispatch =
        include_str!("../../../../migrations/20260930120000_workflow_action_dispatch.sql");
    let observations = include_str!(
        "../../../../migrations/20260930183000_workflow_action_observation_guards.sql"
    );
    [
        NativeGuard {
            name: "workflow_action_dispatch_commit_guard",
            relation: "workflow_action_dispatches",
            migration: dispatch,
            timing: 5,
            deferred: true,
        },
        NativeGuard {
            name: "workflow_action_applied_request_guard",
            relation: "workflow_action_evidence",
            migration: binding,
            timing: 7,
            deferred: false,
        },
        NativeGuard {
            name: "workflow_action_evidence_commit_guard",
            relation: "workflow_action_evidence",
            migration: binding,
            timing: 5,
            deferred: true,
        },
        NativeGuard {
            name: "workflow_action_receipt_finality_conflict",
            relation: "workflow_action_receipts",
            migration: observations,
            timing: 5,
            deferred: false,
        },
        NativeGuard {
            name: "workflow_action_conflict_park_pending",
            relation: "workflow_action_evidence_conflicts",
            migration: binding,
            timing: 5,
            deferred: false,
        },
    ]
}

pub(super) async fn native_catalog(f: &AdmissionFixture) -> Value {
    let definitions = guard_definitions();
    let names: Vec<_> = definitions
        .iter()
        .map(|definition| definition.name)
        .collect();
    let catalog: Value = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(native) ORDER BY native.name) FROM (SELECT trigger.tgname AS name,relation.relname AS relation,procedure.proname AS function,trigger.tgenabled AS enabled,trigger.tgtype AS timing,trigger.tgdeferrable AS deferrable,trigger.tginitdeferred AS deferred,procedure.prosrc AS body,pg_get_triggerdef(trigger.oid) AS definition FROM pg_trigger AS trigger JOIN pg_class AS relation ON relation.oid=trigger.tgrelid JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace JOIN pg_proc AS procedure ON procedure.oid=trigger.tgfoid WHERE namespace.nspname='public' AND trigger.tgname=ANY($1) AND NOT trigger.tgisinternal) AS native")
        .bind(names).fetch_one(f.persistence().pool()).await.unwrap();
    let records = catalog.as_array().unwrap();
    assert_eq!(records.len(), definitions.len());
    for definition in definitions {
        let record = records
            .iter()
            .find(|record| record["name"] == definition.name)
            .unwrap();
        assert_eq!(record["function"], definition.name);
        assert_eq!(record["relation"], definition.relation);
        assert_eq!(record["enabled"], "O");
        assert_eq!(record["timing"], definition.timing);
        assert_eq!(record["deferrable"], definition.deferred);
        assert_eq!(record["deferred"], definition.deferred);
        let start = definition
            .migration
            .find(&format!("FUNCTION {}(", definition.name))
            .unwrap();
        let body = definition.migration[start..]
            .split_once("$$")
            .unwrap()
            .1
            .split_once("$$")
            .unwrap()
            .0;
        assert_eq!(record["body"], body);
    }
    catalog
}

fn positive_fact<'a>(
    request: &ActionDispatchRequest,
    before: &Value,
    after: &'a Value,
    proof: ActionEvidenceId,
    result: &ReconciliationResult,
) -> &'a Value {
    let consumption = &before["workflow_action_evidence_consumptions"][0];
    assert_eq!(
        rows(before, "workflow_action_evidence_consumptions").len(),
        1
    );
    assert_eq!(consumption["evidence_id"], json!(proof.as_uuid()));
    let new = rows(before, "workflow_action_remote_entries")
        .iter()
        .find(|entry| entry["id"] == consumption["remote_entry_id"])
        .unwrap();
    let marker = new["dispatch_id"].as_str().unwrap().parse().unwrap();
    scope(consumption, request, marker);
    scope(new, request, marker);
    assert!(!new["reservation_xid"].is_null());
    assert_eq!(new["reservation_xid"], consumption["reservation_xid"]);
    let fact = append(before, after, "workflow_action_evidence", 1)[0];
    scope(fact, request, marker);
    assert_eq!(fact["id"], json!(result.evidence.unwrap().as_uuid()));
    assert_eq!(fact["disposition"], "applied");
    assert_eq!(fact["applied_request"], "remote_entry");
    assert_eq!(fact["applied_remote_entry_id"], new["id"]);
    assert_eq!(fact["grant_eligible"], false);
    let coverage = append(before, after, "workflow_action_evidence_coverage", 2);
    for entry in rows(before, "workflow_action_remote_entries") {
        let matches: Vec<_> = coverage
            .iter()
            .filter(|coverage| coverage["remote_entry_id"] == entry["id"])
            .collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["evidence_id"], fact["id"]);
        scope(matches[0], request, marker);
    }
    let receipt = append(before, after, "workflow_action_receipts", 1)[0];
    scope(receipt, request, marker);
    assert_eq!(receipt["reconciliation_evidence_id"], fact["id"]);
    assert!(receipt["remote_entry_id"].is_null());
    assert_eq!(receipt["result"], good());
    assert!(rows(after, "workflow_action_evidence_conflicts").is_empty());
    fact
}

pub(super) fn positive_links(
    request: &ActionDispatchRequest,
    granted: &Value,
    before: &Value,
    after: &Value,
    proof: ActionEvidenceId,
    result: &ReconciliationResult,
) {
    let fact = positive_fact(request, before, after, proof, result);
    let consumption = &before["workflow_action_evidence_consumptions"][0];
    assert_eq!(rows(granted, "workflow_action_evidence_coverage").len(), 1);
    assert!(
        rows(granted, "workflow_action_evidence_coverage")
            .iter()
            .all(|coverage| coverage["remote_entry_id"] != consumption["remote_entry_id"])
    );
    assert_eq!(
        granted["workflow_action_evidence"],
        before["workflow_action_evidence"]
    );
    assert_eq!(
        granted["workflow_action_evidence_coverage"],
        before["workflow_action_evidence_coverage"]
    );
    let command = command_audit(request, before, after, fact, result);
    let episode = append(before, after, "workflow_action_claim_episodes", 1)[0];
    assert_eq!(episode["command_key"], command["command_key"]);
    assert_eq!(episode["job_id"], json!(request.fence.scope.job.0));
    assert_eq!(episode["retired_attempt"], json!(request.fence.attempt.0));
    let scheduled = append(before, after, "workflow_action_schedule_witnesses", 1)[0];
    assert_eq!(scheduled["company_id"], command["company_id"]);
    assert_eq!(scheduled["run_id"], command["run_id"]);
    assert_eq!(scheduled["execution_id"], command["execution_id"]);
    assert_eq!(scheduled["job_id"], episode["job_id"]);
    assert_eq!(scheduled["retired_attempt"], episode["retired_attempt"]);
    let witness = append(before, after, "workflow_action_state_witnesses", 1)[0];
    assert_eq!(witness["initial_state"], "waiting");
    assert_eq!(witness["initial_waiting_reason"], "reconciliation");
    positive_owner_delta(before, after, result);
}

fn positive_owner_delta(before: &Value, after: &Value, result: &ReconciliationResult) {
    assert_eq!(after["workflow_runs"][0]["state"], "running");
    assert!(after["workflow_runs"][0]["waiting_reason"].is_null());
    assert_eq!(
        after["workflow_runs"][0]["revision"],
        json!(result.revision.0)
    );
    assert!(result.revision.0 > before["workflow_runs"][0]["revision"].as_u64().unwrap());
    assert_eq!(after["background_tasks"][0]["status"], "pending");
    let mut normalized = after.clone();
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_receipts",
        "workflow_action_evidence_commands",
        "workflow_run_events",
        "workflow_action_claim_episodes",
        "workflow_action_state_witnesses",
        "workflow_action_schedule_witnesses",
    ] {
        normalized[table] = before[table].clone();
    }
    restore_owner(
        &mut normalized,
        before,
        "workflow_runs",
        &["state", "waiting_reason", "revision"],
    );
    restore_owner(
        &mut normalized,
        before,
        "background_tasks",
        &["status", "run_at", "updated_at"],
    );
    assert_eq!(
        normalized.as_object().unwrap().keys().collect::<Vec<_>>(),
        before.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    for (table, old) in before.as_object().unwrap() {
        assert_eq!(&normalized[table], old, "exact preserved {table}");
    }
}

pub(super) fn rows<'a>(state: &'a Value, table: &str) -> &'a [Value] {
    state[table].as_array().unwrap()
}
pub(super) fn append<'a>(
    before: &Value,
    after: &'a Value,
    table: &str,
    count: usize,
) -> Vec<&'a Value> {
    let old = rows(before, table);
    let current = rows(after, table);
    assert_eq!(current.len(), old.len() + count, "{table}");
    assert!(
        old.iter().all(|row| current.contains(row)),
        "retained {table}"
    );
    current.iter().filter(|row| !old.contains(row)).collect()
}
fn scope(row: &Value, request: &ActionDispatchRequest, marker: Uuid) {
    subject_scope(row, request);
    assert_eq!(row["dispatch_id"], json!(marker));
}
pub(super) fn subject_scope(row: &Value, request: &ActionDispatchRequest) {
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
fn restore_owner(normalized: &mut Value, before: &Value, table: &str, fields: &[&str]) {
    assert_eq!(rows(normalized, table).len(), 1);
    assert_eq!(rows(before, table).len(), 1);
    for field in fields {
        normalized[table][0][*field] = before[table][0][*field].clone();
    }
    assert_eq!(
        normalized[table], before[table],
        "only exact {table} fields change"
    );
}

fn applied_fact<'a>(
    h: &AttributionOwner,
    attribution: Attribution,
    before: &Value,
    after: &'a Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
) -> &'a Value {
    let evidence = append(before, after, "workflow_action_evidence", 1)[0];
    scope(evidence, &h.request, command.marker.as_uuid());
    assert_eq!(evidence["id"], json!(result.evidence.unwrap().as_uuid()));
    assert_eq!(evidence["disposition"], "applied");
    assert_eq!(evidence["applied_request"], attribution.stored());
    let entry = match attribution {
        Attribution::CoveredEntry => before["workflow_action_remote_entries"][0]["id"].clone(),
        _ => Value::Null,
    };
    assert_eq!(evidence["applied_remote_entry_id"], entry);
    assert_eq!(evidence["grant_eligible"], false);
    let coverage = append(
        before,
        after,
        "workflow_action_evidence_coverage",
        rows(before, "workflow_action_remote_entries").len(),
    );
    for covered in coverage {
        scope(covered, &h.request, command.marker.as_uuid());
        assert_eq!(covered["evidence_id"], evidence["id"]);
        assert_eq!(
            covered["remote_entry_id"],
            before["workflow_action_remote_entries"][0]["id"]
        );
    }
    evidence
}

fn receipt_conflict(
    h: &AttributionOwner,
    attribution: Attribution,
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    evidence: &Value,
) {
    let receipt = append(before, after, "workflow_action_receipts", 1)[0];
    scope(receipt, &h.request, command.marker.as_uuid());
    assert_eq!(receipt["reconciliation_evidence_id"], evidence["id"]);
    assert!(receipt["remote_entry_id"].is_null());
    assert_eq!(receipt["result"], good());
    let conflict = append(before, after, "workflow_action_evidence_conflicts", 1)[0];
    scope(conflict, &h.request, command.marker.as_uuid());
    assert_eq!(conflict["evidence_id"], json!(h.proof.as_uuid()));
    assert_eq!(conflict["contradictory_evidence_id"], evidence["id"]);
    assert_eq!(conflict["reason"], attribution.reason());
    assert!(conflict["remote_entry_id"].is_null() && conflict["actual_observation_id"].is_null());
}

pub(super) fn committed_conflict(
    h: &AttributionOwner,
    attribution: Attribution,
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
) {
    let evidence = applied_fact(h, attribution, before, after, command, result);
    receipt_conflict(h, attribution, before, after, command, evidence);
    let saved = command_audit(&h.request, before, after, evidence, result);
    assert_eq!(saved["command_key"], json!(command.command_key.as_str()));
    assert_eq!(after["workflow_runs"][0]["state"], "waiting");
    assert_eq!(
        after["workflow_runs"][0]["waiting_reason"],
        "reconciliation"
    );
    assert_eq!(
        after["workflow_runs"][0]["revision"],
        json!(result.revision.0)
    );
    assert!(result.revision.0 > before["workflow_runs"][0]["revision"].as_u64().unwrap());
    assert_eq!(after["background_tasks"][0]["status"], "failed");
    let witnesses = append(before, after, "workflow_action_state_witnesses", 1);
    assert_eq!(witnesses[0]["initial_state"], "running");
    assert!(witnesses[0]["initial_waiting_reason"].is_null());
    let mut normalized = after.clone();
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_receipts",
        "workflow_action_evidence_conflicts",
        "workflow_action_evidence_commands",
        "workflow_run_events",
        "workflow_action_state_witnesses",
    ] {
        normalized[table] = before[table].clone();
    }
    restore_owner(
        &mut normalized,
        before,
        "workflow_runs",
        &["state", "waiting_reason", "revision"],
    );
    restore_owner(
        &mut normalized,
        before,
        "background_tasks",
        &["status", "updated_at"],
    );
    assert_eq!(
        normalized, *before,
        "every unrelated row and immutable history preserved"
    );
}

fn command_audit<'a>(
    request: &ActionDispatchRequest,
    before: &Value,
    after: &'a Value,
    fact: &Value,
    result: &ReconciliationResult,
) -> &'a Value {
    let saved = append(before, after, "workflow_action_evidence_commands", 1)[0];
    let marker = fact["dispatch_id"].as_str().unwrap().parse().unwrap();
    scope(saved, request, marker);
    assert!(saved["id"].is_string() && fact["command_id"].is_string());
    assert_eq!(saved["id"], fact["command_id"]);
    assert_eq!(saved["evidence_id"], fact["id"]);
    for field in ["actor_id", "command_key", "request_digest"] {
        assert!(fact[field].is_string(), "non-null {field}");
        assert_eq!(saved[field], fact[field]);
    }
    assert_eq!(saved["actor_id"], before["workflow_runs"][0]["actor_id"]);
    assert_eq!(saved["outcome"], json!(result.outcome));
    assert_eq!(saved["result_revision"], json!(result.revision.0));
    match result.outcome {
        ReconciliationOutcome::Scheduled { .. } => {
            assert_eq!(saved["scheduled_job_id"], json!(request.fence.scope.job.0))
        }
        ReconciliationOutcome::EvidenceConflict => assert!(saved["scheduled_job_id"].is_null()),
        _ => panic!("exact attribution outcome"),
    }
    let event = append(before, after, "workflow_run_events", 1)[0];
    for field in ["company_id", "run_id", "execution_id", "actor_id"] {
        assert!(event[field].is_string(), "non-null audit {field}");
        assert_eq!(event[field], saved[field]);
    }
    assert_eq!(event["event_kind"], "action_reconciled");
    let sequence = event["sequence"].as_i64().unwrap();
    assert_eq!(sequence, saved["audit_sequence"].as_i64().unwrap());
    let previous = rows(before, "workflow_run_events")
        .iter()
        .filter(|row| row["company_id"] == saved["company_id"] && row["run_id"] == saved["run_id"])
        .map(|row| row["sequence"].as_i64().unwrap())
        .max()
        .unwrap();
    assert_eq!(sequence, previous + 1);
    saved
}
