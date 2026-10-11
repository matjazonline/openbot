//! Native covered-entry chronology: the deferred provenance guard rejects evidence observed at the
//! dispatch marker's time while a covered remote entry was created later. The marker case and the
//! entry-time control differ only in `observed_at`; the production function is the owner.
use super::*;

const CHRONOLOGY_FIXTURE: &[&str] = &[
    "fixture_native_chronology_before",
    "fixture_native_chronology_after",
];
const GUARD: &str = "workflow_action_evidence_commit_guard";
const GUARD_TRIGGER: &str = "CREATE CONSTRAINT TRIGGER workflow_action_evidence_commit_guard AFTER INSERT ON public.workflow_action_evidence DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_action_evidence_commit_guard()";
const GUARD_MESSAGE: &str = "invalid workflow action evidence provenance";
const CONFIGURE: &str = "INSERT INTO fixture_native_chronology_config(company_id,command_key,mode,anchor) \
    VALUES($1,$2,$3,$4) RETURNING mode,anchor";
// Measured on the stored row against the single covered entry, never from the frame.
const MEASURE: &str = "SELECT evidence.observed_at=entry.created_at FROM workflow_action_evidence AS evidence \
    JOIN workflow_action_remote_entries AS entry ON entry.company_id=evidence.company_id \
    AND entry.invocation_id=evidence.invocation_id WHERE evidence.id=$1";

#[derive(Clone, Copy, Debug)]
struct ChronologyCase {
    key: &'static str,
    outcome: Outcome,
}

impl ChronologyCase {
    fn anchor(self) -> &'static str {
        match self.outcome {
            Outcome::Rejected => "marker",
            _ => "entry",
        }
    }
}

fn guard_target(catalog: &Value) {
    let triggers: Vec<_> = catalog["triggers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["name"] == GUARD)
        .collect();
    assert_eq!(triggers.len(), 1);
    assert_eq!(triggers[0]["table"], "workflow_action_evidence");
    assert_eq!(triggers[0]["enabled"], "O");
    assert_eq!(triggers[0]["definition"], GUARD_TRIGGER);
    let functions: Vec<_> = catalog["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["name"] == GUARD)
        .collect();
    assert_eq!(functions.len(), 1);
    let definition = functions[0]["definition"].as_str().unwrap();
    for fragment in [
        "AND entry.created_at<=NEW.created_at AND (entry.created_at>NEW.observed_at",
        "OR NEW.observed_at<marker.created_at",
        "RAISE EXCEPTION 'invalid workflow action evidence provenance' USING ERRCODE='23514'",
    ] {
        assert!(definition.contains(fragment), "{fragment}");
    }
}

fn chronology_catalog(catalog: Value) -> Value {
    let mut catalog = production_catalog(catalog);
    for section in ["triggers", "functions"] {
        catalog[section]
            .as_array_mut()
            .unwrap()
            .retain(|row| !CHRONOLOGY_FIXTURE.contains(&row["name"].as_str().unwrap()));
    }
    catalog
}

async fn install(source: &UnknownSource, command: &ReconcileActionCommand, case: ChronologyCase) {
    let pool = source.fixture.persistence().pool();
    for fixture in [
        include_str!("action_reconciliation_native_diagnostic_fixture.sql"),
        include_str!("action_reconciliation_native_chronology_fixture.sql"),
    ] {
        sqlx::raw_sql(fixture).execute(pool).await.unwrap();
    }
    let (mode, anchor): (String, String) = sqlx::query_as(CONFIGURE)
        .bind(command.scope.company.as_uuid())
        .bind(command.command_key.as_str())
        .bind(case.outcome.mode())
        .bind(case.anchor())
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(mode, case.outcome.mode());
    assert_eq!(anchor, case.anchor());
}

fn transported(error: AppError, outcome: Outcome) -> Envelope {
    let AppError::Database(message) = error else {
        panic!("database error required: {error:?}")
    };
    let prefix = match outcome {
        Outcome::Rejected => "FIXTURE_NATIVE_CHRONOLOGY_CAPTURED_V1:",
        Outcome::Accepted => "FIXTURE_NATIVE_CHRONOLOGY_ACCEPTED_V1:",
        Outcome::Positive => panic!("positive must commit"),
    };
    let frame = message
        .strip_prefix("error returned from database: ")
        .unwrap()
        .strip_prefix(prefix)
        .expect("distinct chronology frame");
    assert!(frame.len() <= 65536);
    let mut frame: Value = serde_json::from_str(frame).unwrap();
    let predicates = frame.as_object_mut().unwrap().remove("predicates").unwrap();
    // Supporting witness only: the guard's other conditions held; the target matches the anchor.
    assert_eq!(
        predicates,
        json!({"remote_marker": true, "observed_ge_marker": true, "verified_not_future": true,
            "verified_le_created": true, "registered_live": true, "command_link": true,
            "applied_request": true, "coverage_bounded": true,
            "entry_after_observed": matches!(outcome, Outcome::Rejected),
            "all_entries_covered": true, "audit_pair": true, "run_head": true,
            "single_coverage": true})
    );
    let envelope: Envelope = serde_json::from_value(frame).unwrap();
    assert_eq!(envelope.version, 1);
    assert_eq!(envelope.depth, 1);
    assert_eq!(envelope.mode, outcome.mode());
    let native = &envelope.native;
    if matches!(outcome, Outcome::Rejected) {
        assert_eq!(native.state, "23514");
        assert_eq!(native.message, GUARD_MESSAGE);
        for frame in [
            "workflow_action_evidence_commit_guard()",
            "SET CONSTRAINTS public.workflow_action_evidence_commit_guard IMMEDIATE",
            "fixture_native_chronology_after()",
        ] {
            assert!(native.context.contains(frame), "{frame}");
        }
    } else {
        assert_eq!(native.state, "");
        assert_eq!(native.message, "");
        assert_eq!(native.context, "");
    }
    for field in [&native.constraint, &native.schema, &native.table] {
        assert_eq!(field, "");
    }
    eprintln!(
        "native_chronology mode={outcome:?} state={} schema={} table={} constraint={} message={}",
        native.state, native.schema, native.table, native.constraint, native.message
    );
    envelope
}

/// The candidate is the original NEW with only `observed_at` moved to the anchor's creation time.
fn assert_anchored(original: &Value, candidate: &Value, before: &Value, case: ChronologyCase) {
    let mut expected = original.clone();
    expected["observed_at"] = candidate["observed_at"].clone();
    assert_eq!(*candidate, expected, "only observed_at changes");
    let marker = instant(&one(before, "workflow_action_dispatches")["created_at"]);
    let entry = instant(&one(before, "workflow_action_remote_entries")["created_at"]);
    assert!(marker < entry);
    let observed = instant(&original["observed_at"]);
    assert!(observed > entry);
    let anchored = if matches!(case.outcome, Outcome::Rejected) {
        marker
    } else {
        entry
    };
    assert_eq!(instant(&candidate["observed_at"]), anchored);
    assert_ne!(observed, anchored);
}

async fn negative(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: ChronologyCase,
    before: &Value,
) {
    let mut observer = source.fixture.persistence().pool().acquire().await.unwrap();
    let error = Box::pin(run_command(
        &source.fixture,
        command,
        source.verifier.clone(),
    ))
    .await
    .unwrap_err();
    let envelope = transported(error, case.outcome);
    assert_original(source, command, &envelope.original, before);
    assert_anchored(&envelope.original, &envelope.candidate, before, case);
    wait_rollback(&mut observer, &envelope).await;
    assert_eq!(
        all_tables(&source.fixture).await,
        *before,
        "complete public and fixture rollback"
    );
}

async fn positive(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: ChronologyCase,
    before: &Value,
) {
    let result = Box::pin(run_command(
        &source.fixture,
        command,
        source.verifier.clone(),
    ))
    .await
    .unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::UnknownRecorded);
    assert!(!result.replayed);
    let at_entry: bool = sqlx::query_scalar(MEASURE)
        .bind(result.evidence.unwrap().as_uuid())
        .fetch_one(source.fixture.persistence().pool())
        .await
        .unwrap();
    assert!(at_entry);
    let mut after = all_tables(&source.fixture).await;
    let images = one(&after, "fixture_native_diagnostic_images").clone();
    assert_eq!(images["depth"], 1);
    assert_original(source, command, &images["original"], before);
    assert_eq!(
        *one(&after, "workflow_action_evidence"),
        images["candidate"]
    );
    assert_anchored(&images["original"], &images["candidate"], before, case);
    assert_witness(command, &result, &after);
    // Evidence is immutable, so the swap back is to this snapshot only, after the DB measurement.
    after["workflow_action_evidence"][0]["observed_at"] = images["original"]["observed_at"].clone();
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        after[relation] = before[relation].clone();
    }
    assert_evidence(source, command, &result, &after);
    assert_commit(command, &result, before, &after);
    eprintln!(
        "native_chronology positive observed_at=entry.created_at selected_flush=true all_flush=true ordinary_commit=true"
    );
}

