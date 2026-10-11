use super::*;
const GUARD: &str = "workflow_action_evidence_commit_guard";

pub(super) fn guard(catalog: &Value) {
    let trigger: Vec<_> = catalog["triggers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["name"] == GUARD)
        .collect();
    assert_eq!(trigger.len(), 1);
    assert_eq!(trigger[0]["enabled"], "O");
    assert_eq!(
        trigger[0]["definition"],
        "CREATE CONSTRAINT TRIGGER workflow_action_evidence_commit_guard AFTER INSERT ON public.workflow_action_evidence DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_action_evidence_commit_guard()"
    );
    let functions: Vec<_> = catalog["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["name"] == GUARD)
        .collect();
    assert_eq!(functions.len(), 1);
    let actual = functions[0]["definition"]
        .as_str()
        .unwrap()
        .split_once("$function$")
        .unwrap()
        .1
        .split_once("$function$")
        .unwrap()
        .0;
    let expected =
        include_str!("../../../../migrations/20260930181000_workflow_action_evidence_binding.sql")
            .split_once("CREATE OR REPLACE FUNCTION workflow_action_evidence_commit_guard()")
            .unwrap()
            .1
            .split_once("$$")
            .unwrap()
            .1
            .split_once("$$")
            .unwrap()
            .0;
    assert_eq!(actual, expected);
}
pub(super) fn coverage_catalog(catalog: Value) -> Value {
    let mut catalog = production_catalog(catalog);
    for section in ["triggers", "functions"] {
        catalog[section].as_array_mut().unwrap().retain(|row| {
            ![
                "fixture_native_coverage_before",
                "fixture_native_coverage_omit",
                "fixture_native_coverage_after",
            ]
            .contains(&row["name"].as_str().unwrap())
        });
    }
    catalog
}
pub(super) fn coverage_transport(error: AppError, case: Case) -> (Envelope, Value) {
    let AppError::Database(message) = error else {
        panic!("database error required: {error:?}")
    };
    let prefix = if case.mode == Mode::Rejected {
        "FIXTURE_NATIVE_COVERAGE_CAPTURED_V1:"
    } else {
        "FIXTURE_NATIVE_COVERAGE_ACCEPTED_V1:"
    };
    let frame = message
        .strip_prefix("error returned from database: ")
        .unwrap()
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("selected coverage frame required: {message}"));
    assert!(frame.len() <= 65536);
    let mut frame: Value = serde_json::from_str(frame).unwrap();
    let staged = frame.as_object_mut().unwrap().remove("staged").unwrap();
    let envelope: Envelope = serde_json::from_value(frame).unwrap();
    assert_eq!(envelope.version, 1);
    assert_eq!(envelope.depth, 1);
    assert_eq!(envelope.mode, case.mode.name());
    let native = &envelope.native;
    assert_eq!(
        (&*native.schema, &*native.table, &*native.constraint),
        ("", "", "")
    );
    if case.mode == Mode::Rejected {
        assert_eq!(native.state, "23514");
        assert_eq!(
            native.message,
            "invalid workflow action evidence provenance"
        );
        assert!(
            native
                .context
                .contains("workflow_action_evidence_commit_guard() line 27 at RAISE")
        );
        assert!(
            native
                .context
                .contains("SET CONSTRAINTS public.workflow_action_evidence_commit_guard IMMEDIATE")
        );
        assert!(native.context.contains("fixture_native_coverage_after()"));
    } else {
        assert_eq!(
            (&*native.state, &*native.message, &*native.context),
            ("", "", "")
        );
    }
    eprintln!(
        "native_coverage target={} mode={} state={} message={}",
        case.target.name(),
        case.mode.name(),
        native.state,
        native.message
    );
    (envelope, staged)
}
pub(super) fn candidate_coverage(original: &Value, candidate: &Value, case: Case, staged: &Value) {
    let mut expected = original.clone();
    if case.target == Target::Membership && case.mode != Mode::Positive {
        let marker = &staged["marker"]["created_at"];
        assert!(instant(marker) < instant(&staged["entry"]["created_at"]));
        for key in ["observed_at", "verified_at", "created_at"] {
            expected[key] = marker.clone();
        }
        assert!(instant(marker) < instant(&candidate["valid_until"]));
        assert!(
            instant(&candidate["valid_until"]) <= instant(marker) + chrono::Duration::hours(24)
        );
    } else if case.target == Target::Marker {
        expected["applied_request"] = json!(if case.mode == Mode::Rejected {
            "marker_reservation"
        } else {
            "unattributed"
        });
        expected["applied_remote_entry_id"] = Value::Null;
    }
    assert_eq!(
        *candidate, expected,
        "only exact approved candidate fields changed"
    );
}
fn scoped<'a>(state: &'a Value, relation: &str, company: &Value) -> Vec<&'a Value> {
    state[relation]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["company_id"] == *company)
        .collect()
}
pub(super) fn assert_staged(
    command: &ReconcileActionCommand,
    case: Case,
    evidence: &Value,
    before: &Value,
    staged: &Value,
) {
    assert_eq!(staged["evidence"], *evidence);
    let company = &evidence["company_id"];
    assert_eq!(
        scoped(before, "workflow_action_dispatches", company),
        vec![&staged["marker"]]
    );
    assert_eq!(
        scoped(before, "workflow_action_remote_entries", company),
        vec![&staged["entry"]]
    );
    let common = common(&staged["marker"]);
    let mut coverage = common.clone();
    coverage["evidence_id"] = evidence["id"].clone();
    coverage["remote_entry_id"] = staged["entry"]["id"].clone();
    assert_eq!(staged["attempts"], json!([coverage]));
    assert_eq!(
        staged["coverage"],
        if case.omitted() {
            json!([])
        } else {
            json!([coverage])
        }
    );
    assert_eq!(
        staged["predicates"],
        json!({"remote_marker":true,"observed_ge_marker":true,
        "verified_not_future":true,"verified_le_created":true,"registered_live":true,"command_link":true,
        "membership_rejects":case.target==Target::Membership && case.mode==Mode::Rejected,
        "marker_rejects":case.target==Target::Marker && case.mode==Mode::Rejected,
        "coverage_bounded":true,"entry_after_observed":false,
        "missing_rejects":case.target==Target::Missing && case.mode==Mode::Rejected,
        "audit_pair":true,"run_head":true,"coverage_count":if case.omitted(){0}else{1}})
    );
    assert_pair(command, case, staged, before);
}
fn common(marker: &Value) -> Value {
    let mut result = json!({});
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
    ] {
        result[field] = marker[field].clone();
    }
    result["dispatch_id"] = marker["id"].clone();
    result
}
fn assert_pair(command: &ReconcileActionCommand, case: Case, staged: &Value, before: &Value) {
    let evidence = &staged["evidence"];
    let pair = &staged["command"];
    let mut expected = common(&staged["marker"]);
    for field in ["command_key", "request_digest", "actor_id"] {
        expected[field] = evidence[field].clone();
    }
    expected["id"] = evidence["command_id"].clone();
    expected["evidence_id"] = evidence["id"].clone();
    expected["expected_revision"] = json!(command.expected_revision.0);
    expected["previous_state"] = json!("waiting");
    expected["previous_waiting_reason"] = json!("reconciliation");
    expected["outcome"] = if case.target == Target::Missing {
        json!({"kind":"unknown_recorded"})
    } else {
        json!({"kind":"scheduled","receipt_only":true})
    };
    for field in [
        "result_revision",
        "scheduled_job_id",
        "audit_sequence",
        "created_at",
    ] {
        expected[field] = pair[field].clone();
    }
    assert_eq!(*pair, expected);
    assert_eq!(pair["command_key"], command.command_key.as_str());
    assert_eq!(pair["result_revision"], staged["run"]["revision"]);
    assert!(pair["result_revision"].as_u64().unwrap() > command.expected_revision.0);
    assert!(instant(&pair["created_at"]) >= instant(&evidence["created_at"]));
    let audit = &staged["audit"];
    assert_eq!(
        *audit,
        json!({"company_id":evidence["company_id"],"run_id":evidence["run_id"],"sequence":pair["audit_sequence"],
        "execution_id":evidence["execution_id"],"actor_id":evidence["actor_id"],"event_kind":"action_reconciled",
        "created_at":audit["created_at"],"terminal_state":null,"terminal_execution_id":null})
    );
    let previous = before["workflow_run_events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["run_id"] == evidence["run_id"])
        .map(|r| r["sequence"].as_u64().unwrap())
        .max()
        .unwrap_or(0);
    assert_eq!(pair["audit_sequence"], previous + 1);
    if case.target == Target::Missing {
        assert!(staged["receipt"].is_null());
        assert!(pair["scheduled_job_id"].is_null());
    } else {
        let receipt = &staged["receipt"];
        let mut expected = common(&staged["marker"]);
        for (field, value) in [
            ("effect_kind", json!("remote")),
            ("remote_entry_id", Value::Null),
            ("reconciliation_evidence_id", evidence["id"].clone()),
            ("result", good()),
            ("created_at", receipt["created_at"].clone()),
        ] {
            expected[field] = value;
        }
        assert_eq!(*receipt, expected);
        assert!(instant(&receipt["created_at"]) >= instant(&evidence["created_at"]));
        assert!(!pair["scheduled_job_id"].is_null());
    }
}
pub(super) fn assert_committed(
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    case: Case,
    before: &Value,
    after: &Value,
    staged: &Value,
) {
    let company = &staged["evidence"]["company_id"];
    let pair = &staged["command"];
    assert_eq!(pair["result_revision"], result.revision.0);
    assert_eq!(
        pair["evidence_id"],
        json!(result.evidence.unwrap().as_uuid())
    );
    let witness = one(after, "fixture_native_diagnostic_witness");
    assert_eq!(
        *witness,
        json!({"company_id":company,"command_key":command.command_key.as_str(),"evidence_id":pair["evidence_id"],
        "command_id":pair["id"],"selected_flush":true,"all_flush":true})
    );
    let mut normalized = after.clone();
    for (table, rows) in [
        ("workflow_action_evidence", json!([staged["evidence"]])),
        ("workflow_action_evidence_commands", json!([pair])),
        (
            "workflow_action_evidence_coverage",
            staged["coverage"].clone(),
        ),
        ("workflow_run_events", json!([staged["audit"]])),
    ] {
        remove_additions(&mut normalized, before, table, &rows);
    }
    let old_run = scoped(before, "workflow_runs", company)[0];
    let new_run = scoped(after, "workflow_runs", company)[0];
    assert_eq!(*new_run, staged["run"]);
    let mut expected = old_run.clone();
    expected["revision"] = pair["result_revision"].clone();
    if case.target == Target::Missing {
        assert_eq!(result.revision.0, command.expected_revision.0 + 1);
    } else {
        expected["state"] = json!("running");
        expected["waiting_reason"] = Value::Null;
        assert_applied_delta(&mut normalized, before, after, staged);
        remove_additions(
            &mut normalized,
            before,
            "workflow_action_receipts",
            &json!([staged["receipt"]]),
        );
    }
    assert_eq!(*new_run, expected);
    replace_row(&mut normalized, "workflow_runs", new_run, old_run);
    for table in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
        "fixture_native_coverage_attempts",
        "fixture_native_coverage_staged",
    ] {
        assert_eq!(after[table].as_array().unwrap().len(), 1);
        let row = &after[table][0];
        assert_eq!(row["company_id"], *company);
        assert_eq!(row["command_key"], command.command_key.as_str());
        if table == "fixture_native_coverage_attempts" {
            assert_eq!(row["proposed"], staged["attempts"][0]);
        }
        normalized[table] = before[table].clone();
    }
    assert_eq!(
        normalized, *before,
        "only individually asserted A changes; B/provider/all other history exact"
    );
}
fn remove_additions(state: &mut Value, before: &Value, table: &str, added: &Value) {
    let rows = state[table].as_array_mut().unwrap();
    let old = before[table].as_array().unwrap();
    assert_eq!(rows.len(), old.len() + added.as_array().unwrap().len());
    for row in added.as_array().unwrap() {
        assert_eq!(rows.iter().filter(|r| *r == row).count(), 1);
        assert!(!old.contains(row));
        rows.retain(|r| r != row);
    }
    assert_eq!(rows, old);
}
fn replace_row(state: &mut Value, table: &str, new: &Value, old: &Value) {
    let rows = state[table].as_array_mut().unwrap();
    assert_eq!(
        new["id"], old["id"],
        "replacement preserves immutable identity"
    );
    assert_eq!(rows.iter().filter(|row| row["id"] == new["id"]).count(), 1);
    let row = rows.iter_mut().find(|row| row["id"] == new["id"]).unwrap();
    assert_eq!(*row, *new);
    // all_tables orders PostgreSQL JSONB text; retain the immutable-id row's position.
    *row = old.clone();
}
fn assert_applied_delta(normalized: &mut Value, before: &Value, after: &Value, staged: &Value) {
    let pair = &staged["command"];
    let old = before["background_tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == pair["scheduled_job_id"])
        .unwrap();
    let new = after["background_tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == pair["scheduled_job_id"])
        .unwrap();
    assert_eq!(old["status"], "failed");
    assert!(old["retry_count"].as_u64().unwrap() > 0);
    let mut expected = old.clone();
    expected["status"] = json!("pending");
    expected["run_at"] = new["run_at"].clone();
    expected["updated_at"] = new["updated_at"].clone();
    assert_eq!(*new, expected);
    assert!(instant(&new["run_at"]) >= instant(&staged["evidence"]["created_at"]));
    assert!(instant(&new["updated_at"]) >= instant(&new["run_at"]));
    replace_row(normalized, "background_tasks", new, old);
    let mut state = scoped(
        after,
        "workflow_action_state_witnesses",
        &pair["company_id"],
    );
    let mut schedule = scoped(
        after,
        "workflow_action_schedule_witnesses",
        &pair["company_id"],
    );
    state.retain(|row| {
        !before["workflow_action_state_witnesses"]
            .as_array()
            .unwrap()
            .contains(row)
    });
    schedule.retain(|row| {
        !before["workflow_action_schedule_witnesses"]
            .as_array()
            .unwrap()
            .contains(row)
    });
    assert_eq!(state.len(), 1);
    assert_eq!(schedule.len(), 1);
    assert_eq!(
        *state[0],
        json!({"company_id":pair["company_id"],"run_id":pair["run_id"],
        "transaction_id":state[0]["transaction_id"],"initial_state":"waiting","initial_waiting_reason":"reconciliation"})
    );
    assert_eq!(
        *schedule[0],
        json!({"company_id":pair["company_id"],"run_id":pair["run_id"],
        "execution_id":pair["execution_id"],"job_id":new["id"],"retired_attempt":new["retry_count"],"transaction_id":state[0]["transaction_id"]})
    );
    let mut episode = schedule[0].clone();
    episode.as_object_mut().unwrap().remove("transaction_id");
    episode["command_key"] = pair["command_key"].clone();
    for (table, row) in [
        ("workflow_action_state_witnesses", state[0]),
        ("workflow_action_schedule_witnesses", schedule[0]),
        ("workflow_action_claim_episodes", &episode),
    ] {
        remove_additions(normalized, before, table, &json!([row]));
    }
}
