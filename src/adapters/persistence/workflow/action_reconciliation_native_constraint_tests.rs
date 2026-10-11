//! Native scalar-constraint attribution: the command_key length, the request, coverage and
//! operation-signature digest grammar, and the disposition set CHECKs, each substituted into a
//! genuine prospective service INSERT of a registered source; the genuine values are the positives.
use super::*;

const CONSTRAINT_FIXTURE: &[&str] = &[
    "fixture_native_constraint_before",
    "fixture_native_constraint_after",
];
const LIMIT: usize = 128;
const DIGEST: usize = 64;
const CONFIGURE: &str = "INSERT INTO fixture_native_constraint_config(company_id,command_key,mode,target,value) \
    VALUES($1,$2,$3,$4,$5) RETURNING value,octet_length(value) AS octets";
// Measured on the stored row, in the column the case targets, against that column's pinned CHECK
// expression; an unknown column yields NULL and fails to decode.
const MEASURE: &str = "SELECT stored.value,octet_length(stored.value) AS octets, \
    CASE $2 WHEN 'command_key' THEN octet_length(stored.value) BETWEEN 1 AND 128 \
    WHEN 'request_digest' THEN stored.value ~ '^[0-9a-f]{64}$' \
    WHEN 'coverage_digest' THEN stored.value ~ '^[0-9a-f]{64}$' \
    WHEN 'operation_signature' THEN stored.value ~ '^[0-9a-f]{64}$' \
    WHEN 'disposition' THEN stored.value IN ('applied','final_not_applied','unknown') END AS admitted \
    FROM (SELECT CASE $2 WHEN 'command_key' THEN evidence.command_key \
    WHEN 'request_digest' THEN evidence.request_digest WHEN 'coverage_digest' THEN evidence.coverage_digest \
    WHEN 'disposition' THEN evidence.disposition \
    WHEN 'operation_signature' THEN evidence.operation_signature END AS value \
    FROM workflow_action_evidence AS evidence WHERE evidence.id=$1) AS stored";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Column {
    CommandKey,
    RequestDigest,
    CoverageDigest,
    Disposition,
    OperationSignature,
}

impl Column {
    fn name(self) -> &'static str {
        match self {
            Self::CommandKey => "command_key",
            Self::RequestDigest => "request_digest",
            Self::CoverageDigest => "coverage_digest",
            Self::Disposition => "disposition",
            Self::OperationSignature => "operation_signature",
        }
    }

    /// Read from a freshly migrated PostgreSQL 18.6 database; the name is derived, then pinned.
    fn definition(self) -> &'static str {
        match self {
            Self::CommandKey => {
                "CHECK (((octet_length(command_key) >= 1) AND (octet_length(command_key) <= 128)))"
            }
            Self::RequestDigest => "CHECK ((request_digest ~ '^[0-9a-f]{64}$'::text))",
            Self::CoverageDigest => "CHECK ((coverage_digest ~ '^[0-9a-f]{64}$'::text))",
            Self::Disposition => {
                "CHECK ((disposition = ANY (ARRAY['applied'::text, 'final_not_applied'::text, 'unknown'::text])))"
            }
            Self::OperationSignature => "CHECK ((operation_signature ~ '^[0-9a-f]{64}$'::text))",
        }
    }
}

/// Each rejected shape violates only its column's CHECK: the source is registered and not
/// grant-eligible, applied_request is NULL, and every other column keeps the genuine value.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Empty,
    Over,
    Short,
    Long,
    Uppercase,
    NonHex,
    Outside,
    Capitalised,
    Genuine,
}

/// "K" then the IdempotencyKey/EvidenceRecordReference common alphabet, `len` bytes in all.
fn sized(len: usize) -> String {
    let alphabet = b"a9_.:-";
    let tail = (0..len - 1).map(|index| char::from(alphabet[index % alphabet.len()]));
    std::iter::once('K').chain(tail).collect()
}

#[derive(Clone, Copy, Debug)]
struct ConstraintCase {
    key: &'static str,
    column: Column,
    shape: Shape,
}

impl ConstraintCase {
    fn outcome(self) -> Outcome {
        match self.shape {
            Shape::Genuine => Outcome::Positive,
            _ => Outcome::Rejected,
        }
    }

    /// The command_key positive is the genuine 128-byte boundary key; the rest name their case.
    fn key(self) -> String {
        match (self.column, self.shape) {
            (Column::CommandKey, Shape::Genuine) => sized(LIMIT),
            _ => self.key.to_owned(),
        }
    }

