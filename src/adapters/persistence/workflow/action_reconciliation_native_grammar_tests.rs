//! Native provenance-grammar attribution: the 128-byte length and the explicit grammar of the
//! registration, verifier_version, provider and authoritative_reference column CHECKs, each
//! substituted into a genuine prospective service INSERT of a registered source.
use super::*;

const GRAMMAR_FIXTURE: &[&str] = &[
    "fixture_native_grammar_before",
    "fixture_native_grammar_after",
];
const LIMIT: usize = 128;
const CONFIGURE: &str = "INSERT INTO fixture_native_grammar_config(company_id,command_key,mode,target,value) \
    VALUES($1,$2,$3,$4,$5) RETURNING value,octet_length(value) AS octets";
// Measured on the stored row, in the column the case targets.
const MEASURE: &str = "SELECT CASE $2 WHEN 'registration' THEN evidence.registration \
    WHEN 'verifier_version' THEN evidence.verifier_version WHEN 'provider' THEN evidence.provider \
    WHEN 'authoritative_reference' THEN evidence.authoritative_reference END AS value, \
    octet_length(CASE $2 WHEN 'registration' THEN evidence.registration \
    WHEN 'verifier_version' THEN evidence.verifier_version WHEN 'provider' THEN evidence.provider \
    WHEN 'authoritative_reference' THEN evidence.authoritative_reference END) AS octets \
    FROM workflow_action_evidence AS evidence WHERE evidence.id=$1";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Column {
    Registration,
    VerifierVersion,
    Provider,
    AuthoritativeReference,
}

impl Column {
    fn name(self) -> &'static str {
        match self {
            Self::Registration => "registration",
            Self::VerifierVersion => "verifier_version",
            Self::Provider => "provider",
            Self::AuthoritativeReference => "authoritative_reference",
        }
    }

    /// Read from a freshly migrated PostgreSQL 18.6 database; only the generated name is derived.
    fn definition(self) -> &'static str {
        match self {
            Self::Registration => {
                "CHECK ((registration ~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'::text))"
            }
            Self::VerifierVersion => {
                "CHECK ((verifier_version ~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'::text))"
            }
            Self::Provider => "CHECK ((provider ~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'::text))",
            Self::AuthoritativeReference => {
                "CHECK ((authoritative_reference ~ '^[A-Za-z0-9][A-Za-z0-9_./:-]{0,127}$'::text))"
            }
        }
    }

    /// Every non-leading character the column's grammar admits.
    fn alphabet(self) -> &'static str {
        match self {
            Self::AuthoritativeReference => "a9_./:-",
            _ => "a9_.-",
        }
    }

    /// Within the length limit, so only the grammar conjunct can reject it.
    fn ungrammatical(self) -> &'static str {
        match self {
            Self::AuthoritativeReference => "ref?grammar",
            _ => ".grammar",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Limit,
    Over,
    Ungrammatical,
}

#[derive(Clone, Copy, Debug)]
struct GrammarCase {
    key: &'static str,
    column: Column,
    shape: Shape,
}

impl GrammarCase {
    fn outcome(self) -> Outcome {
        match self.shape {
            Shape::Limit => Outcome::Positive,
            Shape::Over | Shape::Ungrammatical => Outcome::Rejected,
        }
    }

    fn value(self) -> String {
        let sized = |len: usize| -> String {
            let alphabet = self.column.alphabet().as_bytes();
            let tail = (0..len - 1).map(|index| char::from(alphabet[index % alphabet.len()]));
            std::iter::once('R').chain(tail).collect()
        };
        match self.shape {
            Shape::Limit => sized(LIMIT),
            Shape::Over => sized(LIMIT + 1),
            Shape::Ungrammatical => self.column.ungrammatical().to_owned(),
        }
    }
}

/// The value as the database stored and measured it.
#[derive(sqlx::FromRow)]
struct Measured {
    value: String,
    octets: i32,
}

