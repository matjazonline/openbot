use super::*;
const SHAPE: &str = "workflow_action_applied_request_shape";
const SCOPE: &str = "workflow_action_applied_request_scope";
const GRAMMAR: &str = "workflow_action_evidence_applied_request_check";

pub(super) fn assert_owners(catalog: &[Value], native: &Value) {
    assert_before_owner(native);
    for name in [SHAPE, SCOPE, GRAMMAR] {
        let rows: Vec<_> = catalog
            .iter()
            .filter(|row| row["relation"] == "workflow_action_evidence" && row["name"] == name)
            .collect();
        assert_eq!(rows.len(), 1);
        let owner = rows[0];
        assert_eq!(owner["deferrable"], false);
        assert_eq!(owner["initially_deferred"], false);
        assert_eq!(owner["name_count"], 1);
        let definition = owner["definition"].as_str().unwrap();
        match name {
            SCOPE => {
                assert_eq!(owner["type"], "f");
                assert_eq!(owner["reference"], "workflow_action_remote_entries");
                assert_eq!(
                    owner["columns"],
                    json!([
                        "company_id",
                        "run_id",
                        "execution_id",
                        "invocation_id",
                        "argument_digest",
                        "dispatch_id",
                        "applied_remote_entry_id"
                    ])
                );
                assert_eq!(
                    owner["reference_columns"],
                    json!([
                        "company_id",
                        "run_id",
                        "execution_id",
                        "invocation_id",
                        "argument_digest",
                        "dispatch_id",
                        "id"
                    ])
                );
                assert!(
                    owner["triggers"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|row| row["enabled"] == "O" && row["deferrable"] == false)
                );
            }
            GRAMMAR => {
                assert_eq!(owner["type"], "c");
                assert_eq!(
                    definition,
                    // 04.11-D4 §N adds the message `delivery` attribution.
                    "CHECK ((applied_request = ANY (ARRAY['remote_entry'::text, 'marker_reservation'::text, 'unattributed'::text, 'delivery'::text])))"
                );
            }
            SHAPE => {
                assert_eq!(owner["type"], "c");
                assert_eq!(
                    definition,
                    // 04.11-D4 §N: every remote alternative requires no delivery subject; message
                    // applied evidence is `delivery` with one.
                    "CHECK ((((applied_request IS NULL) AND (applied_remote_entry_id IS NULL)) OR ((disposition = 'applied'::text) AND (delivery_id IS NULL) AND (((applied_request = 'remote_entry'::text) AND (applied_remote_entry_id IS NOT NULL)) OR ((applied_request = ANY (ARRAY['marker_reservation'::text, 'unattributed'::text])) AND (applied_remote_entry_id IS NULL)))) OR ((disposition = 'applied'::text) AND (applied_request = 'delivery'::text) AND (applied_remote_entry_id IS NULL) AND (delivery_id IS NOT NULL))))"
                );
            }
            _ => unreachable!(),
        }
    }
}