async fn exercise(case: ChronologyCase) {
    // Box the real fixture/service seams for stock 2 MiB libtest stacks.
    let source = Box::pin(unknown_source()).await;
    let command = proof_command(
        &source.fixture,
        &source.request,
        &source.verifier.ledger,
        case.key,
    )
    .await;
    let catalog = native_catalog(&source.fixture).await;
    assert_target(&catalog);
    guard_target(&catalog);
    install(&source, &command, case).await;
    let installed = chronology_catalog(native_catalog(&source.fixture).await);
    assert_eq!(installed, catalog);
    let before = all_tables(&source.fixture).await;
    assert_source(&source, &before);
    let config = one(&before, "fixture_native_chronology_config");
    assert_eq!(config["command_key"], command.command_key.as_str());
    assert_eq!(config["mode"], case.outcome.mode());
    assert_eq!(config["anchor"], case.anchor());
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        assert_eq!(before[relation], json!([]));
    }
    match case.outcome {
        Outcome::Positive => Box::pin(positive(&source, &command, case, &before)).await,
        _ => Box::pin(negative(&source, &command, case, &before)).await,
    }
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 0);
    let after = native_catalog(&source.fixture).await;
    guard_target(&after);
    assert_eq!(chronology_catalog(after), catalog);
    source.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_chronology_marker_rejected() {
    for (key, outcome) in [
        ("chronology-marker", Outcome::Rejected),
        ("chronology-entry-probe", Outcome::Accepted),
    ] {
        Box::pin(exercise(ChronologyCase { key, outcome })).await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_native_chronology_entry_committed() {
    let case = ChronologyCase {
        key: "chronology-entry-commit",
        outcome: Outcome::Positive,
    };
    Box::pin(exercise(case)).await;
}