    /// The substituted value; a positive substitutes nothing.
    fn value(self) -> String {
        match self.shape {
            Shape::Empty | Shape::Genuine => String::new(),
            Shape::Over => sized(LIMIT + 1),
            Shape::Short => "a".repeat(DIGEST - 1),
            Shape::Long => "a".repeat(DIGEST + 1),
            Shape::Uppercase => "A".repeat(DIGEST),
            Shape::NonHex => "g".repeat(DIGEST),
            Shape::Outside => "unknown_x".to_owned(),
            Shape::Capitalised => "Unknown".to_owned(),
        }
    }
}

/// The value as the database stored and measured it.
#[derive(sqlx::FromRow)]
struct Configured {
    value: String,
    octets: i32,
}

#[derive(sqlx::FromRow)]
struct Measured {
    value: String,
    octets: i32,
    admitted: bool,
}

fn constraint_target(catalog: &Value, column: Column) -> String {
    let checks: Vec<_> = catalog["checks"]
        .as_array()
        .unwrap()
        .iter()
        // The same definition also guards sibling tables' columns of that name.
        .filter(|row| {
            row["table"] == "workflow_action_evidence" && row["definition"] == column.definition()
        })
        .collect();
    assert_eq!(checks.len(), 1, "{column:?}");
    assert_eq!(checks[0]["validated"], true);
    let name = checks[0]["name"].as_str().unwrap().to_owned();
    assert_eq!(
        name,
        format!("workflow_action_evidence_{}_check", column.name())
    );
    name
}

fn constraint_catalog(catalog: Value) -> Value {
    // Exact fixture exclusions on top of the accepted ones; anything else remains visible.
    let mut catalog = production_catalog(catalog);
    for section in ["triggers", "functions"] {
        catalog[section]
            .as_array_mut()
            .unwrap()
            .retain(|row| !CONSTRAINT_FIXTURE.contains(&row["name"].as_str().unwrap()));
    }
    catalog
}

async fn install(source: &UnknownSource, command: &ReconcileActionCommand, case: ConstraintCase) {
    let pool = source.fixture.persistence().pool();
    for fixture in [
        include_str!("action_reconciliation_native_diagnostic_fixture.sql"),
        include_str!("action_reconciliation_native_constraint_fixture.sql"),
    ] {
        sqlx::raw_sql(fixture).execute(pool).await.unwrap();
    }
    let configured: Configured = sqlx::query_as(CONFIGURE)
        .bind(command.scope.company.as_uuid())
        .bind(command.command_key.as_str())
        .bind(case.outcome().mode())
        .bind(case.column.name())
        .bind(case.value())
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(configured.value, case.value());
    assert_eq!(
        usize::try_from(configured.octets).unwrap(),
        case.value().len()
    );
}

fn transported(error: AppError, constraint: &str) -> Envelope {
    let AppError::Database(message) = error else {
        panic!("database error required: {error:?}")
    };
    let frame = message
        .strip_prefix("error returned from database: ")
        .unwrap()
        .strip_prefix("FIXTURE_NATIVE_CONSTRAINT_CAPTURED_V1:")
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
        "fixture_native_constraint_before()",
    ] {
        assert!(native.context.contains(frame), "{frame}");
    }
    eprintln!(
        "native_constraint state={} schema={} table={} constraint={} message={}",
        native.state, native.schema, native.table, native.constraint, native.message
    );
    envelope
}

/// The candidate is the original NEW with only the targeted column replaced by the value.
fn assert_substituted(original: &Value, candidate: &Value, case: ConstraintCase) {
    let column = case.column.name();
    assert!(original[column].is_string(), "genuine non-null value");
    assert_ne!(original[column], json!(case.value()));
    let mut expected = original.clone();
    expected[column] = json!(case.value());
    assert_eq!(
        *candidate, expected,
        "exact real NEW identity; only declared target changes"
    );
}

async fn negative(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: ConstraintCase,
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
    assert_original(source, command, &envelope.original, before);
    assert_substituted(&envelope.original, &envelope.candidate, case);
    wait_rollback(&mut observer, &envelope).await;
    assert_eq!(
        all_tables(&source.fixture).await,
        *before,
        "complete public and fixture rollback"
    );
}

/// The genuine stored value, measured in the database against the pinned CHECK expression.
fn assert_genuine(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    column: Column,
    stored: &Measured,
) {
    assert!(stored.admitted);
    let octets = usize::try_from(stored.octets).unwrap();
    assert_eq!(octets, stored.value.len());
    match column {
        Column::CommandKey => {
            assert_eq!(stored.value, command.command_key.as_str());
            assert_eq!(octets, LIMIT);
        }
        Column::RequestDigest => {
            assert_eq!(stored.value, command.request_digest().unwrap().as_str());
            assert_eq!(octets, DIGEST);
        }
        Column::CoverageDigest => {
            let observed = source.verifier.observation.lock().unwrap();
            assert_eq!(stored.value, observed.as_ref().unwrap()["coverage_digest"]);
            assert_eq!(octets, DIGEST);
        }
        Column::OperationSignature => {
            assert_eq!(
                stored.value,
                source.verifier.registration().operation().as_str()
            );
            assert_eq!(octets, DIGEST);
        }
        Column::Disposition => assert_eq!(stored.value, "unknown"),
    }
}

