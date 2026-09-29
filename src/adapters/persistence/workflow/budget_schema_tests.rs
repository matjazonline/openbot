//! Schema accounting tests deliberately use SQL; these facts grant no dispatch permission.
use super::*;

async fn limited() -> (AdmissionFixture, ActivationRequest) {
    let mut source = example("data.map");
    source["limits"]["root_budget"] = json!({"activations": 2, "model_calls": 2, "repetitions": 2});
    fixture(source).await
}

async fn receipt(
    db: &mut PgConnection,
    scope: ActivationRequest,
    resource: &str,
    key: &str,
    quantity: i32,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) SELECT $1,$2,$3,root_run_id,$4,$5,$6,'granted' FROM workflow_run_budgets WHERE company_id=$1 AND run_id=$2 ON CONFLICT DO NOTHING RETURNING disposition")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(resource).bind(key).bind(quantity).fetch_optional(db).await
}

async fn accounting(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('identities',(SELECT jsonb_agg(to_jsonb(budget) ORDER BY root_run_id) FROM workflow_root_budgets AS budget),'usage',(SELECT jsonb_agg(to_jsonb(usage) ORDER BY root_run_id) FROM workflow_root_budget_usage AS usage),'receipts',(SELECT jsonb_agg(to_jsonb(receipt) ORDER BY run_id,execution_id,resource,reservation_key) FROM workflow_budget_receipts AS receipt))")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_budget_schema_duplicate_first_claim_and_replay_with_remaining_allowance() {
    let (f, root) = limited().await;
    let barrier = tokio::sync::Barrier::new(2);
    let reserve = || async {
        let mut db = f.persistence().pool().acquire().await.unwrap();
        barrier.wait().await;
        receipt(&mut db, root, "model_call", "op", 1).await.unwrap()
    };
    let (a, b) = tokio::join!(reserve(), reserve());
    assert_ne!(a.is_some(), b.is_some());
    let before = accounting(&f).await;
    assert_eq!(before["usage"][0]["model_calls"], 1);
    assert_eq!(before["receipts"].as_array().unwrap().len(), 1);
    let mut db = f.persistence().pool().acquire().await.unwrap();
    assert!(
        receipt(&mut db, root, "model_call", "op", 1)
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) VALUES($1,$2,$3,$2,'model_call','op',1,'granted') ON CONFLICT(company_id,run_id,execution_id,resource,reservation_key) DO UPDATE SET quantity=EXCLUDED.quantity")
        .bind(root.company.as_uuid()).bind(root.run.as_uuid()).bind(root.execution.as_uuid()).execute(&mut *db).await.unwrap();
    assert!(receipt(&mut db, root, "model_call", "op", 2).await.is_err());
    assert_eq!(accounting(&f).await, before);
}

#[tokio::test]
async fn workflow_budget_schema_competing_descendants_and_exact_duplicate_receipts() {
    let (f, root) = limited().await;
    let a = admit_child(&f, root, "start", "a").await;
    let b = admit_child(&f, a, "start", "b").await;
    for resource in ["activation", "model_call", "repetition"] {
        let key = if resource == "activation" {
            "activation"
        } else {
            "op"
        };
        let barrier = tokio::sync::Barrier::new(3);
        let reserve = |scope| {
            let f = &f;
            let barrier = &barrier;
            async move {
                let mut db = f.persistence().pool().acquire().await.unwrap();
                barrier.wait().await;
                receipt(&mut db, scope, resource, key, 1)
                    .await
                    .unwrap()
                    .unwrap()
            }
        };
        let (first, second, third) = tokio::join!(reserve(root), reserve(a), reserve(b));
        let dispositions = [first, second, third];
        assert_eq!(dispositions.iter().filter(|v| *v == "granted").count(), 2);
        assert_eq!(dispositions.iter().filter(|v| *v == "exhausted").count(), 1);
    }
    let before = accounting(&f).await;
    let barrier = tokio::sync::Barrier::new(2);
    let replay = || async {
        let mut db = f.persistence().pool().acquire().await.unwrap();
        barrier.wait().await;
        receipt(&mut db, a, "model_call", "op", 1).await.unwrap()
    };
    let (first, second) = tokio::join!(replay(), replay());
    assert!(first.is_none() && second.is_none());
    let mut db = f.persistence().pool().acquire().await.unwrap();
    assert!(receipt(&mut db, a, "model_call", "op", 2).await.is_err());
    assert_eq!(accounting(&f).await, before);
    for name in ["activations", "model_calls", "repetitions"] {
        assert_eq!(before["usage"][0][name], 2);
        assert_eq!(before["identities"][0][name], 2);
    }
}

