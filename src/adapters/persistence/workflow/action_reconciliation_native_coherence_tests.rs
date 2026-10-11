//! Native cross-column attribution on the stored evidence row: the time-order CHECK, the
//! provenance all-or-none CHECK in both directions, and the grant_eligible scope CHECK, each
//! substituted into a genuine prospective service INSERT; positives commit and are DB-measured.
use super::*;
use crate::application::workflow::actions::ClaimedDisposition;

const COHERENCE_FIXTURE: &[&str] = &[
    "fixture_native_coherence_before",
    "fixture_native_coherence_after",
];
const NOTE: &str = "operator native coherence note";
const CONFIGURE: &str = "INSERT INTO fixture_native_coherence_config(company_id,command_key,mode,shape,source) \
    VALUES($1,$2,$3,$4,$5) RETURNING to_jsonb(fixture_native_coherence_config)";
// Each pinned CHECK expression evaluated on the stored row, plus the boundary facts it rests on.
const MEASURE: &str = "SELECT (evidence.observed_at<=evidence.verified_at \
    AND evidence.verified_at<=evidence.created_at) AS time_order, \
    ((evidence.registration IS NULL AND evidence.verifier_version IS NULL AND evidence.provider IS NULL \
    AND evidence.authoritative_reference IS NULL AND evidence.operation_signature IS NULL \
    AND evidence.disposition='unknown' AND evidence.valid_until=evidence.verified_at \
    AND NOT evidence.grant_eligible) OR (evidence.registration IS NOT NULL \
    AND evidence.verifier_version IS NOT NULL AND evidence.provider IS NOT NULL \
    AND evidence.authoritative_reference IS NOT NULL AND evidence.operation_signature IS NOT NULL \
    AND evidence.valid_until>evidence.verified_at \
    AND evidence.valid_until<=evidence.verified_at+interval '24 hours')) AS presence, \
    (NOT evidence.grant_eligible OR evidence.disposition='final_not_applied') AS grant_scope, \
    evidence.observed_at=evidence.verified_at AS observed_is_verified, \
    evidence.verified_at=evidence.created_at AS verified_is_created, \
    evidence.valid_until=evidence.verified_at AS valid_is_verified, \
    num_nulls(evidence.registration,evidence.verifier_version,evidence.provider, \
    evidence.authoritative_reference,evidence.operation_signature) AS absent, \
    evidence.grant_eligible FROM workflow_action_evidence AS evidence WHERE evidence.id=$1";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Check {
    TimeOrder,
    Presence,
    GrantScope,
}

impl Check {
    fn name(self) -> &'static str {
        match self {
            Self::TimeOrder => "workflow_action_evidence_check",
            Self::Presence => "workflow_action_evidence_check1",
            Self::GrantScope => "workflow_action_evidence_check2",
        }
    }

    /// Read from a freshly migrated PostgreSQL 18.6 database; the name is derived, then pinned.
    fn definition(self) -> &'static str {
        match self {
            Self::TimeOrder => {
                "CHECK (((observed_at <= verified_at) AND (verified_at <= created_at)))"
            }
            Self::Presence => {
                "CHECK ((((registration IS NULL) AND (verifier_version IS NULL) AND (provider IS NULL) AND (authoritative_reference IS NULL) AND (operation_signature IS NULL) AND (disposition = 'unknown'::text) AND (valid_until = verified_at) AND (NOT grant_eligible)) OR ((registration IS NOT NULL) AND (verifier_version IS NOT NULL) AND (provider IS NOT NULL) AND (authoritative_reference IS NOT NULL) AND (operation_signature IS NOT NULL) AND (valid_until > verified_at) AND (valid_until <= (verified_at + '24:00:00'::interval)))))"
            }
            Self::GrantScope => {
                "CHECK (((NOT grant_eligible) OR (disposition = 'final_not_applied'::text)))"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Registered,
    Unregistered,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Unregistered => "unregistered",
        }
    }
}

/// Each rejected shape violates only its CHECK: the registered window (valid_until about ten
/// minutes after verified_at) still holds, column CHECKs admit NULL, and deferred guards never run.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    ObservedAfterVerified,
    VerifiedAfterCreated,
    ObservedEqualsVerified,
    VerifiedEqualsCreated,
    Absent(&'static str),
    GrantEligible,
    RegistrationPresent,
    Genuine,
}

impl Shape {
    fn name(self) -> String {
        match self {
            Self::ObservedAfterVerified => "observed_after_verified".into(),
            Self::VerifiedAfterCreated => "verified_after_created".into(),
            Self::ObservedEqualsVerified => "observed_equals_verified".into(),
            Self::VerifiedEqualsCreated => "verified_equals_created".into(),
            Self::Absent(column) => format!("null_{column}"),
            Self::GrantEligible => "grant_eligible".into(),
            Self::RegistrationPresent => "registration_present".into(),
            Self::Genuine => "genuine".into(),
        }
    }

