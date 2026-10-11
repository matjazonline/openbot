//! Exact native owners, single-field candidates and authentic parent rows.
use super::*;

const COMMAND: &str = "workflow_action_evidence_commands";
const SCHEDULED: &str = "CHECK (((scheduled_job_id IS NULL) OR ((previous_state = 'waiting'::text) AND (previous_waiting_reason = 'reconciliation'::text) AND ((outcome ->> 'kind'::text) = 'scheduled'::text))))";

pub(super) fn owner(catalog: &[Value], target: Target) -> &Value {
    let (reference, columns, references) = match target {
        Target::Dispatch => (
            "workflow_action_dispatches",
            json!([
                "company_id",
                "run_id",
                "execution_id",
                "invocation_id",
                "argument_digest",
                "dispatch_id"
            ]),
            json!([
                "company_id",
                "run_id",
                "execution_id",
                "invocation_id",
                "argument_digest",
                "id"
            ]),
        ),
        Target::Audit => (
            "workflow_run_events",
            json!(["company_id", "run_id", "audit_sequence"]),
            json!(["company_id", "run_id", "sequence"]),
        ),
        Target::Job => (
            "background_tasks",
            json!(["company_id", "execution_id", "scheduled_job_id"]),
            json!(["company_id", "workflow_execution_id", "id"]),
        ),
        _ => ("", Value::Null, Value::Null),
    };
    let matches: Vec<_> = catalog
        .iter()
        .filter(|row| {
            row["relation"] == COMMAND
                && if target.fk() {
                    row["type"] == "f"
                        && row["reference"] == reference
                        && row["columns"] == columns
                        && row["reference_columns"] == references
                } else {
                    row["type"] == "c" && row["definition"] == SCHEDULED
                }
        })
        .collect();
    assert_eq!(matches.len(), 1, "one exact catalog owner");
    assert_eq!(matches[0]["deferrable"], false);
    assert_eq!(matches[0]["initially_deferred"], false);
    matches[0]
}

