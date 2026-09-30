//! Immediate native CHECK attribution from a genuine prospective service INSERT.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeDiagnostic {
    state: String,
    schema: String,
    table: String,
    constraint: String,
    message: String,
    context: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    mode: String,
    original: Value,
    candidate: Value,
    native: NativeDiagnostic,
    owner_pid: i32,
    database: String,
    depth: u32,
}

#[derive(Clone, Copy, Debug)]
enum Probe {
    Scalar,
    Acceptance,
    Positive,
}

impl Probe {
    fn mode(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Acceptance => "acceptance",
            Self::Positive => "positive",
        }
    }
}

async fn install(source: &UnknownSource, command: &ReconcileActionCommand, probe: Probe) {
    sqlx::raw_sql(include_str!(
        "action_reconciliation_native_diagnostic_fixture.sql"
    ))
    .execute(source.fixture.persistence().pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO fixture_native_diagnostic_config(company_id,command_key,mode) VALUES($1,$2,$3)")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str()).bind(probe.mode())
        .execute(source.fixture.persistence().pool()).await.unwrap();
}

fn production_catalog(mut catalog: Value) -> Value {
    // Exact fixture exclusions; a differently named production change remains visible.
    for (section, names) in [
        (
            "triggers",
            vec![
                "fixture_native_diagnostic_before",
                "fixture_native_diagnostic_after",
            ],
        ),
        (
            "functions",
            vec![
                "fixture_native_diagnostic_before",
                "fixture_native_diagnostic_after",
                "fixture_native_diagnostic_probe",
            ],
        ),
    ] {
        catalog[section]
            .as_array_mut()
            .unwrap()
            .retain(|row| !names.contains(&row["name"].as_str().unwrap()));
    }
    catalog
}

fn assert_target(catalog: &Value) {
    let checks: Vec<_> = catalog["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["table"] == "workflow_action_evidence"
                && row["name"] == "workflow_action_evidence_diagnostic_check"
        })
        .collect();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0]["validated"], true);
    assert_eq!(
        checks[0]["definition"],
        "CHECK (((jsonb_typeof(diagnostic) = 'object'::text) AND (octet_length((diagnostic)::text) <= 16384)))"
    );
}

fn transported(error: AppError, probe: Probe) -> Envelope {
    let AppError::Database(message) = error else {
        panic!("database error required: {error:?}")
    };
    let prefix = match probe {
        Probe::Scalar => "FIXTURE_NATIVE_DIAGNOSTIC_CAPTURED_V1:",
        Probe::Acceptance => "FIXTURE_NATIVE_DIAGNOSTIC_ACCEPTED_V1:",
        Probe::Positive => panic!("positive must commit"),
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
    assert_eq!(envelope.mode, probe.mode());
    let native = &envelope.native;
    match probe {
        Probe::Scalar => {
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
        Probe::Acceptance => {
            assert_eq!(native.state, "PZ001");
            assert_eq!(native.message, "native diagnostic probe accepted");
            assert_eq!(native.schema, "");
            assert_eq!(native.table, "");
            assert_eq!(native.constraint, "");
            assert!(native.context.contains("at RAISE"));
        }
        Probe::Positive => unreachable!(),
    }
    eprintln!(
        "native_diagnostic mode={probe:?} state={} schema={} table={} constraint={} message={}",
        native.state, native.schema, native.table, native.constraint, native.message
    );
    envelope
}

fn assert_original(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    row: &Value,
    before: &Value,
) {
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
    assert_eq!(row["disposition"], "unknown");
    assert_eq!(row["grant_eligible"], false);
    assert!(row["applied_request"].is_null());
    assert!(row["applied_remote_entry_id"].is_null());
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

async fn wait_rollback(
    observer: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    envelope: &Envelope,
) {
    let observer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut **observer)
        .await
        .unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&mut **observer)
        .await
        .unwrap();
    assert_ne!(observer_pid, envelope.owner_pid);
    assert_eq!(database, envelope.database);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()").execute(&mut **observer).await.unwrap();
            let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=$1 AND (activity.datname IS DISTINCT FROM $2 OR activity.state IS DISTINCT FROM 'idle' OR activity.xact_start IS NOT NULL))")
                .bind(envelope.owner_pid).bind(&envelope.database).fetch_one(&mut **observer).await.unwrap();
            if clean { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("bounded owner rollback completion");
    eprintln!(
        "native_diagnostic rollback_quiescent owner={} observer={observer_pid} database={database}",
        envelope.owner_pid
    );
}

async fn negative(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    probe: Probe,
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
    let envelope = transported(error, probe);
    assert_original(source, command, &envelope.original, before);
    let mut expected = envelope.original.clone();
    if matches!(probe, Probe::Scalar) {
        expected["diagnostic"] = json!(7);
    }
    assert_eq!(
        envelope.candidate, expected,
        "exact real NEW identity; only declared target changes"
    );
    wait_rollback(&mut observer, &envelope).await;
    assert_eq!(
        all_tables(&source.fixture).await,
        *before,
        "complete public and fixture rollback"
    );
}

async fn positive(source: &UnknownSource, command: &ReconcileActionCommand, before: &Value) {
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
    assert_evidence(source, command, &result, &after);
    let images = one(&after, "fixture_native_diagnostic_images");
    assert_eq!(images["original"], images["candidate"]);
    assert_eq!(images["original"], *one(&after, "workflow_action_evidence"));
    assert_eq!(images["depth"], 1);
    let witness = one(&after, "fixture_native_diagnostic_witness");
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
        one(&after, "workflow_action_evidence_commands")["id"]
    );
    assert_eq!(witness["selected_flush"], true);
    assert_eq!(witness["all_flush"], true);
    for relation in [
        "fixture_native_diagnostic_images",
        "fixture_native_diagnostic_witness",
    ] {
        assert_eq!(before[relation], json!([]));
        after[relation] = before[relation].clone();
    }
    assert_commit(command, &result, before, &after);
    eprintln!(
        "native_diagnostic positive original_new=true selected_flush=true all_flush=true ordinary_commit=true"
    );
}

async fn exercise(probe: Probe) {
    // Box the real fixture/service seams for stock 2 MiB libtest stacks.
    let source = Box::pin(unknown_source()).await;
    let command = proof_command(
        &source.fixture,
        &source.request,
        &source.verifier.ledger,
        probe.mode(),
    )
    .await;
    let catalog = native_catalog(&source.fixture).await;
    assert_target(&catalog);
    install(&source, &command, probe).await;
    assert_eq!(
        production_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    let before = all_tables(&source.fixture).await;
    assert_source(&source, &before);
    match probe {
        Probe::Positive => Box::pin(positive(&source, &command, &before)).await,
        _ => Box::pin(negative(&source, &command, probe, &before)).await,
    }
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 0);
    assert_eq!(
        production_catalog(native_catalog(&source.fixture).await),
        catalog
    );
    source.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_diagnostic_scalar_and_controls() {
    for probe in [Probe::Scalar, Probe::Acceptance, Probe::Positive] {
        Box::pin(exercise(probe)).await;
    }
}
