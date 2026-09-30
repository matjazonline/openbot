//! Company provenance is rejected by the public BEFORE guard, separately from sibling FKs.
use super::*;

#[path = "action_reconciliation_command_probe_tests.rs"]
mod command_probe_tests;

struct CompanyReceipt {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    marker: Uuid,
    evidence: Uuid,
    entry: Uuid,
}

async fn foreign_action(f: &AdmissionFixture) -> (AdmissionFixture, ActionDispatchRequest) {
    let fixture = Fixture::in_database(
        f.binding.fixture._db.clone(),
        &format!("-receipt-company-{}", Uuid::new_v4().simple()),
    )
    .await;
    let source = serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    let foreign = Box::pin(AdmissionFixture::with_binding(
        BindingFixture::from_fixture(fixture, source).await,
    ))
    .await;
    let command = foreign
        .prepare(foreign.request("foreign-receipt", foreign.manual()))
        .await;
    foreign.persistence().admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(foreign.persistence().pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    let claim = foreign
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    Box::pin(initialize_action_on_claim(
        foreign,
        claim,
        publication::ActionRecovery::Reconcile,
        json!({"value":2}),
    ))
    .await
}

async fn applied_history(
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
) -> CompanyReceipt {
    let provider = LedgerProvider::new(
        &fixture,
        Delivery::Apply {
            result: good(),
            recover: false,
            lose: true,
        },
    );
    assert!(
        Box::pin(invoke(&fixture, &request, &provider))
            .await
            .is_err()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let marker = scoped_marker(&fixture, &request).await;
    let result = Box::pin(reconcile(
        &fixture,
        &request,
        marker,
        verifier,
        "company-applied-missing",
    ))
    .await;
    assert!(!matches!(
        result.outcome,
        ReconciliationOutcome::Scheduled { .. }
    ));
    let evidence = sqlx::query_scalar("SELECT id FROM workflow_action_evidence WHERE company_id=$1 AND invocation_id=$2 AND disposition='applied' AND registration IS NOT NULL")
        .bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid())
        .fetch_one(fixture.persistence().pool()).await.unwrap();
    let entry = sqlx::query_scalar(
        "SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(fixture.persistence().pool())
    .await
    .unwrap();
    let authentic: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
         JOIN workflow_action_dispatches AS marker ON
            (marker.company_id,marker.run_id,marker.execution_id,marker.invocation_id,marker.argument_digest,marker.id)=
            (evidence.company_id,evidence.run_id,evidence.execution_id,evidence.invocation_id,evidence.argument_digest,evidence.dispatch_id)
         JOIN workflow_action_remote_entries AS entry ON
            (entry.company_id,entry.run_id,entry.execution_id,entry.invocation_id,entry.argument_digest,entry.dispatch_id)=
            (marker.company_id,marker.run_id,marker.execution_id,marker.invocation_id,marker.argument_digest,marker.id)
         JOIN fixture_evidence_ledger AS ledger ON ledger.company_id=entry.company_id
            AND ledger.invocation_id=entry.invocation_id AND ledger.entry_id=entry.id
         JOIN fixture_provider_effects AS effect ON effect.entry_id=entry.id AND effect.result=ledger.result
         WHERE evidence.company_id=$1 AND evidence.id=$2 AND marker.id=$3 AND entry.id=$4
            AND evidence.applied_request='remote_entry' AND evidence.applied_remote_entry_id=entry.id
            AND NOT ledger.recover_result)",
    ).bind(request.scope().company.as_uuid()).bind(evidence).bind(marker).bind(entry)
        .fetch_one(fixture.persistence().pool()).await.unwrap();
    assert!(
        authentic,
        "Applied source is linked to its own real provider effect"
    );
    CompanyReceipt {
        fixture,
        request,
        marker,
        evidence,
        entry,
    }
}

async fn company_positive(tx: &mut Transaction<'_, Postgres>, history: &CompanyReceipt) {
    #[derive(Debug, PartialEq, sqlx::FromRow)]
    struct ReceiptSource {
        company_id: Uuid,
        reconciliation_evidence_id: Uuid,
    }

    sqlx::query("SAVEPOINT company_receipt_positive")
        .execute(&mut **tx)
        .await
        .unwrap();
    receipt(
        tx,
        history.marker,
        json!({"reconciliation_evidence_id":history.evidence}),
    )
    .await
    .unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut **tx)
        .await
        .unwrap();
    let rows: Vec<ReceiptSource> = sqlx::query_as(
        "SELECT company_id,reconciliation_evidence_id FROM workflow_action_receipts
         WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(history.request.scope().company.as_uuid())
    .bind(history.request.subject.invocation.as_uuid())
    .fetch_all(&mut **tx)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![ReceiptSource {
            company_id: history.request.scope().company.as_uuid(),
            reconciliation_evidence_id: history.evidence,
        }]
    );
    sqlx::query("ROLLBACK TO SAVEPOINT company_receipt_positive")
        .execute(&mut **tx)
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_action_reconciliation_receipt_sql_authentic_foreign_company_guard() {
    // Box genuine owner/provider setup so the combined fixture stays within stock 2 MiB.
    let (fixture, request) = Box::pin(setup()).await;
    let verifier = ledger(&fixture, &request).await;
    let (foreign, foreign_request) = Box::pin(foreign_action(&fixture)).await;
    let foreign_verifier = register_ledger(&foreign, &foreign_request).await;
    let a = Box::pin(applied_history(fixture, request, verifier)).await;
    let b = Box::pin(applied_history(foreign, foreign_request, foreign_verifier)).await;
    assert_ne!(a.request.scope().company, b.request.scope().company);
    assert_ne!(a.request.subject.invocation, b.request.subject.invocation);
    assert_ne!(a.marker, b.marker);
    assert_ne!(a.evidence, b.evidence);
    assert_ne!(a.entry, b.entry);
    let db_a: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(a.fixture.persistence().pool())
        .await
        .unwrap();
    let db_b: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(b.fixture.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        db_a, db_b,
        "both companies share one physical fixture database"
    );
    assert_eq!(effects(&a.fixture).await, 2);
    assert_eq!(
        counts(&a.fixture).await.0,
        0,
        "both canonical receipt slots are empty"
    );
    for (subject, source) in [(&a, &b), (&b, &a)] {
        let before = all_tables(&a.fixture).await;
        let mut tx = a.fixture.persistence().pool().begin().await.unwrap();
        company_positive(&mut tx, &a).await;
        company_positive(&mut tx, &b).await;
        let error = receipt(
            &mut tx,
            subject.marker,
            json!({"reconciliation_evidence_id":source.evidence}),
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
            all_tables(&a.fixture).await,
            "foreign source guard preserves both companies and real effects"
        );
    }
}
