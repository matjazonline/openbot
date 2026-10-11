//! Native registered-validity attribution: the compound registration CHECK's strict lower bound
//! and inclusive `verified_at + interval '24 hours'` upper bound, each applied to a genuine
//! prospective service INSERT. The committed +24h row proves native structural validity only.
use super::*;

const VALIDITY_FIXTURE: &[&str] = &[
    "fixture_native_validity_before",
    "fixture_native_validity_after",
];
// Read from a freshly migrated PostgreSQL 18.6 database; only the generated name is derived.
const VALIDITY_CHECK: &str = "CHECK ((((registration IS NULL) AND (verifier_version IS NULL) AND (provider IS NULL) AND (authoritative_reference IS NULL) AND (operation_signature IS NULL) AND (disposition = 'unknown'::text) AND (valid_until = verified_at) AND (NOT grant_eligible)) OR ((registration IS NOT NULL) AND (verifier_version IS NOT NULL) AND (provider IS NOT NULL) AND (authoritative_reference IS NOT NULL) AND (operation_signature IS NOT NULL) AND (valid_until > verified_at) AND (valid_until <= (verified_at + '24:00:00'::interval)))))";
const CONFIGURE: &str = "INSERT INTO fixture_native_validity_config(company_id,command_key,mode,bound) \
    VALUES($1,$2,$3,$4) RETURNING mode,bound";
// Measured on the stored row against the stored original image, in the CHECK's own form.
const MEASURE: &str = "SELECT evidence.valid_until=evidence.verified_at+interval '24 hours' AS boundary, \
    evidence.valid_until>evidence.verified_at \
    AND evidence.valid_until<=evidence.verified_at+interval '24 hours' AS bounded, \
    evidence.valid_until-evidence.verified_at=(images.original->>'valid_until')::timestamptz \
    -(images.original->>'verified_at')::timestamptz AS retained, \
    num_nonnulls(evidence.registration,evidence.verifier_version,evidence.provider, \
    evidence.authoritative_reference,evidence.operation_signature)=5 AS registered, \
    (extract(epoch FROM evidence.valid_until-evidence.verified_at)*1000000)::bigint AS lifetime_us \
    FROM workflow_action_evidence AS evidence JOIN fixture_native_diagnostic_images AS images \
    ON images.company_id=evidence.company_id AND images.command_key=evidence.command_key \
    WHERE evidence.id=$1";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Bound {
    Lower,
    Upper,
    Boundary,
    Retain,
}

