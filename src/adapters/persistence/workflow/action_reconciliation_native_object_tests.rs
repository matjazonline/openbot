//! Native object-diagnostic attribution: the array type and the 16384/16385 jsonb-text bytes,
//! each substituted into a genuine prospective service INSERT.
use super::*;

const OBJECT_FIXTURE: &[&str] = &[
    "fixture_native_object_before",
    "fixture_native_object_after",
];
const DIAGNOSTIC_BYTES: i32 = 16384;
// A literal target, or an object padded in SQL to exactly $5 octets of its own jsonb text.
const CONFIGURE: &str = "INSERT INTO fixture_native_object_config(company_id,command_key,mode,diagnostic) \
    VALUES($1,$2,$3,COALESCE($4::jsonb,jsonb_build_object('pad'::text,repeat('x'::text, \
    $5::integer-octet_length(jsonb_build_object('pad'::text,''::text)::text))))) \
    RETURNING diagnostic,jsonb_typeof(diagnostic) AS kind,octet_length(diagnostic::text) AS octets";

#[derive(Clone, Copy, Debug)]
enum Outcome {
    Rejected,
    Accepted,
    Positive,
}

impl Outcome {
    fn mode(self) -> &'static str {
        match self {
            Self::Rejected => "rejected",
            Self::Accepted => "accepted",
            Self::Positive => "positive",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Target {
    Array,
    Object,
    Bytes(i32),
}

#[derive(Clone, Copy, Debug)]
struct Case {
    key: &'static str,
    outcome: Outcome,
    target: Target,
}

/// The diagnostic as the database stored and measured it in the config row.
#[derive(sqlx::FromRow)]
struct Configured {
    diagnostic: Value,
    kind: String,
    octets: i32,
}

async fn install(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: Case,
) -> Configured {
    let pool = source.fixture.persistence().pool();
    for fixture in [
        include_str!("action_reconciliation_native_diagnostic_fixture.sql"),
        include_str!("action_reconciliation_native_object_fixture.sql"),
    ] {
        sqlx::raw_sql(fixture).execute(pool).await.unwrap();
    }
    let (literal, octets) = match case.target {
        Target::Array => (Some(json!([{ "pad": "x" }])), None),
        Target::Object => (Some(json!({ "pad": "x" })), None),
        Target::Bytes(octets) => (None, Some(octets)),
    };
    let configured: Configured = sqlx::query_as(CONFIGURE)
        .bind(command.scope.company.as_uuid())
        .bind(command.command_key.as_str())
        .bind(case.outcome.mode())
        .bind(literal)
        .bind(octets)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_measured(&configured, case.target);
    configured
}

fn assert_measured(configured: &Configured, target: Target) {
    match target {
        Target::Array => {
            assert_eq!(configured.kind, "array");
            // Under the byte limit, so only the type conjunct can reject it.
            assert!(configured.octets < DIAGNOSTIC_BYTES);
        }
        Target::Object => {
            assert_eq!(configured.kind, "object");
            assert!(configured.octets < DIAGNOSTIC_BYTES);
        }
        Target::Bytes(octets) => {
            assert_eq!(configured.kind, "object");
            assert_eq!(configured.octets, octets);
        }
    }
}

fn object_catalog(catalog: Value) -> Value {
    // Exact fixture exclusions on top of the accepted ones; anything else remains visible.
    let mut catalog = production_catalog(catalog);
    for section in ["triggers", "functions"] {
        catalog[section]
            .as_array_mut()
            .unwrap()
            .retain(|row| !OBJECT_FIXTURE.contains(&row["name"].as_str().unwrap()));
    }
    catalog
}

fn transported(error: AppError, outcome: Outcome) -> Envelope {
    let AppError::Database(message) = error else {
        panic!("database error required: {error:?}")
    };
    let prefix = match outcome {
        Outcome::Rejected => "FIXTURE_NATIVE_OBJECT_CAPTURED_V1:",
        Outcome::Accepted => "FIXTURE_NATIVE_OBJECT_ACCEPTED_V1:",
        Outcome::Positive => panic!("positive must commit"),
    };
    let frame = message
        .strip_prefix("error returned from database: ")
        .unwrap()
        .strip_prefix(prefix)
        .expect("distinct probe frame");
    assert!(frame.len() <= 65536);
    let envelope: Envelope = serde_json::from_str(frame).unwrap();
    assert_eq!(envelope.version, 1);
    assert_eq!(envelope.depth, 1);
    assert_eq!(envelope.mode, outcome.mode());
    let native = &envelope.native;
    match outcome {
        Outcome::Rejected => assert_check_violation(native),
        Outcome::Accepted => assert_probe_accepted(native),
        Outcome::Positive => unreachable!(),
    }
    // The probe ran nested under the substituting trigger, not from some other caller.
    assert!(native.context.contains("fixture_native_object_before()"));
    eprintln!(
        "native_object mode={outcome:?} state={} schema={} table={} constraint={} message={}",
        native.state, native.schema, native.table, native.constraint, native.message
    );
    envelope
}

fn assert_check_violation(native: &NativeDiagnostic) {
    assert_eq!(native.state, "23514");
    assert_eq!(native.schema, "public");
    assert_eq!(native.table, "workflow_action_evidence");
    assert_eq!(
        native.constraint,
        "workflow_action_evidence_diagnostic_check"
    );
    assert_eq!(
        native.message,
        "new row for relation \"workflow_action_evidence\" violates check constraint \"workflow_action_evidence_diagnostic_check\""
    );
    assert!(
        native
            .context
            .contains("INSERT INTO public.workflow_action_evidence(")
    );
    assert!(
        native
            .context
            .contains("fixture_native_diagnostic_probe(workflow_action_evidence)")
    );
    assert!(native.context.contains("at SQL statement"));
}

fn assert_probe_accepted(native: &NativeDiagnostic) {
    assert_eq!(native.state, "PZ001");
    assert_eq!(native.message, "native diagnostic probe accepted");
    assert_eq!(native.schema, "");
    assert_eq!(native.table, "");
    assert_eq!(native.constraint, "");
    assert!(native.context.contains("at RAISE"));
}

/// The candidate is the original NEW with only the diagnostic replaced by the configured value.
fn assert_substituted(original: &Value, candidate: &Value, configured: &Configured) {
    assert_ne!(original["diagnostic"], configured.diagnostic);
    let mut expected = original.clone();
    expected["diagnostic"] = configured.diagnostic.clone();
    assert_eq!(
        *candidate, expected,
        "exact real NEW identity; only declared target changes"
    );
}

async fn negative(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: Case,
    configured: &Configured,
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
    assert_substituted(&envelope.original, &envelope.candidate, configured);
    wait_rollback(&mut observer, &envelope).await;
    assert_eq!(
        all_tables(&source.fixture).await,
        *before,
        "complete public and fixture rollback"
    );
}

async fn assert_stored(
    source: &UnknownSource,
    result: &ReconciliationResult,
    configured: &Configured,
) {
    let (kind, octets): (String, i32) = sqlx::query_as("SELECT jsonb_typeof(evidence.diagnostic),octet_length(evidence.diagnostic::text) FROM workflow_action_evidence AS evidence WHERE evidence.id=$1")
        .bind(result.evidence.unwrap().as_uuid())
        .fetch_one(source.fixture.persistence().pool()).await.unwrap();
    assert_eq!(kind, configured.kind);
    assert_eq!(octets, configured.octets);
}

fn assert_witness(command: &ReconcileActionCommand, result: &ReconciliationResult, after: &Value) {
    let witness = one(after, "fixture_native_diagnostic_witness");
    assert_eq!(
        witness["company_id"],
        json!(command.scope.company.as_uuid())
    );
    assert_eq!(witness["command_key"], command.command_key.as_str());
    assert_eq!(
        witness["evidence_id"],
        json!(result.evidence.unwrap().as_uuid())
    );
    assert_eq!(
        witness["command_id"],
        one(after, "workflow_action_evidence_commands")["id"]
    );
    assert_eq!(witness["selected_flush"], true);
    assert_eq!(witness["all_flush"], true);
}

async fn positive(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    configured: &Configured,
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
    let mut after = all_tables(&source.fixture).await;
    let images = one(&after, "fixture_native_diagnostic_images").clone();
    assert_eq!(images["depth"], 1);
    assert_original(source, command, &images["original"], before);
    assert_stored(source, &result, configured).await;
    assert_eq!(
        *one(&after, "workflow_action_evidence"),
        images["candidate"]
    );
    assert_substituted(&images["original"], &images["candidate"], configured);
    assert_witness(command, &result, &after);
    // Evidence is immutable, so the swap back is to this snapshot only: the remaining
    // assertions then prove the genuine issued envelope and an otherwise ordinary commit.
    after["workflow_action_evidence"][0]["diagnostic"] = images["original"]["diagnostic"].clone();
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        assert_eq!(before[relation], json!([]));
        after[relation] = before[relation].clone();
    }
    assert_evidence(source, command, &result, &after);
    assert_commit(command, &result, before, &after);
    eprintln!(
        "native_object positive kind={} octets={} original_new=true selected_flush=true all_flush=true ordinary_commit=true",
        configured.kind, configured.octets
    );
}

async fn exercise(case: Case) {
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
    let configured = install(&source, &command, case).await;
    assert_eq!(
        object_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    let before = all_tables(&source.fixture).await;
    assert_source(&source, &before);
    match case.outcome {
        Outcome::Positive => Box::pin(positive(&source, &command, &configured, &before)).await,
        _ => Box::pin(negative(&source, &command, case, &configured, &before)).await,
    }
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 0);
    assert_eq!(
        object_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    source.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_diagnostic_array_and_corrected_object() {
    for case in [
        Case {
            key: "array",
            outcome: Outcome::Rejected,
            target: Target::Array,
        },
        Case {
            key: "array-corrected",
            outcome: Outcome::Positive,
            target: Target::Object,
        },
    ] {
        Box::pin(exercise(case)).await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_native_diagnostic_jsonb_text_bytes() {
    let over = Target::Bytes(DIAGNOSTIC_BYTES + 1);
    let limit = Target::Bytes(DIAGNOSTIC_BYTES);
    for case in [
        Case {
            key: "bytes-16385",
            outcome: Outcome::Rejected,
            target: over,
        },
        Case {
            key: "bytes-16384-probe",
            outcome: Outcome::Accepted,
            target: limit,
        },
        Case {
            key: "bytes-16384",
            outcome: Outcome::Positive,
            target: limit,
        },
    ] {
        Box::pin(exercise(case)).await;
    }
}
#[path = "action_reconciliation_native_chronology_tests.rs"]
mod native_chronology_tests;
#[path = "action_reconciliation_native_coherence_tests.rs"]
mod native_coherence_tests;
#[path = "action_reconciliation_native_constraint_tests.rs"]
mod native_constraint_tests;
#[path = "action_reconciliation_native_grammar_tests.rs"]
mod native_grammar_tests;
#[path = "action_reconciliation_native_validity_tests.rs"]
mod native_validity_tests;
