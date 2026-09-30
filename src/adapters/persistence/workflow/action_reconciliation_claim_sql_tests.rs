//! Isolated SQL attacks on exact claim retirement and its protected audit.
use super::*;
use sqlx::{Postgres, Transaction};

#[path = "action_reconciliation_claim_sql_wrong_execution_tests.rs"]
mod wrong_execution_tests;

#[path = "action_reconciliation_claim_sql_historical_tests.rs"]
mod historical_tests;

#[path = "action_reconciliation_claim_sql_episode_tests.rs"]
mod episode_tests;

#[path = "action_reconciliation_claim_sql_deferred_tests.rs"]
mod deferred_tests;

#[path = "action_reconciliation_claim_abort_tests.rs"]
mod abort_tests;

#[path = "action_reconciliation_episode_lifecycle_tests.rs"]
mod episode_lifecycle_tests;

#[path = "action_reconciliation_claim_boundary_tests.rs"]
mod claim_boundary_tests;

#[path = "action_reconciliation_claim_sql_candidate_tests.rs"]
mod candidate_tests;

struct Scheduled {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    command: ReconcileActionCommand,
}

#[derive(sqlx::FromRow)]
struct RetirementWitness {
    retirement_confirmed: bool,
    current_transaction: bool,
    audit_sequence: i64,
}

