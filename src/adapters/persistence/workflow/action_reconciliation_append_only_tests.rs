//! Real proof/consumption history rejects changes while company deletion retains its owner contract.
use super::*;

struct Fact {
    table: &'static str,
    columns: &'static str,
    key: &'static str,
    change: &'static str,
    unchanged: &'static str,
}

const FACTS: &[Fact] = &[
    Fact {
        table: "workflow_action_evidence",
        columns: "company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,actor_id,command_id,command_key,request_digest,coverage_digest,disposition,registration,verifier_version,provider,authoritative_reference,operation_signature,observed_at,verified_at,valid_until,created_at,grant_eligible,diagnostic,applied_request,applied_remote_entry_id",
        key: "company_id,id",
        change: "diagnostic=diagnostic || '{\"attempted_change\":true}'::jsonb",
        unchanged: "diagnostic=diagnostic",
    },
    Fact {
        table: "workflow_action_evidence_coverage",
        columns: "company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id",
        key: "company_id,evidence_id,remote_entry_id",
        change: "remote_entry_id=$2",
        unchanged: "remote_entry_id=remote_entry_id",
    },
    Fact {
        table: "workflow_action_evidence_consumptions",
        // Omit reservation_xid: the native INSERT identity trigger owns a fresh
        // current xid, so an earlier spoof refusal cannot mask the UPDATE guard.
        columns: "company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,created_at",
        key: "company_id,evidence_id",
        change: "remote_entry_id=$2",
        unchanged: "remote_entry_id=remote_entry_id",
    },
    Fact {
        table: "workflow_action_evidence_commands",
        columns: "company_id,command_key,id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,actor_id,request_digest,expected_revision,result_revision,outcome,evidence_id,audit_sequence,scheduled_job_id,previous_state,previous_waiting_reason,created_at",
        key: "company_id,command_key",
        change: "result_revision=result_revision+1",
        unchanged: "result_revision=result_revision",
    },
];

enum ConflictAction {
    Update,
    Ignore,
}

fn upsert(fact: &Fact, action: ConflictAction) -> String {
    let conflict = match action {
        // A self-assignment is still an UPDATE, not immutable replay.
        ConflictAction::Update => "DO UPDATE SET company_id=EXCLUDED.company_id",
        ConflictAction::Ignore => "DO NOTHING",
    };
    format!(
        "INSERT INTO {} ({}) SELECT {} FROM {} WHERE company_id=$1 ON CONFLICT ({}) {conflict}",
        fact.table, fact.columns, fact.columns, fact.table, fact.key
    )
}

async fn reject(f: &AdmissionFixture, sql: &str, company: Uuid, alternate_entry: Option<Uuid>) {
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let query = sqlx::query(sql).bind(company);
    let query = if let Some(entry) = alternate_entry {
        query.bind(entry)
    } else {
        query
    };
    let error = query.execute(&mut *tx).await.unwrap_err();
    let db_error = error.as_database_error().unwrap();
    assert_eq!(db_error.code().as_deref(), Some("P0001"), "{sql}");
    assert_eq!(db_error.constraint(), None, "{sql}");
    assert_eq!(
        db_error.message(),
        "workflow action effect fact is append-only",
        "{sql}"
    );
    eprintln!("append-only negative PASS: {sql}; P0001 native immutable guard");
    tx.rollback().await.unwrap();
    assert_eq!(all_tables(f).await, before, "all public rows after {sql}");
}