    fn check(self) -> Option<Check> {
        match self {
            Self::ObservedAfterVerified | Self::VerifiedAfterCreated => Some(Check::TimeOrder),
            Self::Absent(_) | Self::RegistrationPresent => Some(Check::Presence),
            Self::GrantEligible => Some(Check::GrantScope),
            Self::ObservedEqualsVerified | Self::VerifiedEqualsCreated | Self::Genuine => None,
        }
    }

    fn outcome(self) -> Outcome {
        match self.check() {
            Some(_) => Outcome::Rejected,
            None => Outcome::Positive,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CoherenceCase {
    key: &'static str,
    shape: Shape,
    source: Source,
}

#[derive(sqlx::FromRow)]
struct Measured {
    time_order: bool,
    presence: bool,
    grant_scope: bool,
    observed_is_verified: bool,
    verified_is_created: bool,
    valid_is_verified: bool,
    absent: i32,
    grant_eligible: bool,
}

fn coherence_target(catalog: &Value, check: Check) -> String {
    let checks: Vec<_> = catalog["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["table"] == "workflow_action_evidence" && row["definition"] == check.definition()
        })
        .collect();
    assert_eq!(checks.len(), 1, "{check:?}");
    assert_eq!(checks[0]["validated"], true);
    assert_eq!(checks[0]["name"], check.name());
    check.name().to_owned()
}

fn coherence_catalog(catalog: Value) -> Value {
    // Exact fixture exclusions on top of the accepted ones; anything else remains visible.
    let mut catalog = production_catalog(catalog);
    for section in ["triggers", "functions"] {
        catalog[section]
            .as_array_mut()
            .unwrap()
            .retain(|row| !COHERENCE_FIXTURE.contains(&row["name"].as_str().unwrap()));
    }
    catalog
}

async fn install(source: &UnknownSource, command: &ReconcileActionCommand, case: CoherenceCase) {
    let pool = source.fixture.persistence().pool();
    for fixture in [
        include_str!("action_reconciliation_native_diagnostic_fixture.sql"),
        include_str!("action_reconciliation_native_coherence_fixture.sql"),
    ] {
        sqlx::raw_sql(fixture).execute(pool).await.unwrap();
    }
    let configured: Value = sqlx::query_scalar(CONFIGURE)
        .bind(command.scope.company.as_uuid())
        .bind(command.command_key.as_str())
        .bind(case.shape.outcome().mode())
        .bind(case.shape.name())
        .bind(case.source.name())
        .fetch_one(pool)
        .await
        .unwrap();
    assert_configured_row(&configured, command, case);
}

fn transported(error: AppError, constraint: &str) -> Envelope {
    let AppError::Database(message) = error else {
        panic!("database error required: {error:?}")
    };
    let frame = message
        .strip_prefix("error returned from database: ")
        .unwrap()
        .strip_prefix("FIXTURE_NATIVE_COHERENCE_CAPTURED_V1:")
        .expect("distinct probe frame");
    assert!(frame.len() <= 65536);
    let envelope: Envelope = serde_json::from_str(frame).unwrap();
    assert_eq!(envelope.version, 1);
    assert_eq!(envelope.depth, 1);
    assert_eq!(envelope.mode, Outcome::Rejected.mode());
    let native = &envelope.native;
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
        // The probe ran nested under the substituting trigger, not from some other caller.
        "fixture_native_coherence_before()",
    ] {
        assert!(native.context.contains(frame), "{frame}");
    }
    eprintln!(
        "native_coherence state={} schema={} table={} constraint={} message={}",
        native.state, native.schema, native.table, native.constraint, native.message
    );
    envelope
}

fn without(row: &Value, column: &str) -> Value {
    let mut row = row.clone();
    assert!(row.as_object_mut().unwrap().remove(column).is_some());
    row
}

/// The candidate is the original NEW with only the shape's column replaced as declared.
fn assert_substituted(original: &Value, candidate: &Value, shape: Shape) {
    let micro = chrono::Duration::microseconds(1);
    let (column, instant_value, plain) = match shape {
        Shape::ObservedAfterVerified => (
            "observed_at",
            Some(instant(&original["verified_at"]) + micro),
            None,
        ),
        Shape::VerifiedAfterCreated => (
            "verified_at",
            Some(instant(&original["created_at"]) + micro),
            None,
        ),
        Shape::ObservedEqualsVerified => {
            ("observed_at", Some(instant(&original["verified_at"])), None)
        }
        Shape::VerifiedEqualsCreated => {
            ("verified_at", Some(instant(&original["created_at"])), None)
        }
        Shape::Absent(column) => (column, None, Some(Value::Null)),
        Shape::GrantEligible => ("grant_eligible", None, Some(json!(true))),
        Shape::RegistrationPresent => ("registration", None, Some(json!("reg"))),
        Shape::Genuine => {
            assert_eq!(candidate, original, "genuine NEW unchanged");
            return;
        }
    };
    assert_eq!(
        without(candidate, column),
        without(original, column),
        "exact real NEW identity; only declared target changes"
    );
    if let Some(expected) = instant_value {
        assert_eq!(instant(&candidate[column]), expected);
        // A genuine row may already sit on an equality boundary: verified_at is issued as
        // max(elapsed clock, observed_at) (service.rs:173); only the rejected shapes must move.
        if shape.check().is_some() {
            assert_ne!(instant(&original[column]), expected);
        }
    }
    if let Some(expected) = plain {
        assert_eq!(candidate[column], expected);
        assert_ne!(original[column], expected);
    }
}

/// The genuine UnknownNote row: no provenance, a zero-length window, the operator note.
fn assert_unregistered(command: &ReconcileActionCommand, row: &Value, before: &Value) {
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
    assert_eq!(row["disposition"], "unknown");
    assert_eq!(row["grant_eligible"], false);
    for field in [
        "registration",
        "verifier_version",
        "provider",
        "authoritative_reference",
        "operation_signature",
        "applied_request",
        "applied_remote_entry_id",
    ] {
        assert!(row[field].is_null(), "{field}");
    }
    let verified = instant(&row["verified_at"]);
    assert_eq!(instant(&row["observed_at"]), verified);
    assert_eq!(instant(&row["valid_until"]), verified);
    assert!(verified <= instant(&row["created_at"]));
    assert_eq!(
        row["diagnostic"],
        json!({"version": 1, "note": NOTE, "claimed": ClaimedDisposition::Unknown})
    );
}

fn assert_source_row(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: CoherenceCase,
    row: &Value,
    before: &Value,
) {
    match case.source {
        Source::Registered => assert_original(source, command, row, before),
        Source::Unregistered => assert_unregistered(command, row, before),
    }
}

async fn negative(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: CoherenceCase,
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
    let envelope = transported(error, constraint);
    assert_source_row(source, command, case, &envelope.original, before);
    assert_substituted(&envelope.original, &envelope.candidate, case.shape);
    wait_rollback(&mut observer, &envelope).await;
    assert_eq!(
        all_tables(&source.fixture).await,
        *before,
        "complete public and fixture rollback"
    );
}

fn assert_measured(case: CoherenceCase, stored: &Measured) {
    assert!(stored.time_order && stored.presence && stored.grant_scope);
    assert!(!stored.grant_eligible);
    match (case.shape, case.source) {
        (Shape::ObservedEqualsVerified, Source::Registered) => {
            assert!(stored.observed_is_verified);
        }
        (Shape::VerifiedEqualsCreated, Source::Registered) => {
            assert!(stored.verified_is_created);
        }
        (Shape::Genuine, Source::Registered) => {
            assert_eq!(stored.absent, 0);
            assert!(!stored.valid_is_verified);
        }
        (Shape::Genuine, Source::Unregistered) => {
            assert_eq!(stored.absent, 5);
            assert!(stored.valid_is_verified && stored.observed_is_verified);
        }
        other => panic!("not a positive: {other:?}"),
    }
}

async fn positive(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: CoherenceCase,
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
    assert_eq!(images["command_key"], command.command_key.as_str());
    assert_source_row(source, command, case, &images["original"], before);
    assert_substituted(&images["original"], &images["candidate"], case.shape);
    assert_eq!(
        *one(&after, "workflow_action_evidence"),
        images["candidate"]
    );
    let stored: Measured = sqlx::query_as(MEASURE)
        .bind(result.evidence.unwrap().as_uuid())
        .fetch_one(source.fixture.persistence().pool())
        .await
        .unwrap();
    assert_measured(case, &stored);
    assert_witness(command, &result, &after);
    let moved = images["original"] != images["candidate"];
    // Evidence is immutable, so the swap back is to this snapshot only: the remaining
    // assertions then prove the genuine issued row and an otherwise ordinary commit.
    after["workflow_action_evidence"][0] = images["original"].clone();
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        after[relation] = before[relation].clone();
    }
    match case.source {
        Source::Registered => assert_evidence(source, command, &result, &after),
        Source::Unregistered => {
            let evidence = one(&after, "workflow_action_evidence");
            assert_unregistered(command, evidence, before);
            assert_eq!(
                one(&after, "workflow_action_evidence_coverage")["evidence_id"],
                evidence["id"]
            );
        }
    }
    assert_commit(command, &result, before, &after);
    eprintln!(
        "native_coherence positive shape={} source={} time_order={} presence={} grant_scope={} absent={} substituted={} selected_flush=true all_flush=true ordinary_commit=true",
        case.shape.name(),
        case.source.name(),
        stored.time_order,
        stored.presence,
        stored.grant_scope,
        stored.absent,
        moved
    );
}

