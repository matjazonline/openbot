//! Receipt source checks use real provider histories and roll back every public row.
use super::*;

#[path = "action_reconciliation_receipt_company_tests.rs"]
mod company_tests;

struct ReceiptHistory {
    fixture: AdmissionFixture,
    requests: Vec<ActionDispatchRequest>,
    markers: Vec<Uuid>,
    entries: Vec<Uuid>,
    evidence: Vec<Uuid>,
    local: ActionDispatchRequest,
}

async fn history() -> ReceiptHistory {
    // Box the genuine admission/provider seams to retain stock 2 MiB test stacks.
    let truths = [
        SiblingTruth::AppliedNoResult,
        SiblingTruth::AppliedNoResult,
        SiblingTruth::Unknown,
        SiblingTruth::Final,
    ];
    let (fixture, request) = Box::pin(setup()).await;
    let verifier = ledger(&fixture, &request).await;
    let mut requests = vec![request];
    for number in 1..truths.len() {
        requests.push(sibling(&fixture, &requests[0], number + 10).await);
    }
    // Prepare this legitimate invocation while dispatch authority is still live.
    let local = sibling(&fixture, &requests[0], 999).await;
    Box::pin(provider_histories(&fixture, &requests, &truths)).await;
    let mut markers = Vec::new();
    let mut entries = Vec::new();
    let mut evidence = Vec::new();
    for (number, request) in requests.iter().enumerate() {
        let marker = scoped_marker(&fixture, request).await;
        let result = Box::pin(reconcile(
            &fixture,
            request,
            marker,
            verifier.clone(),
            &format!("receipt-sql-{number}"),
        ))
        .await;
        assert!(!matches!(
            result.outcome,
            ReconciliationOutcome::Scheduled { .. }
        ));
        markers.push(marker);
        entries.push(sqlx::query_scalar("SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2")
            .bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid())
            .fetch_one(fixture.persistence().pool()).await.unwrap());
        evidence.push(
            sqlx::query_scalar(
                "SELECT id FROM workflow_action_evidence WHERE company_id=$1 AND invocation_id=$2",
            )
            .bind(request.scope().company.as_uuid())
            .bind(request.subject.invocation.as_uuid())
            .fetch_one(fixture.persistence().pool())
            .await
            .unwrap(),
        );
    }
    assert_eq!(
        effects(&fixture).await,
        2,
        "both Applied identities have real effects"
    );
    assert_eq!(
        counts(&fixture).await.0,
        0,
        "missing output leaves receipt slots empty"
    );
    ReceiptHistory {
        fixture,
        requests,
        markers,
        entries,
        evidence,
        local,
    }
}

async fn provider_histories(
    fixture: &AdmissionFixture,
    requests: &[ActionDispatchRequest],
    truths: &[SiblingTruth],
) {
    for (request, truth) in requests.iter().zip(truths) {
        let delivery = match truth {
            SiblingTruth::AppliedNoResult => Delivery::Apply {
                result: good(),
                recover: false,
                lose: true,
            },
            _ => Delivery::Pending,
        };
        assert!(
            Box::pin(invoke(
                fixture,
                request,
                &LedgerProvider::new(fixture, delivery)
            ))
            .await
            .is_err()
        );
        if matches!(truth, SiblingTruth::Final) {
            barrier(fixture, request).await;
        }
    }
}

async fn receipt(
    tx: &mut Transaction<'_, Postgres>,
    marker: Uuid,
    patch: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO workflow_action_receipts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,remote_entry_id,reconciliation_evidence_id,result)
         SELECT company_id,run_id,execution_id,invocation_id,
            COALESCE($2->>'argument_digest',argument_digest),
            COALESCE(($2->>'dispatch_id')::uuid,id),
            COALESCE($2->>'effect_kind',effect_kind),
            ($2->>'remote_entry_id')::uuid,($2->>'reconciliation_evidence_id')::uuid,$3
         FROM workflow_action_dispatches WHERE id=$1",
    )
    .bind(marker)
    .bind(patch)
    .bind(good())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn positive(tx: &mut Transaction<'_, Postgres>, marker: Uuid, source: Value) {
    sqlx::query("SAVEPOINT valid_receipt")
        .execute(&mut **tx)
        .await
        .unwrap();
    receipt(tx, marker, source).await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut **tx)
        .await
        .unwrap();
    sqlx::query("ROLLBACK TO SAVEPOINT valid_receipt")
        .execute(&mut **tx)
        .await
        .unwrap();
}