async fn positive(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: ConstraintCase,
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
    assert_original(source, command, &images["original"], before);
    assert_eq!(
        images["candidate"], images["original"],
        "nothing substituted"
    );
    let column = case.column.name();
    let stored: Measured = sqlx::query_as(MEASURE)
        .bind(result.evidence.unwrap().as_uuid())
        .bind(column)
        .fetch_one(source.fixture.persistence().pool())
        .await
        .unwrap();
    assert_eq!(json!(stored.value), images["original"][column]);
    assert_genuine(source, command, case.column, &stored);
    assert_eq!(*one(&after, "workflow_action_evidence"), images["original"]);
    assert_witness(command, &result, &after);
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        after[relation] = before[relation].clone();
    }
    assert_evidence(source, command, &result, &after);
    assert_commit(command, &result, before, &after);
    eprintln!(
        "native_constraint positive column={column} value_octets={} admitted={} selected_flush=true all_flush=true ordinary_commit=true",
        stored.octets, stored.admitted
    );
}

fn assert_configured(before: &Value, command: &ReconcileActionCommand, case: ConstraintCase) {
    let config = one(before, "fixture_native_constraint_config");
    assert_eq!(config["command_key"], command.command_key.as_str());
    assert_eq!(config["mode"], case.outcome().mode());
    assert_eq!(config["target"], case.column.name());
    assert_eq!(config["value"], json!(case.value()));
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        assert_eq!(before[relation], json!([]));
    }
}

async fn exercise(case: ConstraintCase) {
    // Box the real fixture/service seams for stock 2 MiB libtest stacks.
    let source = Box::pin(unknown_source()).await;
    let key = case.key();
    let command = proof_command(
        &source.fixture,
        &source.request,
        &source.verifier.ledger,
        &key,
    )
    .await;
    assert_eq!(command.command_key.as_str(), key);
    let catalog = native_catalog(&source.fixture).await;
    assert_target(&catalog);
    let constraint = constraint_target(&catalog, case.column);
    install(&source, &command, case).await;
    assert_eq!(
        constraint_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    let before = all_tables(&source.fixture).await;
    assert_source(&source, &before);
    assert_configured(&before, &command, case);
    match case.outcome() {
        Outcome::Positive => Box::pin(positive(&source, &command, case, &before)).await,
        _ => Box::pin(negative(&source, &command, case, &constraint, &before)).await,
    }
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 0);
    assert_eq!(
        constraint_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    source.fixture.persistence().pool().close().await;
}

async fn matrix(column: Column, cases: &[(&'static str, Shape)]) {
    for &(key, shape) in cases {
        Box::pin(exercise(ConstraintCase { key, column, shape })).await;
    }
}

const DIGEST_SHAPES: [Shape; 5] = [
    Shape::Short,
    Shape::Long,
    Shape::Uppercase,
    Shape::NonHex,
    Shape::Genuine,
];

async fn digest_matrix(column: Column, keys: [&'static str; 5]) {
    let cases: Vec<_> = keys.into_iter().zip(DIGEST_SHAPES).collect();
    Box::pin(matrix(column, &cases)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_constraint_command_key() {
    let cases = [
        ("command-key-empty", Shape::Empty),
        ("command-key-129", Shape::Over),
        // The key is the genuine 128-byte boundary value; this label is unused.
        ("command-key-128", Shape::Genuine),
    ];
    Box::pin(matrix(Column::CommandKey, &cases)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_constraint_request_digest() {
    let keys = [
        "request-digest-63",
        "request-digest-65",
        "request-digest-upper",
        "request-digest-nonhex",
        "request-digest-genuine",
    ];
    Box::pin(digest_matrix(Column::RequestDigest, keys)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_constraint_coverage_digest() {
    let keys = [
        "coverage-digest-63",
        "coverage-digest-65",
        "coverage-digest-upper",
        "coverage-digest-nonhex",
        "coverage-digest-genuine",
    ];
    Box::pin(digest_matrix(Column::CoverageDigest, keys)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_constraint_operation_signature() {
    let keys = [
        "signature-63",
        "signature-65",
        "signature-upper",
        "signature-nonhex",
        "signature-genuine",
    ];
    Box::pin(digest_matrix(Column::OperationSignature, keys)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_constraint_disposition() {
    let cases = [
        ("disposition-outside", Shape::Outside),
        ("disposition-capitalised", Shape::Capitalised),
        ("disposition-empty", Shape::Empty),
        ("disposition-genuine", Shape::Genuine),
    ];
    Box::pin(matrix(Column::Disposition, &cases)).await;
}