fn assert_configured_row(config: &Value, command: &ReconcileActionCommand, case: CoherenceCase) {
    assert_eq!(config["company_id"], json!(command.scope.company.as_uuid()));
    assert_eq!(config["command_key"], command.command_key.as_str());
    assert_eq!(config["mode"], case.shape.outcome().mode());
    assert_eq!(config["shape"], case.shape.name());
    assert_eq!(config["source"], case.source.name());
}

fn assert_configured(before: &Value, command: &ReconcileActionCommand, case: CoherenceCase) {
    assert_configured_row(
        one(before, "fixture_native_coherence_config"),
        command,
        case,
    );
    // The reused scalar fixture stays inert only while its own config table is empty.
    for relation in [
        "fixture_native_diagnostic_config",
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        assert_eq!(before[relation], json!([]));
    }
}

async fn coherence_command(source: &UnknownSource, case: CoherenceCase) -> ReconcileActionCommand {
    let mut command = proof_command(
        &source.fixture,
        &source.request,
        &source.verifier.ledger,
        case.key,
    )
    .await;
    if case.source == Source::Unregistered {
        command.input = EvidenceInput::UnknownNote {
            note: NOTE.into(),
            claimed: ClaimedDisposition::Unknown,
        };
    }
    command
}

async fn exercise(case: CoherenceCase) {
    // Box the real fixture/service seams for stock 2 MiB libtest stacks.
    let source = Box::pin(unknown_source()).await;
    let command = coherence_command(&source, case).await;
    let catalog = native_catalog(&source.fixture).await;
    assert_target(&catalog);
    let constraint = case
        .shape
        .check()
        .map(|check| coherence_target(&catalog, check));
    install(&source, &command, case).await;
    assert_eq!(
        coherence_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    let before = all_tables(&source.fixture).await;
    assert_source(&source, &before);
    assert_configured(&before, &command, case);
    match constraint {
        None => Box::pin(positive(&source, &command, case, &before)).await,
        Some(constraint) => {
            Box::pin(negative(&source, &command, case, &constraint, &before)).await;
        }
    }
    let verified = match case.source {
        Source::Registered => 1,
        Source::Unregistered => 0,
    };
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), verified);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 0);
    let end = all_tables(&source.fixture).await;
    assert_eq!(end["fixture_native_diagnostic_config"], json!([]));
    assert_eq!(
        end["fixture_native_coherence_config"],
        before["fixture_native_coherence_config"]
    );
    assert_eq!(
        coherence_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    source.fixture.persistence().pool().close().await;
}