#[tokio::test]
async fn workflow_budget_schema_rollback_scope_mutation_and_owner_cascade() {
    let (f, root) = limited().await;
    let child = admit_child(&f, root, "start", "child").await;
    let before = accounting(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(
        receipt(&mut tx, child, "model_call", "abort", 2)
            .await
            .unwrap()
            .as_deref(),
        Some("granted")
    );
    tx.rollback().await.unwrap();
    assert_eq!(accounting(&f).await, before);
    let mut db = f.persistence().pool().acquire().await.unwrap();
    receipt(&mut db, child, "model_call", "saved", 2)
        .await
        .unwrap();
    for query in [
        "UPDATE workflow_root_budgets SET model_calls=3 WHERE root_run_id=$1",
        "DELETE FROM workflow_root_budgets WHERE root_run_id=$1",
        "UPDATE workflow_root_budget_usage SET model_calls=0 WHERE root_run_id=$1",
        "UPDATE workflow_root_budget_usage SET repetitions=1 WHERE root_run_id=$1",
        "DELETE FROM workflow_root_budget_usage WHERE root_run_id=$1",
        "DELETE FROM workflow_run_budgets WHERE root_run_id=$1",
        "UPDATE workflow_run_budgets SET parent_run_id=NULL,root_run_id=run_id WHERE root_run_id=$1",
        "UPDATE workflow_budget_receipts SET quantity=1 WHERE root_run_id=$1",
        "DELETE FROM workflow_budget_receipts WHERE root_run_id=$1",
    ] {
        assert!(
            sqlx::query(query)
                .bind(root.run.as_uuid())
                .execute(&mut *db)
                .await
                .is_err(),
            "{query}"
        );
    }
    for bad in [0, -1, 1001] {
        assert!(
            receipt(&mut db, child, "model_call", "invalid", bad)
                .await
                .is_err()
        );
    }
    let foreign_execution = ActivationRequest {
        execution: root.execution,
        ..child
    };
    assert!(
        receipt(&mut db, foreign_execution, "repetition", "scope", 1)
            .await
            .is_err()
    );
    let other = f.prepare(f.request("other-root", f.manual())).await;
    f.persistence().admit(&other).await.unwrap();
    let before = accounting(&f).await;
    assert!(sqlx::query("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) VALUES($1,$2,$3,$4,'repetition','foreign',1,'granted')")
        .bind(Uuid::new_v4()).bind(child.run.as_uuid()).bind(child.execution.as_uuid()).bind(root.run.as_uuid()).execute(&mut *db).await.is_err());
    assert!(sqlx::query("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) VALUES($1,$2,$3,$4,'repetition','wrong-root',1,'granted')")
        .bind(child.company.as_uuid()).bind(child.run.as_uuid()).bind(child.execution.as_uuid()).bind(other.proposed_run_id().as_uuid()).execute(&mut *db).await.is_err());
    assert_eq!(accounting(&f).await, before);
    // A company cascade removes owners and their immutable facts together.
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(root.company.as_uuid())
        .execute(&mut *db)
        .await
        .unwrap();
    assert_eq!(
        accounting(&f).await,
        json!({"identities":null,"usage":null,"receipts":null})
    );
}

#[tokio::test]
async fn workflow_budget_schema_consumption_never_locks_root_run_or_execution() {
    let (f, root) = limited().await;
    let child = admit_child(&f, root, "start", "child").await;
    let mut held = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(root.run.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE id=$1 FOR UPDATE")
        .bind(root.execution.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        for key in ["one", "two"] {
            assert_eq!(
                receipt(&mut tx, child, "model_call", key, 1)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("granted")
            );
        }
        tx.commit().await.unwrap();
    })
    .await
    .unwrap();
    held.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_budget_schema_retained_migration_preserves_exact_activations_and_seals() {
    let (f, root) = limited().await;
    let child = admit_child(&f, root, "start", "child").await;
    f.persistence().activate(root).await.unwrap();
    f.persistence().activate(child).await.unwrap();
    // Recreate the pre-budget boundary only inside this isolated, rollback-only
    // transaction. No retained database or migration checksum is changed.
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::raw_sql("DROP TABLE workflow_budget_receipts,workflow_root_budget_usage,workflow_run_budgets,workflow_root_budgets CASCADE; DROP FUNCTION insert_workflow_budget_link(),preserve_workflow_budget_identity(),preserve_workflow_budget_receipt(),debit_workflow_budget_receipt(),preserve_workflow_budget_usage() CASCADE; ALTER TABLE workflow_runs DROP CONSTRAINT workflow_run_budget_parent_identity;")
        .execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20260929070000_workflow_root_budgets.sql"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    let saved: Value = sqlx::query_scalar("SELECT jsonb_build_object('provenance',budget.provenance,'activations',usage.activations,'model_calls',usage.model_calls,'repetitions',usage.repetitions,'receipts',(SELECT count(*) FROM workflow_budget_receipts)) FROM workflow_root_budgets AS budget JOIN workflow_root_budget_usage AS usage USING(company_id,root_run_id)")
        .fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        saved,
        json!({"provenance":"sealed_legacy","activations":2,"model_calls":0,"repetitions":0,"receipts":2})
    );
    assert_eq!(
        receipt(&mut tx, child, "model_call", "no-new-allowance", 1)
            .await
            .unwrap()
            .as_deref(),
        Some("exhausted")
    );
    tx.rollback().await.unwrap();
}
