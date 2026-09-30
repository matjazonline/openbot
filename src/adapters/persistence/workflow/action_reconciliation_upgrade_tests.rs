//! Upgrade at the last schema before exact claim episodes (20261001090000).
//! The test-only legacy owner is byte-for-byte lease_claim.rs from committed
//! c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc (SHA256 recorded in the evidence).
//! It creates historical claims only; no current owner or provenance shim runs
//! against the historical schema. Admission, dispatch, settlement and completion
//! use their unchanged production owners with every historical trigger enabled.
use super::*;
use crate::adapters::persistence::test_support::own_database_with_migrations;
use crate::adapters::persistence::workflow::upgrade_legacy_claim as legacy_claim;
use sqlx::migrate::{MigrateError, Migrator};
use std::borrow::Cow;

#[path = "action_reconciliation_upgrade_catalog_diagnostics.rs"]
mod catalog_diagnostics;

const HISTORY_BOUNDARY: i64 = 20261001090000;
const EPISODE_MIGRATION: i64 = 20261001100000;
static CURRENT: Migrator = sqlx::migrate!("./migrations");

fn historical_migrations() -> Migrator {
    Migrator {
        migrations: Cow::Owned(
            CURRENT
                .iter()
                .filter(|migration| migration.version <= HISTORY_BOUNDARY)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
}

async fn assert_migration_history(pool: &sqlx::PgPool, migrations: &Migrator) {
    let rows: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(rows.len(), migrations.iter().count());
    for ((version, checksum, success), expected) in rows.iter().zip(migrations.iter()) {
        assert_eq!(*version, expected.version);
        assert_eq!(checksum.as_slice(), expected.checksum.as_ref());
        assert!(*success);
    }
}

async fn historical_claim(f: &AdmissionFixture, scope: ActivationRequest) -> ClaimedWorkflow {
    let started = tokio::time::Instant::now();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let claim = Box::pin(legacy_claim::claim_on(
        &mut tx,
        scope,
        worker(),
        policy(),
        started,
    ))
    .await
    .unwrap()
    .expect("the genuine historical owner claims the eligible job");
    tx.commit().await.unwrap();
    claim
}

async fn historical_fixture() -> (AdmissionFixture, ActionDispatchRequest) {
    let prefix = historical_migrations();
    let db = Arc::new(
        own_database_with_migrations(&prefix)
            .await
            .expect("upgrade tests require a task-owned database"),
    );
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    println!("upgrade historical database: {name}");
    assert_migration_history(&db.pool, &prefix).await;
    let fixture = Fixture::in_database(db, "").await;
    let source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    let binding = BindingFixture::from_fixture(fixture, source).await;
    let f = AdmissionFixture::with_binding(binding).await;
    let admitted = f.prepare(f.request("upgrade-history", f.manual())).await;
    assert!(matches!(
        f.persistence().admit(&admitted).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    let job = sqlx::query_scalar(
        "SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2",
    )
    .bind(admitted.company_id().as_uuid())
    .bind(admitted.first_execution_id().as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let scope = ActivationRequest {
        company: admitted.company_id(),
        run: admitted.proposed_run_id(),
        execution: admitted.first_execution_id(),
        job: WorkflowJobId(job),
    };
    let claim = Box::pin(historical_claim(&f, scope)).await;
    Box::pin(setup_action_on_claim(
        f,
        claim,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await
}

async fn historical_schedule() -> (AdmissionFixture, ActionDispatchRequest) {
    // Real provider/action seams stay boxed on stock 2 MiB stacks.
    let (f, request) = Box::pin(historical_fixture()).await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    assert!(matches!(
        invoke(&f, &request, &provider).await,
        Err(AppError::Timeout(_))
    ));
    park(&f, &request).await;
    let saved = reconcile(&f, &request, marker(&f).await, verifier, "upgrade-recovery").await;
    assert_eq!(
        saved.outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    );
    let history = all_tables(&f).await;
    for table in [
        "workflow_action_evidence_commands",
        "workflow_action_evidence",
        "workflow_action_receipts",
        "workflow_action_remote_entries",
        "workflow_action_dispatches",
        "workflow_run_events",
        "task_attempts",
        "workflow_budget_receipts",
        "workflow_root_budget_usage",
    ] {
        assert!(
            !history[table].as_array().unwrap().is_empty(),
            "real {table}"
        );
    }
    assert!(history.get("workflow_action_claim_episodes").is_none());
    let command = &history["workflow_action_evidence_commands"][0];
    assert_eq!(command["outcome"]["kind"], "scheduled");
    assert_eq!(
        command["scheduled_job_id"],
        request.fence.scope.job.0.to_string()
    );
    assert_eq!(history["background_tasks"][0]["status"], "pending");
    (f, request)
}

fn assert_history_preserved(before: &Value, after: &Value) {
    for (table, rows) in before.as_object().unwrap() {
        if table == "_sqlx_migrations" {
            for row in rows.as_array().unwrap() {
                assert!(after[table].as_array().unwrap().contains(row));
            }
        } else {
            assert_eq!(rows, &after[table], "original bytes of {table}");
        }
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_upgrade_fresh_exact_migrations() {
    let database = own_database().await.expect("fresh upgrade database");
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    println!("upgrade fresh database: {name}");
    assert_migration_history(&database.pool, &CURRENT).await;
    CURRENT.run(&database.pool).await.unwrap();
    assert_migration_history(&database.pool, &CURRENT).await;
    let empty: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM workflow_action_claim_episodes) AND NOT EXISTS(SELECT 1 FROM workflow_action_schedule_witnesses) AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals)")
        .fetch_one(&database.pool).await.unwrap();
    assert!(empty);
}

#[tokio::test]
async fn workflow_action_reconciliation_upgrade_populated_preserves_completed_history() {
    let (f, mut request) = Box::pin(historical_schedule()).await;
    let claim = Box::pin(historical_claim(&f, request.fence.scope)).await;
    request.fence = claim.fence;
    let restart = LedgerProvider::new(&f, Delivery::Pending);
    let RemoteDispatchObservation::Committed(receipt) =
        invoke(&f, &request, &restart).await.unwrap()
    else {
        panic!("historical recovered receipt survives restart");
    };
    assert_eq!(restart.calls.load(Ordering::SeqCst), 0);
    assert!(
        f.persistence()
            .complete_io(FencedWorkflowResult {
                fence: request.fence,
                output: receipt.result,
            })
            .await
            .unwrap()
            .is_some()
    );
    let before = all_tables(&f).await;
    assert_eq!(before["background_tasks"][0]["status"], "completed");
    assert_eq!(before["task_attempts"].as_array().unwrap().len(), 2);
    assert_migration_history(f.persistence().pool(), &historical_migrations()).await;
    CURRENT.run(f.persistence().pool()).await.unwrap();
    assert_migration_history(f.persistence().pool(), &CURRENT).await;
    let after = all_tables(&f).await;
    assert_history_preserved(&before, &after);
    for table in [
        "workflow_action_claim_episodes",
        "workflow_action_schedule_witnesses",
        "workflow_action_claim_budget_refusals",
    ] {
        assert_eq!(after[table], json!([]), "no guessed historical {table}");
    }
    CURRENT.run(f.persistence().pool()).await.unwrap();
    assert_eq!(
        after,
        all_tables(&f).await,
        "upgrade rerun changes no history"
    );
}

async fn schema_catalog(executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>) -> Value {
    sqlx::query_scalar(r#"SELECT jsonb_build_object(
        'relations',(SELECT jsonb_agg(to_jsonb(relation) ORDER BY relation.oid) FROM pg_class AS relation JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace WHERE namespace.nspname='public'),
        'attributes',(SELECT jsonb_agg(to_jsonb(attribute) ORDER BY attribute.attrelid,attribute.attnum) FROM pg_attribute AS attribute JOIN pg_class AS relation ON relation.oid=attribute.attrelid JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace WHERE namespace.nspname='public'),
        'constraints',(SELECT jsonb_agg(to_jsonb(constraint_row) ORDER BY constraint_row.oid) FROM pg_constraint AS constraint_row JOIN pg_namespace AS namespace ON namespace.oid=constraint_row.connamespace WHERE namespace.nspname='public'),
        'triggers',(SELECT jsonb_agg(to_jsonb(trigger_row) ORDER BY trigger_row.oid) FROM pg_trigger AS trigger_row JOIN pg_class AS relation ON relation.oid=trigger_row.tgrelid JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace WHERE namespace.nspname='public'),
        'functions',(SELECT jsonb_agg(to_jsonb(function_row) ORDER BY function_row.oid) FROM pg_proc AS function_row JOIN pg_namespace AS namespace ON namespace.oid=function_row.pronamespace WHERE namespace.nspname='public'))"#)
        .fetch_one(executor).await.unwrap()
}

#[tokio::test]
async fn workflow_action_reconciliation_upgrade_ambiguous_pending_rejects_atomically() {
    let (f, request) = Box::pin(historical_schedule()).await;
    let before = all_tables(&f).await;
    assert_eq!(before["task_attempts"].as_array().unwrap().len(), 1);
    assert_eq!(
        before["background_tasks"][0]["id"],
        request.fence.scope.job.0.to_string()
    );
    let catalog = schema_catalog(f.persistence().pool()).await;
    let context = catalog_diagnostics::context(f.persistence().pool()).await;
    for attempt in 1..=2 {
        // SQLx 0.8 returns before releasing its session advisory lock on migration errors.
        // Close our session rather than returning that lock to the pool between attempts.
        let mut connection = f.persistence().pool().acquire().await.unwrap();
        connection.close_on_drop();
        let error = CURRENT.run(&mut *connection).await.unwrap_err();
        connection.close().await.unwrap();
        let MigrateError::ExecuteMigration(error, version) = error else {
            panic!("unexpected migration error: {error}");
        };
        assert_eq!(version, EPISODE_MIGRATION);
        let database = error.as_database_error().expect("named database preflight");
        assert_eq!(database.code().as_deref(), Some("23514"));
        assert_eq!(
            database.message(),
            "workflow reconciliation pending episode provenance is ambiguous"
        );
        assert_eq!(
            before,
            all_tables(&f).await,
            "every row and migration metadata roll back"
        );
        let after = schema_catalog(f.persistence().pool()).await;
        let after_context = catalog_diagnostics::context(f.persistence().pool()).await;
        catalog_diagnostics::assert_unchanged(&catalog, &after, &context, &after_context, attempt);
        assert_migration_history(f.persistence().pool(), &historical_migrations()).await;
    }
}