async fn matrix(source: Source, cases: &[(&'static str, Shape)]) {
    for &(key, shape) in cases {
        Box::pin(exercise(CoherenceCase { key, shape, source })).await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_native_coherence_time_order() {
    let cases = [
        ("time-observed-after-verified", Shape::ObservedAfterVerified),
        ("time-verified-after-created", Shape::VerifiedAfterCreated),
        (
            "time-observed-equals-verified",
            Shape::ObservedEqualsVerified,
        ),
        ("time-verified-equals-created", Shape::VerifiedEqualsCreated),
    ];
    Box::pin(matrix(Source::Registered, &cases)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_coherence_registered_presence() {
    let cases = [
        ("presence-registration", Shape::Absent("registration")),
        (
            "presence-verifier-version",
            Shape::Absent("verifier_version"),
        ),
        ("presence-provider", Shape::Absent("provider")),
        (
            "presence-reference",
            Shape::Absent("authoritative_reference"),
        ),
        ("presence-signature", Shape::Absent("operation_signature")),
        ("presence-registered-genuine", Shape::Genuine),
    ];
    Box::pin(matrix(Source::Registered, &cases)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_coherence_unregistered_presence() {
    let cases = [
        (
            "presence-unregistered-registration",
            Shape::RegistrationPresent,
        ),
        ("presence-unregistered-genuine", Shape::Genuine),
    ];
    Box::pin(matrix(Source::Unregistered, &cases)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_coherence_grant_scope() {
    let cases = [
        ("grant-eligible-unknown", Shape::GrantEligible),
        ("grant-genuine-false", Shape::Genuine),
    ];
    Box::pin(matrix(Source::Registered, &cases)).await;
}
