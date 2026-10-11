//! Native-owner, authentic-source and transported-diagnostic assertions.
use super::*;

pub(super) fn owner_view(state: &Value, foreign: &CompanyReceipt) -> Value {
    let mut view = state.clone();
    for rows in view.as_object_mut().unwrap().values_mut() {
        rows.as_array_mut().unwrap().retain(|row| {
            row["company_id"] != json!(foreign.request.scope().company.as_uuid())
                && row["invocation_id"] != json!(foreign.request.subject.invocation.as_uuid())
                && row["entry_id"] != json!(foreign.entry)
        });
    }
    view
}

pub(super) fn target_owner(catalog: &[Value], target: Target) -> &Value {
    let mut columns = vec![
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
    ];
    if !matches!(target, Target::Dispatch) {
        columns.push("dispatch_id");
    }
    let mut referenced = columns.clone();
    columns.push(target.field());
    referenced.push("id");
    let owners: Vec<_> = catalog
        .iter()
        .filter(|row| {
            row["type"] == "f"
                && row["relation"] == target.relation()
                && row["reference"] == target.reference()
                && row["columns"] == json!(columns)
                && row["reference_columns"] == json!(referenced)
        })
        .collect();
    assert_eq!(owners.len(), 1, "unique table-scoped exact composite FK");
    let owner = owners[0];
    assert_eq!(owner["deferrable"], false);
    assert_eq!(owner["initially_deferred"], false);
    let triggers: Vec<_> = owner["triggers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["relation"] == target.relation() && row["function"] == "RI_FKey_check_ins"
        })
        .collect();
    assert_eq!(triggers.len(), 1);
    assert_eq!(triggers[0]["enabled"], "O");
    assert_eq!(triggers[0]["deferrable"], false);
    assert_eq!(triggers[0]["initially_deferred"], false);
    owner
}

pub(super) fn assert_native_owners(catalog: &[Value], native: &Value) {
    for name in [
        "workflow_action_evidence_command_link",
        "workflow_action_evidence_commit_guard",
    ] {
        let owners: Vec<_> = catalog
            .iter()
            .filter(|row| row["relation"] == "workflow_action_evidence" && row["name"] == name)
            .collect();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0]["deferrable"], true);
        assert_eq!(owners[0]["initially_deferred"], true);
        let triggers = owners[0]["triggers"].as_array().unwrap();
        assert!(triggers.iter().all(|trigger| trigger["enabled"] == "O"));
        let inserts: Vec<_> = triggers
            .iter()
            .filter(|trigger| {
                trigger["relation"] == "workflow_action_evidence"
                    && (trigger["function"] == "RI_FKey_check_ins"
                        || trigger["name"] == "workflow_action_evidence_commit_guard")
            })
            .collect();
        assert_eq!(inserts.len(), 1);
        assert_eq!(inserts[0]["deferrable"], true);
        assert_eq!(inserts[0]["initially_deferred"], true);
    }
    let coverage: Vec<_> = native["triggers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["table"] == "workflow_action_evidence_coverage")
        .collect();
    assert_eq!(coverage.len(), 1);
    assert_eq!(
        coverage[0]["name"],
        "workflow_action_evidence_coverage_immutable"
    );
    assert_eq!(coverage[0]["enabled"], "O");
    assert!(
        !coverage[0]["definition"]
            .as_str()
            .unwrap()
            .contains("INSERT")
    );
}

pub(super) fn production(mut catalog: Value) -> Value {
    catalog = production_catalog(catalog);
    for section in ["triggers", "functions"] {
        catalog[section].as_array_mut().unwrap().retain(|row| {
            !matches!(
                row["name"].as_str().unwrap(),
                "fixture_native_fk_before" | "fixture_native_fk_coverage_probe"
            )
        });
    }
    catalog
}

pub(super) fn original_images(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    before: &Value,
    target: Target,
    original: &Value,
    evidence: &Value,
) {
    assert_original(source, command, evidence, before);
    if matches!(target, Target::Dispatch) {
        assert_eq!(original, evidence);
        return;
    }
    let entry = one(before, "workflow_action_remote_entries");
    assert_eq!(original.as_object().unwrap().len(), 8);
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
        "dispatch_id",
    ] {
        assert_eq!(original[field], evidence[field]);
        assert_eq!(original[field], entry[field]);
    }
    assert_eq!(original["evidence_id"], evidence["id"]);
    assert_eq!(original["remote_entry_id"], entry["id"]);
}

pub(super) fn transport(
    error: AppError,
    cell: Cell,
    target: Target,
    owner: &Value,
) -> (Envelope, Value) {
    let AppError::Database(message) = error else {
        panic!("native database transport required: {error:?}")
    };
    let prefix = if matches!(cell, Cell::Accepted) {
        "FIXTURE_NATIVE_FK_ACCEPTED_V1:"
    } else {
        "FIXTURE_NATIVE_FK_CAPTURED_V1:"
    };
    let frame = message
        .strip_prefix("error returned from database: ")
        .unwrap()
        .strip_prefix(prefix)
        .unwrap();
    assert!(frame.len() <= 65536);
    let mut value: Value = serde_json::from_str(frame).unwrap();
    let evidence = value.as_object_mut().unwrap().remove("evidence").unwrap();
    let envelope: Envelope = serde_json::from_value(value).unwrap();
    assert_eq!(envelope.version, 1);
    assert_eq!(envelope.depth, 1);
    assert_eq!(envelope.mode, cell.mode());
    let diagnostic = &envelope.native;
    if matches!(cell, Cell::Accepted) {
        assert_eq!(diagnostic.state, "PZ001");
        assert_eq!(diagnostic.message, "native diagnostic probe accepted");
        assert_eq!(diagnostic.schema, "");
        assert_eq!(diagnostic.table, "");
        assert_eq!(diagnostic.constraint, "");
        assert!(diagnostic.context.contains("at RAISE"));
    } else {
        assert_eq!(
            diagnostic.state, "23503",
            "{}; {}",
            diagnostic.message, diagnostic.context
        );
        assert_eq!(diagnostic.schema, "public");
        assert_eq!(diagnostic.table, target.relation());
        assert_eq!(diagnostic.constraint, owner["name"].as_str().unwrap());
        assert!(
            diagnostic
                .context
                .contains(&format!("INSERT INTO public.{}(", target.relation()))
        );
        assert!(diagnostic.context.contains("at SQL statement"));
        assert!(diagnostic.message.contains(&diagnostic.constraint));
    }
    (envelope, evidence)
}

pub(super) fn assert_sources(source: &UnknownSource, foreign: &CompanyReceipt, initial: &Value) {
    assert_source(source, &owner_view(initial, foreign));
    let marker = one(&owner_view(initial, foreign), "workflow_action_dispatches").clone();
    for (field, value) in [
        ("company_id", foreign.request.scope().company.as_uuid()),
        ("run_id", foreign.request.scope().run.as_uuid()),
        ("execution_id", foreign.request.scope().execution.as_uuid()),
        (
            "invocation_id",
            foreign.request.subject.invocation.as_uuid(),
        ),
        ("id", foreign.marker),
    ] {
        assert_ne!(marker[field], json!(value));
    }
    assert_ne!(
        one(
            &owner_view(initial, foreign),
            "workflow_action_remote_entries"
        )["id"],
        json!(foreign.entry)
    );
}