async fn immutable_matrix(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let company = request.scope().company.as_uuid();
    let before = all_tables(f).await;
    let coverage = &before["workflow_action_evidence_coverage"][0];
    let consumption = &before["workflow_action_evidence_consumptions"][0];
    assert_ne!(coverage["remote_entry_id"], consumption["remote_entry_id"]);
    assert_eq!(coverage["evidence_id"], consumption["evidence_id"]);
    for fact in FACTS {
        assert_eq!(before[fact.table].as_array().unwrap().len(), 1);
        let alternate_entry = match fact.table {
            "workflow_action_evidence_coverage" => Some(consumption["remote_entry_id"].clone()),
            "workflow_action_evidence_consumptions" => Some(coverage["remote_entry_id"].clone()),
            _ => None,
        }
        .map(|value| serde_json::from_value::<Uuid>(value).unwrap());
        let mutation = format!(
            "UPDATE {} SET {} WHERE company_id=$1",
            fact.table, fact.change
        );
        reject(f, &mutation, company, alternate_entry).await;
        for sql in [
            format!(
                "UPDATE {} SET {} WHERE company_id=$1",
                fact.table, fact.unchanged
            ),
            format!("DELETE FROM {} WHERE company_id=$1", fact.table),
            upsert(fact, ConflictAction::Update),
        ] {
            reject(f, &sql, company, None).await;
        }
        // Matching INSERT candidate, scopes, keys and native BEFORE INSERT guards
        // all pass. Only the conflict action differs from the attack above.
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let sql = upsert(fact, ConflictAction::Ignore);
        assert_eq!(
            sqlx::query(&sql)
                .bind(company)
                .execute(&mut *tx)
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        tx.commit().await.unwrap();
        assert_eq!(
            all_tables(f).await,
            before,
            "immutable conflict replay: {sql}"
        );
        eprintln!("append-only matching control PASS: {sql}");
    }
}

async fn owner_delete(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let before = all_tables(f).await;
    for fact in FACTS {
        assert!(!before[fact.table].as_array().unwrap().is_empty());
    }
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert_eq!(
        sqlx::query("DELETE FROM companies WHERE id=$1")
            .bind(request.scope().company.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    tx.commit().await.unwrap();
    let after = all_tables(f).await;
    for (table, rows) in after.as_object().unwrap() {
        if table.starts_with("workflow_")
            && before[table]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row.get("company_id").is_some())
            || ["companies", "background_tasks", "task_attempts"].contains(&table.as_str())
        {
            assert_eq!(rows, &json!([]), "owner cascade removes {table}");
        }
        if table == "workflow_capacity_policy" {
            assert_eq!(
                rows, &before[table],
                "global capacity configuration survives owner deletion"
            );
        }
        if table.starts_with("fixture_provider_") || table == "fixture_evidence_ledger" {
            assert_eq!(
                rows, &before[table],
                "external provider history survives {table}"
            );
        }
    }
    assert_eq!(effects(f).await, 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_append_only_mutation_delete_upsert_and_owner_lifecycle() {
    // Box genuine provider/service phase seams to retain stock 2 MiB test stacks.
    let (f, mut request) = Box::pin(setup()).await;
    let verifier = ledger(&f, &request).await;
    let old = LedgerProvider::new(&f, Delivery::Pending);
    assert!(Box::pin(invoke(&f, &request, &old)).await.is_err());
    assert_eq!(old.calls.load(Ordering::SeqCst), 1);
    park(&f, &request).await;
    barrier(&f, &request).await;
    let command = proof_command(&f, &request, &verifier, "append-only-final").await;
    let finality = Box::pin(run_command(&f, &command, verifier.clone()))
        .await
        .unwrap();
    assert_eq!(
        finality.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    new_claim(&f, &mut request).await;
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reserved) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("genuine final proof reserves its one consuming entry")
    };
    assert_eq!(entry_consumptions(&f).await, (2, 1));
    Box::pin(immutable_matrix(&f, &request)).await;
    // Rejected changes leave the actual reserved entry usable by its real owner.
    let entered = writer.enter_remote(*reserved).await.unwrap();
    let applied = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    assert!(
        Box::pin(applied.invoke(&entered.action, &entered.provider))
            .await
            .is_err()
    );
    assert_eq!(applied.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&f).await, 1);
    park(&f, &request).await;
    let recovered = Box::pin(reconcile(
        &f,
        &request,
        marker(&f).await,
        verifier,
        "append-only-applied",
    ))
    .await;
    assert_eq!(
        recovered.outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    );
    assert_eq!(projection(&f).await, ("committed".into(), false));
    assert_eq!(entry_consumptions(&f).await, (2, 1));
    Box::pin(owner_delete(&f, &request)).await;
    f.persistence().pool().close().await;
}