async fn scheduled() -> Scheduled {
    // Box the real action service seam to retain stock 2 MiB test stacks.
    let (f, request) = Box::pin(limited()).await;
    let verifier = ledger(&f, &request).await;
    assert!(
        invoke(&f, &request, &LedgerProvider::new(&f, Delivery::Pending))
            .await
            .is_err()
    );
    park(&f, &request).await;
    barrier(&f, &request).await;
    let command = proof_command(&f, &request, &verifier, "claim-sql-scheduled").await;
    assert_eq!(
        run_command(&f, &command, verifier).await.unwrap().outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    Scheduled {
        fixture: f,
        request,
        command,
    }
}

async fn exhaust(s: &Scheduled) {
    let scope = child(&s.fixture, s.request.fence.scope).await;
    let claim = s
        .fixture
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    debit(&s.fixture, claim.fence).await;
}

fn guard(error: &sqlx::Error, message: &str) {
    let database = error.as_database_error().unwrap();
    assert_eq!(database.code().as_deref(), Some("23514"), "{error}");
    assert_eq!(database.message(), message, "{error}");
}

async fn audit(tx: &mut Transaction<'_, Postgres>, scope: ActivationRequest, kind: &str) -> i64 {
    // Existing audit owner supplies real owner/actor/execution and sequence allocation.
    crate::adapters::persistence::workflow::waits::audit(&mut *tx, scope, kind)
        .await
        .unwrap();
    sqlx::query_scalar(
        "SELECT MAX(sequence) FROM workflow_run_events WHERE company_id=$1 AND run_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .unwrap()
}

async fn refusal(
    tx: &mut Transaction<'_, Postgres>,
    s: &Scheduled,
    sequence: i64,
) -> Result<(), sqlx::Error> {
    // All episode/attempt/command owner FKs are exact genuine scheduled references.
    let result = sqlx::query("INSERT INTO workflow_action_claim_budget_refusals(company_id,run_id,execution_id,job_id,command_key,retired_attempt,audit_sequence) SELECT company_id,run_id,execution_id,job_id,command_key,retired_attempt,$3 FROM workflow_action_claim_episodes WHERE company_id=$1 AND command_key=$2")
        .bind(s.request.fence.scope.company.as_uuid()).bind(s.command.command_key.as_str())
        .bind(sequence).execute(&mut **tx).await?;
    assert_eq!(result.rows_affected(), 1);
    Ok(())
}

async fn current_retirement(tx: &mut Transaction<'_, Postgres>, s: &Scheduled) -> i64 {
    let scope = s.request.fence.scope;
    assert!(
        crate::adapters::persistence::workflow::lease::lock_scope(&mut *tx, scope, policy(),)
            .await
            .unwrap()
            .is_some()
    );
    // Reuse the production pending retirement owner. Database triggers, rather
    // than supplied provenance, capture OLD pending state and current xid.
    crate::adapters::persistence::workflow::pending_recovery::settle(
        &mut *tx, scope,
        crate::adapters::persistence::workflow::pending_recovery::PendingFailure::RootBudgetExhausted,
    ).await.unwrap();
    let witness: RetirementWitness = sqlx::query_as("SELECT retirement_confirmed,transaction_id=pg_current_xact_id() AS current_transaction,audit_sequence FROM workflow_action_claim_retirement_witnesses WHERE company_id=$1 AND command_key=$2")
        .bind(scope.company.as_uuid()).bind(s.command.command_key.as_str())
        .fetch_one(&mut **tx).await.unwrap();
    assert!(
        witness.retirement_confirmed && witness.current_transaction,
        "genuine current owner retirement"
    );
    witness.audit_sequence
}

async fn rejected_refusal(s: &Scheduled, kind: &str) {
    let before = all_tables(&s.fixture).await;
    let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
    let sequence = audit(&mut tx, s.request.fence.scope, kind).await;
    let error = refusal(&mut tx, s, sequence).await.unwrap_err();
    guard(
        &error,
        "invalid current workflow reconciliation budget refusal",
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "entire forged refusal rolls back"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_eligible_and_caller_labelled_refusals() {
    let s = Box::pin(scheduled()).await;
    let eligible: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_budget_eligible($1,$2)")
            .bind(s.request.fence.scope.company.as_uuid())
            .bind(s.request.fence.scope.run.as_uuid())
            .fetch_one(s.fixture.persistence().pool())
            .await
            .unwrap();
    assert!(eligible);
    Box::pin(rejected_refusal(&s, "workflow.root_budget_exhausted")).await;
    Box::pin(exhaust(&s)).await;
    let eligible: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_budget_eligible($1,$2)")
            .bind(s.request.fence.scope.company.as_uuid())
            .bind(s.request.fence.scope.run.as_uuid())
            .fetch_one(s.fixture.persistence().pool())
            .await
            .unwrap();
    assert!(!eligible);
    Box::pin(rejected_refusal(&s, "workflow.root_budget_exhausted")).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_historical_audit_without_current_retirement() {
    let s = Box::pin(scheduled()).await;
    Box::pin(exhaust(&s)).await;
    let mut history = s.fixture.persistence().pool().begin().await.unwrap();
    let sequence = audit(
        &mut history,
        s.request.fence.scope,
        "workflow.root_budget_exhausted",
    )
    .await;
    history.commit().await.unwrap();
    let before = all_tables(&s.fixture).await;
    let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
    let error = refusal(&mut tx, &s, sequence).await.unwrap_err();
    guard(
        &error,
        "invalid current workflow reconciliation budget refusal",
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "historical event supplies no current retirement"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_consumed_episode_refusal() {
    let s = Box::pin(scheduled()).await;
    let claim = s
        .fixture
        .persistence()
        .claim_io(s.request.fence.scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.fence.attempt.0, s.request.fence.attempt.0 + 1);
    Box::pin(exhaust(&s)).await;
    let episode: Option<String> =
        sqlx::query_scalar("SELECT workflow_action_pending_claim_episode($1,$2,$3,$4)")
            .bind(s.request.fence.scope.company.as_uuid())
            .bind(s.request.fence.scope.run.as_uuid())
            .bind(s.request.fence.scope.execution.as_uuid())
            .bind(s.request.fence.scope.job.0)
            .fetch_one(s.fixture.persistence().pool())
            .await
            .unwrap();
    assert!(episode.is_none());
    Box::pin(rejected_refusal(&s, "workflow.root_budget_exhausted")).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_wrong_kind_with_current_retirement() {
    let s = Box::pin(scheduled()).await;
    Box::pin(exhaust(&s)).await;
    let before = all_tables(&s.fixture).await;
    let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
    let genuine = current_retirement(&mut tx, &s).await;
    let wrong = audit(
        &mut tx,
        s.request.fence.scope,
        "workflow.not_budget_exhausted",
    )
    .await;
    assert_ne!(genuine, wrong);
    let error = refusal(&mut tx, &s, wrong).await.unwrap_err();
    guard(
        &error,
        "invalid current workflow reconciliation budget refusal",
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(&s.fixture).await,
        "current retirement/audits/refusal all roll back"
    );
}

const AUDIT_MUTATIONS: &[&str] = &[
    "UPDATE workflow_run_events SET event_kind='replacement' WHERE company_id=$1 AND run_id=$2 AND sequence=$3",
    "UPDATE workflow_run_events SET execution_id=NULL WHERE company_id=$1 AND run_id=$2 AND sequence=$3",
    "UPDATE workflow_run_events SET sequence=sequence+1000 WHERE company_id=$1 AND run_id=$2 AND sequence=$3",
    "UPDATE workflow_run_events SET company_id=gen_random_uuid() WHERE company_id=$1 AND run_id=$2 AND sequence=$3",
    "UPDATE workflow_run_events SET run_id=gen_random_uuid() WHERE company_id=$1 AND run_id=$2 AND sequence=$3",
    "UPDATE workflow_run_events SET actor_id=NULL WHERE company_id=$1 AND run_id=$2 AND sequence=$3",
    "DELETE FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND sequence=$3",
    // Replacement through the event's exact existing primary key must also hit
    // the production UPDATE guard; the INSERT's references are all valid.
    "INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) SELECT company_id,run_id,sequence,'replacement',actor_id,execution_id FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND sequence=$3 ON CONFLICT(company_id,run_id,sequence) DO UPDATE SET event_kind=EXCLUDED.event_kind",
];

async fn mutate_audit(tx: &mut Transaction<'_, Postgres>, s: &Scheduled, sequence: i64, sql: &str) {
    let error = sqlx::query(sql)
        .bind(s.request.fence.scope.company.as_uuid())
        .bind(s.request.fence.scope.run.as_uuid())
        .bind(sequence)
        .execute(&mut **tx)
        .await
        .unwrap_err();
    guard(
        &error,
        "workflow claim budget retirement audit is immutable",
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_same_transaction_audit_protection() {
    let s = Box::pin(scheduled()).await;
    Box::pin(exhaust(&s)).await;
    let before = all_tables(&s.fixture).await;
    for sql in AUDIT_MUTATIONS {
        let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
        let sequence = current_retirement(&mut tx, &s).await;
        refusal(&mut tx, &s, sequence).await.unwrap();
        // Explicit positive control establishes validity before the attempted
        // substitution; no preceding FK/deferred failure can mask the guard.
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(&mut *tx)
            .await
            .unwrap();
        mutate_audit(&mut tx, &s, sequence, sql).await;
        tx.rollback().await.unwrap();
        assert_eq!(
            before,
            all_tables(&s.fixture).await,
            "same transaction rollback: {sql}"
        );
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_sql_actual_refusal_later_audit_protection() {
    let s = Box::pin(scheduled()).await;
    Box::pin(exhaust(&s)).await;
    let before = all_tables(&s.fixture).await;
    assert!(
        s.fixture
            .persistence()
            .claim_io(s.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let saved = all_tables(&s.fixture).await;
    assert_refusal(&before, &saved, &s.request, &s.command);
    assert_eq!(before["task_attempts"], saved["task_attempts"]);
    assert_eq!(
        before["workflow_root_budget_usage"],
        saved["workflow_root_budget_usage"]
    );
    let sequence = saved["workflow_action_claim_budget_refusals"][0]["audit_sequence"]
        .as_i64()
        .unwrap();
    for sql in AUDIT_MUTATIONS {
        let mut tx = s.fixture.persistence().pool().begin().await.unwrap();
        mutate_audit(&mut tx, &s, sequence, sql).await;
        tx.rollback().await.unwrap();
        assert_eq!(saved, all_tables(&s.fixture).await, "later rollback: {sql}");
    }
}