pub(super) fn assert_native_order(native: &Value, catalog: &[Value]) {
    let binding =
        &native["trigger:workflow_action_evidence_commands.workflow_action_bind_schedule"];
    assert_eq!(binding["enabled"], "O");
    assert_eq!(binding["deferrable"], false);
    assert!(
        binding["definition"]
            .as_str()
            .unwrap()
            .contains("AFTER INSERT")
    );
    let scope =
        &native["trigger:workflow_action_evidence_commands.workflow_action_command_scope_guard"];
    assert_eq!(scope["enabled"], "O");
    assert_eq!(scope["deferrable"], true);
    assert_eq!(scope["deferred"], true);
    for target in [Target::Dispatch, Target::Audit, Target::Job] {
        let owner = owner(catalog, target);
        let triggers: Vec<_> = owner["triggers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["relation"] == COMMAND && row["function"] == "RI_FKey_check_ins")
            .collect();
        assert_eq!(triggers.len(), 1);
        let trigger = triggers[0];
        assert_eq!(trigger["enabled"], "O");
        assert_eq!(trigger["deferrable"], false);
        assert_eq!(trigger["initially_deferred"], false);
        assert!(trigger["name"].as_str().unwrap() < "workflow_action_bind_schedule");
        assert!(
            trigger["definition"]
                .as_str()
                .unwrap()
                .contains("AFTER INSERT")
        );
    }
}

pub(super) fn assert_installation(before: &Value, after: &Value) {
    let mut remaining = after.as_object().unwrap().clone();
    for (key, value) in before.as_object().unwrap() {
        assert_eq!(
            remaining.remove(key).as_ref(),
            Some(value),
            "native catalog {key}"
        );
    }
    assert!(remaining.keys().all(|key| {
        key.starts_with("constraint:fixture_native_command_")
            || key == "trigger:workflow_action_evidence_commands.fixture_native_command_before"
            || (key.starts_with("function:")
                && remaining[key]["name"]
                    .as_str()
                    .unwrap()
                    .starts_with("fixture_native_command_"))
    }));
    assert!(
        remaining.contains_key(
            "trigger:workflow_action_evidence_commands.fixture_native_command_before"
        )
    );
}

pub(super) fn transport(
    error: AppError,
    target: Target,
    cell: Cell,
    owner: &Value,
) -> (Envelope, Value) {
    let AppError::Database(message) = error else {
        panic!("native transport: {error:?}");
    };
    let frame = message
        .strip_prefix("error returned from database: FIXTURE_NATIVE_COMMAND_V1:")
        .unwrap();
    assert!(frame.len() <= 65536);
    let mut value: Value = serde_json::from_str(frame).unwrap();
    let linked = json!({"evidence":value.as_object_mut().unwrap().remove("evidence").unwrap(),
        "audit":value.as_object_mut().unwrap().remove("audit").unwrap(),"job":value.as_object_mut().unwrap().remove("job").unwrap()});
    let envelope: Envelope = serde_json::from_value(value).unwrap();
    assert_eq!(envelope.version, 1);
    assert_eq!(envelope.depth, 1);
    assert_eq!(envelope.mode, cell.mode());
    let native = &envelope.native;
    if cell.accepted() {
        assert_eq!(
            native.state, "PZ001",
            "{}; {}",
            native.message, native.context
        );
        assert_eq!(native.message, "native diagnostic probe accepted");
        assert_eq!(
            (&*native.schema, &*native.table, &*native.constraint),
            ("", "", "")
        );
        assert!(native.context.contains("at RAISE"));
    } else {
        assert_eq!(
            native.state,
            if target.fk() { "23503" } else { "23514" },
            "{}; {}",
            native.message,
            native.context
        );
        assert_eq!(native.schema, "public");
        assert_eq!(native.table, COMMAND);
        assert_eq!(native.constraint, owner["name"].as_str().unwrap());
        assert!(
            native
                .context
                .contains("INSERT INTO public.workflow_action_evidence_commands(")
        );
        assert!(native.context.contains("at SQL statement"));
        assert!(native.message.contains(&native.constraint));
    }
    (envelope, linked)
}

pub(super) fn original(
    source: &final_source::Final,
    foreign: &CompanyReceipt,
    before: &Value,
    envelope: &Envelope,
    linked: &Value,
    target: Target,
    cell: Cell,
) {
    let original = &envelope.original;
    let evidence = &linked["evidence"];
    super::super::super::super::super::super::commit_tests::assert_issued(
        source.verifier.issued.lock().unwrap().as_ref().unwrap(),
        evidence,
    );
    let audit = &linked["audit"];
    let job = &linked["job"];
    for (key, value) in source.expected["command"].as_object().unwrap() {
        assert_eq!(&original[key], value, "authentic command {key}");
    }
    for (key, value) in source.expected["evidence"].as_object().unwrap() {
        if key != "diagnostic" {
            assert_eq!(&evidence[key], value, "authentic evidence {key}");
        }
    }
    for key in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
        "dispatch_id",
        "actor_id",
        "request_digest",
        "command_key",
    ] {
        assert_eq!(original[key], evidence[key], "evidence identity {key}");
    }
    assert_eq!(original["id"], evidence["command_id"]);
    assert_eq!(original["evidence_id"], evidence["id"]);
    for key in ["company_id", "run_id", "execution_id", "actor_id"] {
        assert_eq!(original[key], audit[key]);
    }
    assert_eq!(original["audit_sequence"], audit["sequence"]);
    assert_eq!(audit["event_kind"], "action_reconciled");
    assert_eq!(original["scheduled_job_id"], job["id"]);
    assert_eq!(original["company_id"], job["company_id"]);
    assert_eq!(original["execution_id"], job["workflow_execution_id"]);
    assert_eq!(job["status"], "pending");
    assert_eq!(original["previous_state"], "waiting");
    assert_eq!(original["previous_waiting_reason"], "reconciliation");
    assert_eq!(
        original["outcome"],
        json!({"kind":"scheduled","receipt_only":false})
    );
    assert!(
        original["result_revision"].as_u64().unwrap()
            > original["expected_revision"].as_u64().unwrap()
    );
    let config = &before["fixture_native_command_config"][0];
    let mut expected = original.clone();
    if cell != Cell::Accepted {
        if target == Target::Kind {
            if cell == Cell::Missing {
                expected["outcome"].as_object_mut().unwrap().remove("kind");
            } else {
                expected["outcome"]["kind"] = config["replacement"].clone();
            }
        } else {
            expected[target.field()] = config["replacement"].clone();
        }
    }
    assert_eq!(envelope.candidate, expected, "only declared field changes");
    if cell == Cell::Foreign {
        foreign_target(foreign, before, original, target, &config["replacement"]);
    }
}

fn foreign_target(
    foreign: &CompanyReceipt,
    before: &Value,
    original: &Value,
    target: Target,
    replacement: &Value,
) {
    let company = json!(foreign.request.scope().company.as_uuid());
    assert_ne!(original["company_id"], company);
    let (relation, field) = match target {
        Target::Dispatch => ("workflow_action_dispatches", "id"),
        Target::Job => ("background_tasks", "id"),
        Target::Audit => ("workflow_run_events", "sequence"),
        _ => panic!("FK target"),
    };
    let rows: Vec<_> = before[relation]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["company_id"] == company && row[field] == *replacement)
        .collect();
    assert_eq!(rows.len(), 1);
    if target == Target::Audit {
        assert!(replacement.as_i64().unwrap() > original["audit_sequence"].as_i64().unwrap());
        assert!(
            before[relation]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["company_id"] != original["company_id"]
                    || row["run_id"] != original["run_id"]
                    || row["sequence"] != *replacement)
        );
        let commands: Vec<_> = before[COMMAND]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                row["company_id"] == company
                    && row["run_id"] == rows[0]["run_id"]
                    && row["audit_sequence"] == *replacement
            })
            .collect();
        assert_eq!(commands.len(), 1);
        assert_eq!(rows[0]["actor_id"], commands[0]["actor_id"]);
        assert_eq!(rows[0]["execution_id"], commands[0]["execution_id"]);
        assert_eq!(rows[0]["event_kind"], "action_reconciled");
        let evidence: Vec<_> = before["workflow_action_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["company_id"] == company && row["id"] == commands[0]["evidence_id"])
            .collect();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0]["command_id"], commands[0]["id"]);
        assert_eq!(evidence[0]["command_key"], commands[0]["command_key"]);
    }
}
