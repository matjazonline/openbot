//! Composed OtherAudit proof: service UNIQUE and separately labelled native SQL controls.
use super::*;
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgDatabaseError, PgPoolOptions};

#[path = "action_reconciliation_other_audit_sql_tests.rs"]
mod sql_controls;

const OWNERSHIP: &str = "workflow_action_command_audit_unique";

#[derive(Clone, Copy, Debug)]
enum OwnershipMode {
    Unchanged,
    OtherAudit,
}
impl OwnershipMode {
    fn key(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::OtherAudit => "other-audit",
        }
    }
}

struct UnknownOwner {
    fixture: AdmissionFixture,
    verifier: Arc<LedgerVerifier>,
    candidate: ReconcileActionCommand,
    history: Value,
    expected: Value,
}

async fn unknown_owner() -> UnknownOwner {
    // Box the real admission/action seam to retain stock 2 MiB test stacks.
    let (f, request) = Box::pin(setup()).await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(matches!(
        Box::pin(invoke(&f, &request, &provider)).await,
        Err(AppError::Timeout(_))
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    park(&f, &request).await;
    let mut first = command(&f, &request, marker(&f).await).await;
    first.command_key = IdempotencyKey::parse("genuine-unknown-c1").unwrap();
    let saved = Box::pin(run_command(&f, &first, verifier.clone()))
        .await
        .unwrap();
    assert_eq!(saved.outcome, ReconciliationOutcome::UnknownRecorded);
    let history: Value = sqlx::query_scalar("SELECT jsonb_build_object('command',to_jsonb(command),'evidence',to_jsonb(evidence),'audit',to_jsonb(audit)) FROM workflow_action_evidence_commands AS command JOIN workflow_action_evidence AS evidence ON evidence.company_id=command.company_id AND evidence.id=command.evidence_id JOIN workflow_run_events AS audit ON audit.company_id=command.company_id AND audit.run_id=command.run_id AND audit.sequence=command.audit_sequence WHERE command.company_id=$1 AND command.command_key=$2")
        .bind(first.scope.company.as_uuid()).bind(first.command_key.as_str()).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(
        history["command"]["evidence_id"],
        saved.evidence.unwrap().as_uuid().to_string()
    );
    assert_eq!(history["command"]["result_revision"], saved.revision.0);
    assert_eq!(
        history["audit"]["execution_id"],
        first.scope.execution.as_uuid().to_string()
    );
    assert_eq!(
        history["audit"]["actor_id"],
        first.actor.user_id().to_string()
    );
    assert_eq!(history["audit"]["event_kind"], "action_reconciled");
    assert_eq!(history["evidence"]["command_id"], history["command"]["id"]);
    assert_eq!(
        history["evidence"]["request_digest"],
        first.request_digest().unwrap().as_str()
    );
    barrier(&f, &request).await;
    assert!(!delayed_apply(&f, &request, &good()).await);
    let candidate = proof_command(&f, &request, &verifier, "genuine-final-c2").await;
    let scope = candidate.scope;
    let common = json!({"company_id":scope.company.as_uuid(),"run_id":scope.run.as_uuid(),"execution_id":scope.execution.as_uuid(),
        "invocation_id":candidate.subject.invocation.as_uuid(),"argument_digest":candidate.subject.argument_digest.as_str(),
        "dispatch_id":candidate.marker.as_uuid(),"actor_id":candidate.actor.user_id(),"command_key":candidate.command_key.as_str(),
        "request_digest":candidate.request_digest().unwrap().as_str()});
    let mut evidence = common.clone();
    evidence.as_object_mut().unwrap().extend(json!({"disposition":"final_not_applied","grant_eligible":true,
        "registration":verifier.registration.id().as_str(),"verifier_version":verifier.registration.version().as_str(),
        "provider":verifier.registration.provider().as_str(),"operation_signature":verifier.registration.operation().as_str(),
        "authoritative_reference":candidate.command_key.as_str(),"applied_request":null,"applied_remote_entry_id":null}).as_object().unwrap().clone());
    let mut command_expected = common;
    command_expected.as_object_mut().unwrap().extend(
        json!({
            "expected_revision":candidate.expected_revision.0,
            "scheduled_job_id":request.fence.scope.job.0,
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    UnknownOwner {
        fixture: f,
        verifier,
        candidate,
        history,
        expected: json!({"command":command_expected,"evidence":evidence}),
    }
}

#[derive(Debug)]
struct Backend {
    pid: i32,
    database: String,
    started: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct Activity {
    database: String,
    started: DateTime<Utc>,
    state: String,
    transaction: Option<DateTime<Utc>>,
}

async fn backend(
    connection: &mut sqlx::PgConnection,
    observer: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
) -> Backend {
    let (pid, database): (i32, String) =
        sqlx::query_as("SELECT pg_backend_pid(),current_database()::text")
            .fetch_one(connection)
            .await
            .unwrap();
    let started = sqlx::query_scalar(
        "SELECT backend_start FROM pg_stat_activity WHERE pid=$1 AND datname=$2",
    )
    .bind(pid)
    .bind(&database)
    .fetch_one(&mut **observer)
    .await
    .unwrap();
    Backend {
        pid,
        database,
        started,
    }
}

async fn quiescent(observer: &mut sqlx::pool::PoolConnection<sqlx::Postgres>, owner: &Backend) {
    let observer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut **observer)
        .await
        .unwrap();
    assert_ne!(observer_pid, owner.pid);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut **observer)
                .await
                .unwrap();
            let row: Option<Activity> = sqlx::query_as("SELECT datname::text AS database,backend_start AS started,state,xact_start AS transaction FROM pg_stat_activity WHERE pid=$1")
                .bind(owner.pid).fetch_optional(&mut **observer).await.unwrap();
            match row {
                None => break,
                Some(activity) => {
                    assert_eq!(activity.database, owner.database);
                    assert_eq!(activity.started, owner.started, "surviving PID must be the same backend");
                    if activity.state == "idle" && activity.transaction.is_none() { break; }
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("bounded correlated actual owner rollback completion");
    eprintln!("actual_backend_quiescent owner={owner:?} observer_pid={observer_pid}");
}

async fn ownership_catalog(f: &AdmissionFixture) -> Value {
    let rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('oid',constraint_row.oid::bigint,'name',constraint_row.conname,'type',constraint_row.contype,'validated',constraint_row.convalidated,'deferrable',constraint_row.condeferrable,'deferred',constraint_row.condeferred,'definition',pg_get_constraintdef(constraint_row.oid),'schema',namespace.nspname,'table',relation.relname,'unique',index_row.indisunique,'valid',index_row.indisvalid,'ready',index_row.indisready,'immediate',index_row.indimmediate) FROM pg_constraint AS constraint_row JOIN pg_class AS relation ON relation.oid=constraint_row.conrelid JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace JOIN pg_index AS index_row ON index_row.indexrelid=constraint_row.conindid WHERE namespace.nspname='public' AND relation.relname='workflow_action_evidence_commands' AND constraint_row.conname=$1")
        .bind(OWNERSHIP).fetch_all(f.persistence().pool()).await.unwrap();
    assert_eq!(rows.len(), 1);
    let row = rows.into_iter().next().unwrap();
    assert_eq!(row["type"], "u");
    assert_eq!(
        row["definition"],
        "UNIQUE (company_id, run_id, audit_sequence)"
    );
    for field in ["validated", "unique", "valid", "ready", "immediate"] {
        assert_eq!(row[field], true);
    }
    assert_eq!(row["deferrable"], false);
    assert_eq!(row["deferred"], false);
    row
}

async fn install(owner: &UnknownOwner, mode: OwnershipMode, backend: &Backend) {
    let f = &owner.fixture;
    sqlx::raw_sql(include_str!(
        "action_reconciliation_other_audit_fixture.sql"
    ))
    .execute(f.persistence().pool())
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "action_reconciliation_other_audit_validate.sql"
    ))
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let baseline = all_tables(f).await;
    sqlx::query("INSERT INTO fixture_other_audit_config(version,mode,company_id,command_key,expected,history,baseline,owner_pid,database_name) VALUES(1,$1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(mode.key()).bind(owner.candidate.scope.company.as_uuid()).bind(owner.candidate.command_key.as_str())
        .bind(&owner.expected).bind(&owner.history).bind(baseline).bind(backend.pid).bind(&backend.database)
        .execute(f.persistence().pool()).await.unwrap();
}

fn service_error_matches(error: &AppError, constraint: &str, native_display: &str) -> bool {
    let AppError::Database(message) = error else {
        return false;
    };
    let expected = format!(
        "error returned from database: duplicate key value violates unique constraint \"{constraint}\""
    );
    message == &expected && message == native_display
}

async fn service_command(
    owner: &UnknownOwner,
    pool: &sqlx::PgPool,
) -> AppResult<ReconciliationResult> {
    let persistence = PostgresPersistence::new(pool.clone());
    ActionReconciliationService::new(
        PostgresActionReconciliation::new(persistence.clone(), Resources),
        LifecycleAuthorizer::new(
            persistence.clone(),
            persistence.clone(),
            persistence.clone(),
        ),
        Directory(persistence),
        Some(owner.verifier.clone()),
    )
    .reconcile(
        &owner.candidate,
        &CancellationToken::new(),
        Duration::from_secs(5),
    )
    .await
}

fn assert_positive(owner: &UnknownOwner, after: &Value, result: &ReconciliationResult) {
    let captures = after["fixture_other_audit_capture"].as_array().unwrap();
    assert_eq!(captures.len(), 1);
    let capture = &captures[0];
    assert_eq!(capture["after_checked"], true);
    assert_eq!(capture["original_new"], capture["returned_new"]);
    let command = &capture["original_new"];
    assert!(
        after["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .contains(command)
    );
    assert_ne!(
        command["audit_sequence"],
        owner.history["audit"]["sequence"]
    );
    assert_ne!(command["id"], owner.history["command"]["id"]);
    assert_eq!(command["result_revision"], result.revision.0);
    assert_eq!(
        command["evidence_id"],
        result.evidence.unwrap().as_uuid().to_string()
    );
    for (table, field) in [
        ("workflow_action_evidence_commands", "command"),
        ("workflow_action_evidence", "evidence"),
        ("workflow_run_events", "audit"),
    ] {
        assert!(
            after[table]
                .as_array()
                .unwrap()
                .contains(&owner.history[field])
        );
    }
    assert_eq!(
        after["workflow_action_claim_episodes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        after["workflow_action_claim_episodes"][0]["command_key"],
        owner.candidate.command_key.as_str()
    );
}

async fn service_pair(mode: OwnershipMode) {
    let owner = Box::pin(unknown_owner()).await;
    let f = &owner.fixture;
    let catalog = ownership_catalog(f).await;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .connect_with(f.persistence().pool().connect_options().as_ref().clone())
        .await
        .unwrap();
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    let actual_backend = backend(&mut connection, &mut observer).await;
    drop(connection);
    install(&owner, mode, &actual_backend).await;
    let native_display = Box::pin(sql_controls::pair(&owner, &mut observer)).await;
    let baseline = all_tables(f).await;
    let result = Box::pin(service_command(&owner, &pool)).await;
    quiescent(&mut observer, &actual_backend).await;
    let after = all_tables(f).await;
    match mode {
        OwnershipMode::Unchanged => {
            let saved = result.unwrap();
            assert_eq!(
                saved.outcome,
                ReconciliationOutcome::Scheduled {
                    receipt_only: false
                }
            );
            assert_positive(&owner, &after, &saved);
        }
        OwnershipMode::OtherAudit => {
            let error = result.expect_err("actual owner must fail native ownership UNIQUE");
            eprintln!("actual_service_native_text_only {error:?}");
            assert!(service_error_matches(&error, OWNERSHIP, &native_display));
            assert!(!service_error_matches(
                &error,
                "workflow_wrong_expected_constraint",
                &native_display
            ));
            assert_eq!(
                baseline, after,
                "correlated rollback preserves every public row, C1 and provider history"
            );
        }
    }
    assert_eq!(effects(f).await, 0);
    assert_eq!(ownership_catalog(f).await, catalog);
    pool.close().await;
    drop(observer);
    f.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_other_audit_genuine_final_positive() {
    Box::pin(service_pair(OwnershipMode::Unchanged)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_other_audit_genuine_unique_rollback() {
    Box::pin(service_pair(OwnershipMode::OtherAudit)).await;
}