fn constraint(error: &sqlx::Error, code: &str, name: &str) {
    let database = error.as_database_error().unwrap();
    assert_eq!(database.code().as_deref(), Some(code), "{database}");
    assert_eq!(database.constraint(), Some(name), "{database}");
}

async fn rejected(h: &ReceiptHistory, source: Value, code: &str, name: &str) {
    let before = all_tables(&h.fixture).await;
    let mut tx = h.fixture.persistence().pool().begin().await.unwrap();
    positive(
        &mut tx,
        h.markers[0],
        json!({"remote_entry_id":h.entries[0]}),
    )
    .await;
    positive(
        &mut tx,
        h.markers[0],
        json!({"reconciliation_evidence_id":h.evidence[0]}),
    )
    .await;
    let error = receipt(&mut tx, h.markers[0], source).await.unwrap_err();
    constraint(&error, code, name);
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(&h.fixture).await,
        "all public rows survive {name}"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_receipt_sql_remote_source_xor() {
    let h = Box::pin(history()).await;
    for source in [
        json!({}),
        json!({"remote_entry_id":h.entries[0],"reconciliation_evidence_id":h.evidence[0]}),
    ] {
        rejected(&h, source, "23514", "workflow_action_receipt_kind").await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_receipt_sql_local_has_neither_source() {
    let h = Box::pin(history()).await;
    let before = all_tables(&h.fixture).await;
    for source in [
        json!({"remote_entry_id":h.entries[0]}),
        json!({"reconciliation_evidence_id":h.evidence[0]}),
        json!({"remote_entry_id":h.entries[0],"reconciliation_evidence_id":h.evidence[0]}),
    ] {
        let mut tx = h.fixture.persistence().pool().begin().await.unwrap();
        let marker = Uuid::new_v4();
        Dispatch::insert(&mut tx, &h.local, marker, "local", json!({}))
            .await
            .unwrap();
        positive(&mut tx, marker, json!({})).await;
        let error = receipt(&mut tx, marker, source).await.unwrap_err();
        constraint(&error, "23514", "workflow_action_receipt_kind");
        tx.rollback().await.unwrap();
        assert_eq!(
            before,
            all_tables(&h.fixture).await,
            "local source attack rolls back marker and all public rows"
        );
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_receipt_sql_authentic_foreign_bindings() {
    let h = Box::pin(history()).await;
    assert_ne!(
        h.requests[0].subject.invocation,
        h.requests[1].subject.invocation
    );
    assert_ne!(
        h.requests[0].subject.argument_digest,
        h.requests[1].subject.argument_digest
    );
    rejected(
        &h,
        json!({"reconciliation_evidence_id":h.evidence[1]}),
        "23503",
        "workflow_action_receipt_evidence_source",
    )
    .await;
    rejected(
        &h,
        json!({"remote_entry_id":h.entries[1]}),
        "23503",
        "workflow_action_receipt_remote_entry",
    )
    .await;
    for patch in [
        json!({"argument_digest":h.requests[1].subject.argument_digest.as_str(),"reconciliation_evidence_id":h.evidence[0]}),
        json!({"dispatch_id":h.markers[1],"reconciliation_evidence_id":h.evidence[0]}),
    ] {
        rejected(
            &h,
            patch,
            "23503",
            "workflow_action_receipt_evidence_source",
        )
        .await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_receipt_sql_non_applied_evidence() {
    let h = Box::pin(history()).await;
    for number in [2, 3] {
        let before = all_tables(&h.fixture).await;
        let mut tx = h.fixture.persistence().pool().begin().await.unwrap();
        positive(
            &mut tx,
            h.markers[number],
            json!({"remote_entry_id":h.entries[number]}),
        )
        .await;
        let error = receipt(
            &mut tx,
            h.markers[number],
            json!({"reconciliation_evidence_id":h.evidence[number]}),
        )
        .await
        .unwrap_err();
        let database = error.as_database_error().unwrap();
        assert_eq!(database.code().as_deref(), Some("23514"));
        assert_eq!(
            database.message(),
            "workflow action receipt requires verified applied evidence"
        );
        tx.rollback().await.unwrap();
        assert_eq!(
            before,
            all_tables(&h.fixture).await,
            "nonApplied receipt rolls back every public row"
        );
    }
}
