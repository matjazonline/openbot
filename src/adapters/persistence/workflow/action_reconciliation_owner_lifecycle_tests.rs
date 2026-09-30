//! Owner deletion removes the populated graph, while live-owner facts stay immutable.
use super::*;

const OWNED_FACTS: &[&str] = &[
    "workflow_action_state_witnesses",
    "workflow_action_schedule_witnesses",
    "workflow_action_claim_episodes",
    "workflow_action_claim_retirement_witnesses",
    "workflow_action_claim_budget_refusals",
    "workflow_action_dispatches",
    "workflow_action_remote_entries",
    "workflow_action_reconciliations",
    "workflow_action_evidence",
    "workflow_action_evidence_coverage",
    "workflow_action_evidence_commands",
    "workflow_action_receipts",
    "workflow_action_actual_receipt_observations",
    "workflow_action_evidence_consumptions",
    "workflow_action_evidence_conflicts",
];

async fn count(f: &AdmissionFixture, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE company_id=$1"))
        .bind(f.binding.target.company.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

async fn live_run_owner_deletion_negatives(f: &AdmissionFixture) {
    let before = all_tables(f).await;
    for (table, statement) in [
        (
            "background_tasks",
            "DELETE FROM background_tasks WHERE company_id=$1",
        ),
        (
            "workflow_executions",
            "DELETE FROM workflow_executions WHERE company_id=$1",
        ),
        (
            "task_attempts",
            "DELETE FROM task_attempts WHERE task_id IN (SELECT id FROM background_tasks WHERE company_id=$1)",
        ),
    ] {
        assert!(
            !before[table].as_array().unwrap().is_empty(),
            "populated {table}"
        );
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let error = sqlx::query(statement)
            .bind(f.binding.target.company.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap_err();
        let db_error = error.as_database_error().unwrap();
        eprintln!(
            "live-run {table} DELETE: code={:?} constraint={:?} message={}",
            db_error.code(),
            db_error.constraint(),
            db_error.message()
        );
        if table == "workflow_executions" {
            // The job's original scoped FK remains a separate live-owner boundary.
            assert_eq!(db_error.code().as_deref(), Some("23503"));
            assert_eq!(
                db_error.constraint(),
                Some("background_tasks_workflow_execution_fk")
            );
        } else {
            assert_eq!(db_error.code().as_deref(), Some("P0001"));
            assert_eq!(
                db_error.message(),
                "workflow action effect fact is append-only"
            );
        }
        tx.rollback().await.unwrap();
        assert_eq!(
            before,
            all_tables(f).await,
            "failed live-run {table} deletion"
        );
    }
}

async fn live_owner_negatives(f: &AdmissionFixture) {
    let before = all_tables(f).await;
    for table in OWNED_FACTS {
        if count(f, table).await == 0 {
            continue;
        }
        for statement in [
            format!("DELETE FROM {table} WHERE company_id=$1"),
            format!("UPDATE {table} SET run_id=run_id WHERE company_id=$1"),
        ] {
            let error = sqlx::query(&statement)
                .bind(f.binding.target.company.as_uuid())
                .execute(f.persistence().pool())
                .await
                .unwrap_err();
            let message = error.as_database_error().unwrap().message();
            assert!(
                message.contains("append-only") || message.contains("database owned"),
                "{table}: {message}"
            );
            assert_eq!(
                before,
                all_tables(f).await,
                "failed direct mutation of {table}"
            );
        }
    }
    for kind in ["action_reconciled", "workflow.root_budget_exhausted"] {
        let events: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workflow_run_events WHERE company_id=$1 AND event_kind=$2",
        )
        .bind(f.binding.target.company.as_uuid())
        .bind(kind)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
        if events == 0 {
            continue;
        }
        for statement in [
            "DELETE FROM workflow_run_events WHERE company_id=$1 AND event_kind=$2",
            "UPDATE workflow_run_events SET event_kind=event_kind WHERE company_id=$1 AND event_kind=$2",
            "UPDATE workflow_run_events SET execution_id=NULL WHERE company_id=$1 AND event_kind=$2",
        ] {
            let error = sqlx::query(statement)
                .bind(f.binding.target.company.as_uuid())
                .bind(kind)
                .execute(f.persistence().pool())
                .await
                .unwrap_err();
            let message = error.as_database_error().unwrap().message();
            assert!(
                message.contains("append-only") || message.contains("audit is immutable"),
                "{kind}: {message}"
            );
            assert_eq!(before, all_tables(f).await);
        }
    }
}

async fn assert_deleted(f: &AdmissionFixture) {
    for table in OWNED_FACTS.iter().copied().chain([
        "workflow_runs",
        "workflow_executions",
        "background_tasks",
        "workflow_run_events",
        "workflow_action_intents",
        "workflow_action_model_calls",
        "workflow_control_commands",
        "workflow_root_budgets",
        "workflow_root_budget_usage",
        "workflow_run_budgets",
        "workflow_budget_receipts",
    ]) {
        assert_eq!(
            count(f, table).await,
            0,
            "owner deletion must remove {table}"
        );
    }
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(attempts, 0);
}

#[tokio::test]
async fn workflow_action_reconciliation_owner_lifecycle_scheduled_and_refused() {
    for refuse in [false, true] {
        // Box extended fixture/verification seams to retain stock 2 MiB test stacks.
        let (f, request) = Box::pin(limited()).await;
        let verifier = ledger(&f, &request).await;
        assert!(
            invoke(&f, &request, &LedgerProvider::new(&f, Delivery::Pending))
                .await
                .is_err()
        );
        park(&f, &request).await;
        barrier(&f, &request).await;
        let c = proof_command(&f, &request, &verifier, "owner-delete-schedule").await;
        assert_eq!(
            run_command(&f, &c, verifier).await.unwrap().outcome,
            ReconciliationOutcome::Scheduled {
                receipt_only: false
            }
        );
        if refuse {
            // A real sibling reservation consumes shared allowance; actual claim owns refusal.
            let descendant = child(&f, request.fence.scope).await;
            let claim = f
                .persistence()
                .claim_io(descendant, worker(), policy())
                .await
                .unwrap()
                .unwrap();
            debit(&f, claim.fence).await;
            assert!(
                f.persistence()
                    .claim_io(request.fence.scope, worker(), policy())
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        for table in [
            "workflow_action_state_witnesses",
            "workflow_action_schedule_witnesses",
            "workflow_action_claim_episodes",
        ] {
            assert!(count(&f, table).await > 0, "populated {table}");
        }
        for table in [
            "workflow_action_claim_retirement_witnesses",
            "workflow_action_claim_budget_refusals",
        ] {
            assert_eq!(
                count(&f, table).await,
                if refuse { 1 } else { 0 },
                "populated {table}"
            );
        }
        Box::pin(live_run_owner_deletion_negatives(&f)).await;
        Box::pin(live_owner_negatives(&f)).await;
        sqlx::query("DELETE FROM companies WHERE id=$1")
            .bind(request.scope().company.as_uuid())
            .execute(f.persistence().pool())
            .await
            .unwrap();
        Box::pin(assert_deleted(&f)).await;
    }
}