impl Bound {
    fn name(self) -> &'static str {
        match self {
            Self::Lower => "lower",
            Self::Upper => "upper",
            Self::Boundary => "boundary",
            Self::Retain => "retain",
        }
    }

    fn lifetime(self, genuine: chrono::Duration) -> chrono::Duration {
        let day = chrono::Duration::hours(24);
        match self {
            Self::Lower => chrono::Duration::zero(),
            Self::Upper => day + chrono::Duration::microseconds(1),
            Self::Boundary => day,
            Self::Retain => genuine,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ValidityCase {
    key: &'static str,
    outcome: Outcome,
    bound: Bound,
}

#[derive(sqlx::FromRow)]
struct Measured {
    boundary: bool,
    bounded: bool,
    retained: bool,
    registered: bool,
    lifetime_us: i64,
}

fn validity_target(catalog: &Value) -> String {
    for conjunct in [
        "(registration IS NOT NULL)",
        "(verifier_version IS NOT NULL)",
        "(provider IS NOT NULL)",
        "(authoritative_reference IS NOT NULL)",
        "(operation_signature IS NOT NULL)",
        "(valid_until > verified_at)",
        "(valid_until <= (verified_at + '24:00:00'::interval))",
    ] {
        assert!(VALIDITY_CHECK.contains(conjunct), "{conjunct}");
    }
    let checks: Vec<_> = catalog["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["definition"] == VALIDITY_CHECK)
        .collect();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0]["table"], "workflow_action_evidence");
    assert_eq!(checks[0]["validated"], true);
    checks[0]["name"].as_str().unwrap().to_owned()
}

fn validity_catalog(catalog: Value) -> Value {
    // Exact fixture exclusions on top of the accepted ones; anything else remains visible.
    let mut catalog = production_catalog(catalog);
    for section in ["triggers", "functions"] {
        catalog[section]
            .as_array_mut()
            .unwrap()
            .retain(|row| !VALIDITY_FIXTURE.contains(&row["name"].as_str().unwrap()));
    }
    catalog
}

async fn install(source: &UnknownSource, command: &ReconcileActionCommand, case: ValidityCase) {
    let pool = source.fixture.persistence().pool();
    for fixture in [
        include_str!("action_reconciliation_native_diagnostic_fixture.sql"),
        include_str!("action_reconciliation_native_validity_fixture.sql"),
    ] {
        sqlx::raw_sql(fixture).execute(pool).await.unwrap();
    }
    let (mode, bound): (String, String) = sqlx::query_as(CONFIGURE)
        .bind(command.scope.company.as_uuid())
        .bind(command.command_key.as_str())
        .bind(case.outcome.mode())
        .bind(case.bound.name())
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(mode, case.outcome.mode());
    assert_eq!(bound, case.bound.name());
}

fn transported(error: AppError, outcome: Outcome, constraint: &str) -> Envelope {
    let AppError::Database(message) = error else {
        panic!("database error required: {error:?}")
    };
    let prefix = match outcome {
        Outcome::Rejected => "FIXTURE_NATIVE_VALIDITY_CAPTURED_V1:",
        Outcome::Accepted => "FIXTURE_NATIVE_VALIDITY_ACCEPTED_V1:",
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
        Outcome::Rejected => assert_validity_violation(native, constraint),
        Outcome::Accepted => assert_probe_accepted(native),
        Outcome::Positive => unreachable!(),
    }
    // The probe ran nested under the substituting trigger, not from some other caller.
    assert!(native.context.contains("fixture_native_validity_before()"));
    eprintln!(
        "native_validity mode={outcome:?} state={} schema={} table={} constraint={} message={}",
        native.state, native.schema, native.table, native.constraint, native.message
    );
    envelope
}

fn assert_validity_violation(native: &NativeDiagnostic, constraint: &str) {
    assert_eq!(native.state, "23514");
    assert_eq!(native.schema, "public");
    assert_eq!(native.table, "workflow_action_evidence");
    assert_eq!(native.constraint, constraint);
    assert_eq!(
        native.message,
        format!(
            "new row for relation \"workflow_action_evidence\" violates check constraint \"{constraint}\""
        )
    );
    for frame in [
        "INSERT INTO public.workflow_action_evidence(",
        "fixture_native_diagnostic_probe(workflow_action_evidence)",
        "at SQL statement",
    ] {
        assert!(native.context.contains(frame), "{frame}");
    }
}

/// The candidate is the original NEW with only `valid_until` moved to the declared bound.
fn assert_bounded(original: &Value, candidate: &Value, bound: Bound) -> chrono::Duration {
    let mut expected = original.clone();
    expected["valid_until"] = candidate["valid_until"].clone();
    assert_eq!(
        *candidate, expected,
        "exact real NEW identity; only valid_until changes"
    );
    let lifetime = |row: &Value| instant(&row["valid_until"]) - instant(&row["verified_at"]);
    let genuine = lifetime(original);
    assert!(genuine > chrono::Duration::zero());
    assert!(genuine <= chrono::Duration::hours(24));
    assert_eq!(lifetime(candidate), bound.lifetime(genuine));
    if bound == Bound::Retain {
        assert_eq!(candidate, original);
    } else {
        assert_ne!(
            instant(&original["valid_until"]),
            instant(&candidate["valid_until"])
        );
    }
    lifetime(candidate)
}

async fn negative(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: ValidityCase,
    constraint: &str,
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
    let envelope = transported(error, case.outcome, constraint);
    assert_original(source, command, &envelope.original, before);
    assert_bounded(&envelope.original, &envelope.candidate, case.bound);
    wait_rollback(&mut observer, &envelope).await;
    assert_eq!(
        all_tables(&source.fixture).await,
        *before,
        "complete public and fixture rollback"
    );
}

async fn assert_stored(source: &UnknownSource, result: &ReconciliationResult, bound: Bound) -> i64 {
    let measured: Measured = sqlx::query_as(MEASURE)
        .bind(result.evidence.unwrap().as_uuid())
        .fetch_one(source.fixture.persistence().pool())
        .await
        .unwrap();
    assert!(measured.registered);
    assert!(measured.bounded);
    assert_eq!(measured.boundary, bound == Bound::Boundary);
    assert_eq!(measured.retained, bound == Bound::Retain);
    measured.lifetime_us
}

async fn positive(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    bound: Bound,
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
    let measured = assert_stored(source, &result, bound).await;
    assert_eq!(
        *one(&after, "workflow_action_evidence"),
        images["candidate"]
    );
    let lifetime = assert_bounded(&images["original"], &images["candidate"], bound);
    assert_eq!(Some(measured), lifetime.num_microseconds());
    assert_witness(command, &result, &after);
    // Evidence is immutable, so the swap back is to this snapshot only: the remaining
    // assertions then prove the genuine issued envelope and an otherwise ordinary commit.
    after["workflow_action_evidence"][0]["valid_until"] = images["original"]["valid_until"].clone();
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        after[relation] = before[relation].clone();
    }
    assert_evidence(source, command, &result, &after);
    assert_commit(command, &result, before, &after);
    eprintln!(
        "native_validity positive bound={} lifetime_us={measured} registered=true selected_flush=true all_flush=true ordinary_commit=true structural_validity_only=true",
        bound.name()
    );
}

fn assert_configured(before: &Value, command: &ReconcileActionCommand, case: ValidityCase) {
    let config = one(before, "fixture_native_validity_config");
    assert_eq!(config["command_key"], command.command_key.as_str());
    assert_eq!(config["mode"], case.outcome.mode());
    assert_eq!(config["bound"], case.bound.name());
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        assert_eq!(before[relation], json!([]));
    }
}

async fn exercise(case: ValidityCase) {
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
    let constraint = validity_target(&catalog);
    install(&source, &command, case).await;
    assert_eq!(
        validity_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    let before = all_tables(&source.fixture).await;
    assert_source(&source, &before);
    assert_configured(&before, &command, case);
    match case.outcome {
        Outcome::Positive => Box::pin(positive(&source, &command, case.bound, &before)).await,
        _ => Box::pin(negative(&source, &command, case, &constraint, &before)).await,
    }
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 0);
    assert_eq!(
        validity_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    source.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_validity_registered_bounds_rejected() {
    for (key, outcome, bound) in [
        ("validity-lower", Outcome::Rejected, Bound::Lower),
        ("validity-upper", Outcome::Rejected, Bound::Upper),
        (
            "validity-boundary-probe",
            Outcome::Accepted,
            Bound::Boundary,
        ),
    ] {
        Box::pin(exercise(ValidityCase {
            key,
            outcome,
            bound,
        }))
        .await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_native_validity_registered_bounds_committed() {
    for (key, bound) in [
        ("validity-upper-commit", Bound::Boundary),
        ("validity-lower-commit", Bound::Retain),
    ] {
        let outcome = Outcome::Positive;
        Box::pin(exercise(ValidityCase {
            key,
            outcome,
            bound,
        }))
        .await;
    }
}