fn assert_before_owner(native: &Value) {
    assert!(
        SHAPE < GRAMMAR,
        "invalid discriminator is masked by actual shape owner"
    );
    let guard: Vec<_> = native["triggers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["name"] == "workflow_action_applied_request_guard")
        .collect();
    assert_eq!(guard.len(), 1);
    assert_eq!(guard[0]["enabled"], "O");
    assert_eq!(
        guard[0]["definition"],
        "CREATE TRIGGER workflow_action_applied_request_guard BEFORE INSERT ON public.workflow_action_evidence FOR EACH ROW EXECUTE FUNCTION workflow_action_applied_request_guard()"
    );
    let functions: Vec<_> = native["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["name"] == "workflow_action_applied_request_guard")
        .collect();
    assert_eq!(functions.len(), 1);
    let definition = functions[0]["definition"].as_str().unwrap();
    let actual = definition
        .split_once("$function$")
        .unwrap()
        .1
        .split_once("$function$")
        .unwrap()
        .0;
    let migration =
        include_str!("../../../../migrations/20260930181000_workflow_action_evidence_binding.sql");
    let body = migration
        .split_once("CREATE FUNCTION workflow_action_applied_request_guard()")
        .unwrap()
        .1
        .split_once("$$")
        .unwrap()
        .1
        .split_once("$$")
        .unwrap()
        .0;
    assert_eq!(actual, body);
}

pub(super) fn production(mut catalog: Value) -> Value {
    const FIXTURES: [&str; 3] = [
        "fixture_native_diagnostic_before",
        "fixture_native_diagnostic_after",
        "fixture_native_attribution_before",
    ];
    catalog["triggers"]
        .as_array_mut()
        .unwrap()
        .retain(|row| !FIXTURES.contains(&row["name"].as_str().unwrap()));
    catalog["functions"].as_array_mut().unwrap().retain(|row| {
        ![
            "fixture_native_diagnostic_before",
            "fixture_native_diagnostic_after",
            "fixture_native_diagnostic_probe",
            "fixture_native_attribution_before",
        ]
        .contains(&row["name"].as_str().unwrap())
    });
    catalog
}

pub(super) fn transport(error: AppError, cell: Cell) -> Envelope {
    let AppError::Database(message) = error else {
        panic!("database error required: {error:?}")
    };
    let prefix = if cell.accepted() {
        "FIXTURE_NATIVE_ATTRIBUTION_ACCEPTED_V1:"
    } else {
        "FIXTURE_NATIVE_ATTRIBUTION_CAPTURED_V1:"
    };
    let frame = message
        .strip_prefix("error returned from database: ")
        .unwrap()
        .strip_prefix(prefix)
        .expect("distinct native attribution frame");
    assert!(frame.len() <= 65536);
    let mut value: Value = serde_json::from_str(frame).unwrap();
    let predicate_values = value.as_object_mut().unwrap().remove("predicates").unwrap();
    let envelope: Envelope = serde_json::from_value(value).unwrap();
    assert_eq!(envelope.version, 1);
    assert_eq!(envelope.depth, 1);
    assert_eq!(envelope.mode, cell.mode());
    predicates(cell, &predicate_values);
    let native = &envelope.native;
    assert!(
        native
            .context
            .contains("fixture_native_diagnostic_probe(workflow_action_evidence)")
    );
    if cell.accepted() {
        assert_eq!(native.state, "PZ001");
        assert_eq!(native.message, "native diagnostic probe accepted");
        assert_eq!(
            (&*native.schema, &*native.table, &*native.constraint),
            ("", "", "")
        );
        assert!(native.context.contains("at RAISE"));
    } else {
        rejected(cell, native);
        assert!(
            native
                .context
                .contains("INSERT INTO public.workflow_action_evidence(")
        );
        assert!(native.context.contains("at SQL statement"));
    }
    eprintln!(
        "native_attribution cell={} state={} constraint={} message={}",
        cell.mode(),
        native.state,
        native.constraint,
        native.message
    );
    envelope
}
fn rejected(cell: Cell, native: &NativeDiagnostic) {
    if matches!(cell, Cell::MissingAttribution | Cell::NullWithEntry) {
        assert_eq!(native.state, "23514");
        assert_eq!(
            (&*native.schema, &*native.table, &*native.constraint),
            ("", "", "")
        );
        assert_eq!(
            native.message,
            "applied evidence requires request attribution"
        );
        assert!(
            native
                .context
                .contains("workflow_action_applied_request_guard() line 4 at RAISE")
        );
    } else {
        let fk = matches!(cell, Cell::Orphan | Cell::Foreign);
        let name = if fk { SCOPE } else { SHAPE };
        assert_eq!(native.state, if fk { "23503" } else { "23514" });
        assert_eq!(native.schema, "public");
        assert_eq!(native.table, "workflow_action_evidence");
        assert_eq!(native.constraint, name);
        assert_eq!(
            native.message,
            if fk {
                format!(
                    "insert or update on table \"workflow_action_evidence\" violates foreign key constraint \"{name}\""
                )
            } else {
                format!(
                    "new row for relation \"workflow_action_evidence\" violates check constraint \"{name}\""
                )
            }
        );
    }
}
fn predicates(cell: Cell, values: &Value) {
    let (grammar, shape, before) = match cell {
        Cell::Invalid => (json!(false), json!(false), false),
        Cell::MissingEntry
        | Cell::MarkerEntry
        | Cell::UnattributedEntry
        | Cell::UnknownAttributed
        | Cell::FinalAttributed => (json!(true), json!(false), false),
        Cell::MissingAttribution => (Value::Null, json!(true), true),
        Cell::NullWithEntry => (Value::Null, Value::Null, true),
        Cell::UnknownAccepted | Cell::FinalAccepted => (Value::Null, json!(true), false),
        _ => (json!(true), json!(true), false),
    };
    assert_eq!(
        *values,
        json!({"grammar": grammar,"shape": shape,"before_rejects":before})
    );
}

pub(super) fn original(
    source: &Source,
    command: &ReconcileActionCommand,
    cell: Cell,
    row: &Value,
    before: &Value,
) {
    let before = owner_view(before, command.scope.company.as_uuid());
    let before = &before;
    let marker = one(before, "workflow_action_dispatches");
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
    ] {
        assert_eq!(row[field], marker[field]);
    }
    assert_eq!(row["dispatch_id"], marker["id"]);
    assert_eq!(row["actor_id"], json!(command.actor.user_id()));
    assert_eq!(row["command_key"], command.command_key.as_str());
    assert_eq!(
        row["request_digest"],
        command.request_digest().unwrap().as_str()
    );
    for field in ["id", "command_id"] {
        assert!(
            !Uuid::parse_str(row[field].as_str().unwrap())
                .unwrap()
                .is_nil()
        );
    }
    assert_eq!(
        row["disposition"],
        if cell.unknown() { "unknown" } else { "applied" }
    );
    assert_eq!(row["grant_eligible"], false);
    if cell.unknown() {
        assert!(row["applied_request"].is_null());
        assert!(row["applied_remote_entry_id"].is_null());
    } else {
        assert_eq!(row["applied_request"], "remote_entry");
        assert_eq!(
            row["applied_remote_entry_id"],
            one(before, "workflow_action_remote_entries")["id"]
        );
    }
    original_attestation(source, cell, row, before);
}

fn original_attestation(source: &Source, cell: Cell, row: &Value, before: &Value) {
    let registration = source.verifier.registration();
    assert_eq!(row["registration"], registration.id().as_str());
    assert_eq!(row["verifier_version"], registration.version().as_str());
    assert_eq!(row["provider"], registration.provider().as_str());
    assert_eq!(
        row["operation_signature"],
        registration.operation().as_str()
    );
    let captured = source.verifier.observation.lock().unwrap();
    let captured = captured.as_ref().unwrap();
    assert_eq!(captured["applied"], !cell.unknown());
    assert_eq!(
        captured["entry"],
        one(before, "workflow_action_remote_entries")["id"]
    );
    assert_eq!(row["coverage_digest"], captured["coverage_digest"]);
    assert_eq!(row["authoritative_reference"], captured["reference"]);
    assert_eq!(
        instant(&row["observed_at"]),
        instant(&captured["observed_at"])
    );
    assert_eq!(
        instant(&row["valid_until"]),
        instant(&captured["valid_until"])
    );
    assert!(
        instant(&row["observed_at"])
            >= instant(&one(before, "workflow_action_remote_entries")["created_at"])
    );
    assert!(instant(&row["observed_at"]) <= instant(&row["verified_at"]));
    assert!(instant(&row["verified_at"]) <= instant(&row["created_at"]));
    assert!(instant(&row["valid_until"]) > instant(&row["verified_at"]));
    assert!(
        instant(&row["valid_until"]) <= instant(&row["verified_at"]) + chrono::Duration::hours(24)
    );
    assert_envelope(row, captured, registration);
}

fn owner_view(state: &Value, company: Uuid) -> Value {
    let mut state = state.clone();
    for rows in state.as_object_mut().unwrap().values_mut() {
        rows.as_array_mut()
            .unwrap()
            .retain(|row| row.get("company_id").is_none_or(|id| *id == json!(company)));
    }
    state
}
pub(super) fn candidate(
    cell: Cell,
    replacement: Uuid,
    envelope: &Envelope,
    before: &Value,
    foreign: &CompanyReceipt,
) {
    let original = &envelope.original;
    let mut expected = original.clone();
    match cell {
        Cell::Invalid => expected["applied_request"] = json!("invalid"),
        Cell::MissingEntry => expected["applied_remote_entry_id"] = Value::Null,
        Cell::MarkerEntry => expected["applied_request"] = json!("marker_reservation"),
        Cell::UnattributedEntry => expected["applied_request"] = json!("unattributed"),
        Cell::UnknownAttributed => {
            expected["applied_request"] = json!("remote_entry");
            let rows: Vec<_> = before["workflow_action_remote_entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["company_id"] == original["company_id"])
                .collect();
            assert_eq!(rows.len(), 1);
            expected["applied_remote_entry_id"] = rows[0]["id"].clone();
        }
        Cell::FinalAttributed => expected["disposition"] = json!("final_not_applied"),
        Cell::Orphan | Cell::Foreign => expected["applied_remote_entry_id"] = json!(replacement),
        Cell::MissingAttribution => {
            expected["applied_request"] = Value::Null;
            expected["applied_remote_entry_id"] = Value::Null;
        }
        Cell::NullWithEntry => expected["applied_request"] = Value::Null,
        Cell::RemoteAccepted | Cell::UnknownAccepted => {}
        Cell::MarkerAccepted | Cell::UnattributedAccepted => {
            expected["applied_request"] = json!(if matches!(cell, Cell::MarkerAccepted) {
                "marker_reservation"
            } else {
                "unattributed"
            });
            expected["applied_remote_entry_id"] = Value::Null;
        }
        Cell::FinalAccepted => {
            expected["disposition"] = json!("final_not_applied");
            expected["applied_request"] = Value::Null;
            expected["applied_remote_entry_id"] = Value::Null;
        }
    }
    assert_eq!(
        envelope.candidate, expected,
        "only exact declared candidate fields change"
    );
    foreign_entry(cell, replacement, original, before, foreign);
}

fn foreign_entry(
    cell: Cell,
    replacement: Uuid,
    original: &Value,
    before: &Value,
    foreign: &CompanyReceipt,
) {
    let entries: Vec<_> = before["workflow_action_remote_entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["id"] == json!(foreign.entry))
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["company_id"],
        json!(foreign.request.scope().company.as_uuid())
    );
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "dispatch_id",
    ] {
        assert_ne!(entries[0][field], original[field]);
    }
    if matches!(cell, Cell::Foreign) {
        assert_eq!(replacement, foreign.entry);
    }
}