fn grammar_target(catalog: &Value, column: Column) -> String {
    let checks: Vec<_> = catalog["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["definition"] == column.definition())
        .collect();
    assert_eq!(checks.len(), 1, "{column:?}");
    assert_eq!(checks[0]["table"], "workflow_action_evidence");
    assert_eq!(checks[0]["validated"], true);
    checks[0]["name"].as_str().unwrap().to_owned()
}

fn grammar_catalog(catalog: Value) -> Value {
    // Exact fixture exclusions on top of the accepted ones; anything else remains visible.
    let mut catalog = production_catalog(catalog);
    for section in ["triggers", "functions"] {
        catalog[section]
            .as_array_mut()
            .unwrap()
            .retain(|row| !GRAMMAR_FIXTURE.contains(&row["name"].as_str().unwrap()));
    }
    catalog
}

async fn install(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: GrammarCase,
) -> Measured {
    let pool = source.fixture.persistence().pool();
    for fixture in [
        include_str!("action_reconciliation_native_diagnostic_fixture.sql"),
        include_str!("action_reconciliation_native_grammar_fixture.sql"),
    ] {
        sqlx::raw_sql(fixture).execute(pool).await.unwrap();
    }
    let configured: Measured = sqlx::query_as(CONFIGURE)
        .bind(command.scope.company.as_uuid())
        .bind(command.command_key.as_str())
        .bind(case.outcome().mode())
        .bind(case.column.name())
        .bind(case.value())
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(configured.value, case.value());
    let expected = match case.shape {
        Shape::Limit => LIMIT,
        Shape::Over => LIMIT + 1,
        Shape::Ungrammatical => case.column.ungrammatical().len(),
    };
    assert_eq!(usize::try_from(configured.octets).unwrap(), expected);
    configured
}

fn transported(error: AppError, constraint: &str) -> Envelope {
    let AppError::Database(message) = error else {
        panic!("database error required: {error:?}")
    };
    let frame = message
        .strip_prefix("error returned from database: ")
        .unwrap()
        .strip_prefix("FIXTURE_NATIVE_GRAMMAR_CAPTURED_V1:")
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
        "fixture_native_grammar_before()",
    ] {
        assert!(native.context.contains(frame), "{frame}");
    }
    eprintln!(
        "native_grammar state={} schema={} table={} constraint={} message={}",
        native.state, native.schema, native.table, native.constraint, native.message
    );
    envelope
}

/// The candidate is the original NEW with only the targeted column replaced by the value.
fn assert_substituted(original: &Value, candidate: &Value, case: GrammarCase) {
    let column = case.column.name();
    assert!(original[column].is_string(), "registered source");
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
    case: GrammarCase,
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

async fn positive(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    case: GrammarCase,
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
    let stored: Measured = sqlx::query_as(MEASURE)
        .bind(result.evidence.unwrap().as_uuid())
        .bind(case.column.name())
        .fetch_one(source.fixture.persistence().pool())
        .await
        .unwrap();
    assert_eq!(stored.value, case.value());
    assert_eq!(usize::try_from(stored.octets).unwrap(), LIMIT);
    assert_eq!(
        *one(&after, "workflow_action_evidence"),
        images["candidate"]
    );
    assert_substituted(&images["original"], &images["candidate"], case);
    assert_witness(command, &result, &after);
    // Evidence is immutable, so the swap back is to this snapshot only: the remaining
    // assertions then prove the genuine issued envelope and an otherwise ordinary commit.
    let column = case.column.name();
    after["workflow_action_evidence"][0][column] = images["original"][column].clone();
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        after[relation] = before[relation].clone();
    }
    assert_evidence(source, command, &result, &after);
    assert_commit(command, &result, before, &after);
    eprintln!(
        "native_grammar positive column={column} octets={} selected_flush=true all_flush=true ordinary_commit=true",
        stored.octets
    );
}

fn assert_configured(before: &Value, command: &ReconcileActionCommand, case: GrammarCase) {
    let config = one(before, "fixture_native_grammar_config");
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

async fn exercise(case: GrammarCase) {
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
    let constraint = grammar_target(&catalog, case.column);
    install(&source, &command, case).await;
    assert_eq!(
        grammar_catalog(native_catalog(&source.fixture).await),
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
        grammar_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    source.fixture.persistence().pool().close().await;
}

async fn matrix(column: Column, keys: [&'static str; 3]) {
    for (key, shape) in keys
        .into_iter()
        .zip([Shape::Over, Shape::Ungrammatical, Shape::Limit])
    {
        Box::pin(exercise(GrammarCase { key, column, shape })).await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_native_grammar_registration() {
    let keys = [
        "registration-129",
        "registration-grammar",
        "registration-128",
    ];
    Box::pin(matrix(Column::Registration, keys)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_grammar_verifier_version() {
    let keys = ["verifier-129", "verifier-grammar", "verifier-128"];
    Box::pin(matrix(Column::VerifierVersion, keys)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_grammar_provider() {
    let keys = ["provider-129", "provider-grammar", "provider-128"];
    Box::pin(matrix(Column::Provider, keys)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_grammar_authoritative_reference() {
    let keys = ["reference-129", "reference-grammar", "reference-128"];
    Box::pin(matrix(Column::AuthoritativeReference, keys)).await;
}
