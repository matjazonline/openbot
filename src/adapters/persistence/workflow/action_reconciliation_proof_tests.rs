//! Genuine provider finality races and one-use proof lifetime.
use super::*;
use tokio::sync::Barrier;

#[path = "action_reconciliation_attribution_tests.rs"]
mod attribution_tests;

#[path = "action_reconciliation_proof_clock_tests.rs"]
mod clock_tests;

#[path = "action_reconciliation_sibling_tests.rs"]
mod sibling_tests;

#[path = "action_reconciliation_snapshot_binding_tests.rs"]
mod snapshot_binding_tests;

#[path = "action_reconciliation_control_serialization_tests.rs"]
mod control_serialization_tests;

#[path = "action_reconciliation_late_truth_tests.rs"]
mod late_truth_tests;

#[path = "action_reconciliation_append_only_tests.rs"]
mod append_only_tests;

#[path = "action_reconciliation_validation_tests.rs"]
mod validation_tests;

// Catalog-owned identifiers only; one repeatable-read snapshot covers every fact,
// including reconciliation joins, witnesses and provider effects omitted by old helpers.
pub(super) async fn all_tables(f: &AdmissionFixture) -> Value {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .unwrap();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename::text FROM pg_tables WHERE schemaname='public' ORDER BY tablename",
    )
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    let mut state = serde_json::Map::new();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let rows: Value = sqlx::query_scalar(&format!("SELECT COALESCE(jsonb_agg(to_jsonb(record) ORDER BY to_jsonb(record)::text),'[]'::jsonb) FROM public.\"{quoted}\" AS record"))
            .fetch_one(&mut *tx).await.unwrap();
        state.insert(table, rows);
    }
    tx.commit().await.unwrap();
    Value::Object(state)
}

async fn proof_command(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    verifier: &LedgerVerifier,
    key: &str,
) -> ReconcileActionCommand {
    let mut c = command(f, request, marker(f).await).await;
    c.command_key = IdempotencyKey::parse(key).unwrap();
    c.input = EvidenceInput::VerifiedReference {
        registration: verifier.registration().id().clone(),
        reference: EvidenceRecordReference::parse(key).unwrap(),
    };
    c
}

struct RendezvousVerifier {
    ledger: Arc<LedgerVerifier>,
    ready: Barrier,
}
#[async_trait]
impl ActionEvidenceVerifier for RendezvousVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.ledger.registration()
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        let result = self
            .ledger
            .verify(snapshot, reference, cancellation)
            .await?;
        self.ready.wait().await;
        Ok(result)
    }
}

async fn entry_consumptions(f: &AdmissionFixture) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM workflow_action_remote_entries),(SELECT count(*) FROM workflow_action_evidence_consumptions)")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

async fn competing_proof(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    ledger: &Arc<LedgerVerifier>,
) {
    let parked = all_tables(f).await;
    let verifier = Arc::new(RendezvousVerifier {
        ledger: ledger.clone(),
        ready: Barrier::new(2),
    });
    let a = proof_command(f, request, ledger, "proof-a").await;
    let b = proof_command(f, request, ledger, "proof-b").await;
    let (a_result, b_result) = tokio::join!(
        run_command(f, &a, verifier.clone()),
        run_command(f, &b, verifier)
    );
    let results = [a_result.unwrap(), b_result.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(
                r.outcome,
                ReconciliationOutcome::Scheduled {
                    receipt_only: false
                }
            ))
            .count(),
        1
    );
    let proofs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_evidence WHERE disposition='final_not_applied'",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(proofs, 1);
    let scheduled = all_tables(f).await;
    assert_eq!(
        scheduled["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        scheduled["workflow_action_evidence"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for table in [
        "task_attempts",
        "workflow_budget_receipts",
        "workflow_root_budget_usage",
        "workflow_action_intents",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
    ] {
        assert_eq!(
            parked[table], scheduled[table],
            "schedule preserves {table}"
        );
    }
}

async fn lost_retry(
    f: &AdmissionFixture,
    request: &mut ActionDispatchRequest,
    ledger: &Arc<LedgerVerifier>,
) {
    let old_entry: Uuid = sqlx::query_scalar("SELECT id FROM workflow_action_remote_entries")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    // Existing claim helper genuinely races two claimants and asserts exactly one owner.
    new_claim(f, request).await;
    let retry = LedgerProvider::new(f, Delivery::Pending);
    let (first, second) = tokio::join!(invoke(f, request, &retry), invoke(f, request, &retry));
    let results = [first, second];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(AppError::Timeout(_))))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Ok(RemoteDispatchObservation::PossibleDispatchExists)))
            .count(),
        1
    );
    assert_eq!(retry.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(f).await, (2, 1));
    assert_eq!(effects(f).await, 0);
    assert!(
        !apply_old_entry(f, request, old_entry).await,
        "the exact covered old request cannot apply while the new request is active"
    );
    park(f, request).await;
    // Keep the DB/provider seam boxed for the stock test-thread stack.
    Box::pin(refuse_consumed(f, request, ledger)).await;
}

async fn refuse_consumed(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    ledger: &Arc<LedgerVerifier>,
) {
    assert_eq!(projection(f).await, ("needs_reconciliation".into(), false));
    assert!(!retry_safe(f, request).await);
    let restarted = proof_command(f, request, ledger, "lost-again-unknown").await;
    let unknown = run_command(f, &restarted, ledger.clone()).await.unwrap();
    assert_eq!(unknown.outcome, ReconciliationOutcome::UnknownRecorded);
    let frozen = all_tables(f).await;
    let replay = run_command(f, &restarted, ledger.clone()).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(
        all_tables(f).await,
        frozen,
        "restart exact replay mutates no table"
    );
    assert_eq!(
        f.persistence()
            .retry(RetryCommand {
                company_id: restarted.scope.company,
                run_id: restarted.scope.run,
                actor: restarted.actor,
                expected_revision: unknown.revision,
                command_key: IdempotencyKey::parse("consumed-ordinary-retry").unwrap()
            })
            .await
            .unwrap(),
        RetryResult::Unsafe {
            revision: unknown.revision
        }
    );
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(entry_consumptions(f).await, (2, 1));
}

async fn apply_old_entry(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    entry: Uuid,
) -> bool {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query(
        "SELECT invocation_id FROM fixture_provider_operations WHERE invocation_id=$1 FOR UPDATE",
    )
    .bind(request.subject.invocation.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    let applied = apply_on(&mut tx, entry, &good(), true).await.unwrap();
    tx.commit().await.unwrap();
    applied
}

async fn fresh_call(
    f: &AdmissionFixture,
    request: &mut ActionDispatchRequest,
    ledger: Arc<LedgerVerifier>,
) {
    barrier(f, request).await;
    assert!(!delayed_apply(f, request, &good()).await);
    let fresh = reconcile(f, request, marker(f).await, ledger, "fresh-two-entries").await;
    assert!(matches!(
        fresh.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    ));
    let coverage: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_evidence_coverage WHERE evidence_id=$1",
    )
    .bind(fresh.evidence.unwrap().as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(coverage, 2);
    new_claim(f, request).await;
    let new = LedgerProvider::new(
        f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    assert!(matches!(
        invoke(f, request, &new).await.unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(new.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(f).await, (3, 2));
    assert_eq!(effects(f).await, 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_proof_competing_commands_claimants_loss_and_fresh_coverage()
{
    let (f, mut request) = setup().await;
    let ledger = ledger(&f, &request).await;
    let old = LedgerProvider::new(&f, Delivery::Pending);
    assert!(invoke(&f, &request, &old).await.is_err());
    park(&f, &request).await;
    barrier(&f, &request).await;
    // Box the provider/DB phase seams to retain the stock2MiB stack budget.
    Box::pin(competing_proof(&f, &request, &ledger)).await;
    Box::pin(lost_retry(&f, &mut request, &ledger)).await;
    Box::pin(fresh_call(&f, &mut request, ledger)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_proof_new_consumed_entry_applied_recovery_is_legitimate() {
    let (f, mut request) = setup().await;
    let ledger = ledger(&f, &request).await;
    assert!(
        invoke(&f, &request, &LedgerProvider::new(&f, Delivery::Pending))
            .await
            .is_err()
    );
    park(&f, &request).await;
    barrier(&f, &request).await;
    assert!(matches!(
        reconcile(&f, &request, marker(&f).await, ledger.clone(), "old-final")
            .await
            .outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    ));
    new_claim(&f, &mut request).await;
    let new = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    assert!(invoke(&f, &request, &new).await.is_err());
    assert_eq!(entry_consumptions(&f).await, (2, 1));
    park(&f, &request).await;
    let recovered = reconcile(&f, &request, marker(&f).await, ledger, "new-applied").await;
    assert!(matches!(
        recovered.outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    ));
    assert_eq!(projection(&f).await, ("committed".into(), false));
    let state = all_tables(&f).await;
    assert_eq!(
        state["workflow_action_receipts"].as_array().unwrap().len(),
        1
    );
    assert_eq!(state["workflow_action_evidence_conflicts"], json!([]));
    new_claim(&f, &mut request).await;
    let restart = LedgerProvider::new(&f, Delivery::Pending);
    assert!(matches!(
        invoke(&f, &request, &restart).await.unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(restart.calls.load(Ordering::SeqCst), 0);
    assert_eq!(entry_consumptions(&f).await, (2, 1));
    assert_eq!(effects(&f).await, 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_proof_provider_barrier_competes_with_delayed_effect() {
    for _ in 0..3 {
        let (f, request) = setup().await;
        let ledger = ledger(&f, &request).await;
        assert!(
            invoke(&f, &request, &LedgerProvider::new(&f, Delivery::Pending))
                .await
                .is_err()
        );
        park(&f, &request).await;
        let ready = Barrier::new(2);
        let (applied, ()) = tokio::join!(
            async {
                ready.wait().await;
                delayed_apply(&f, &request, &good()).await
            },
            async {
                ready.wait().await;
                barrier(&f, &request).await
            }
        );
        assert!(
            !delayed_apply(&f, &request, &good()).await,
            "covered request never applies twice or after closure"
        );
        let result = reconcile(
            &f,
            &request,
            marker(&f).await,
            ledger,
            "barrier-versus-effect",
        )
        .await;
        assert_eq!(effects(&f).await, i64::from(applied));
        assert_eq!(
            result.outcome,
            ReconciliationOutcome::Scheduled {
                receipt_only: applied
            }
        );
        assert_eq!(entry_consumptions(&f).await, (1, 0));
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_proof_admin_cannot_authorize_revoked_original_new_call() {
    for revoke_actor in [true, false] {
        let (f, mut request) = setup().await;
        let resource = support::resources(&f).await;
        let admin = support::principal(&f, Some("admin")).await;
        let ledger = ledger(&f, &request).await;
        assert!(
            invoke(&f, &request, &LedgerProvider::new(&f, Delivery::Pending))
                .await
                .is_err()
        );
        park(&f, &request).await;
        barrier(&f, &request).await;
        let mut c = proof_command(&f, &request, &ledger, "admin-final").await;
        c.actor = admin;
        if revoke_actor {
            sqlx::query("DELETE FROM principals WHERE company_id=$1 AND user_id=$2")
                .bind(c.scope.company.as_uuid())
                .bind(f.binding.target.actor.user_id())
                .execute(f.persistence().pool())
                .await
                .unwrap();
        } else {
            support::grant(&f, f.binding.target.actor, false).await;
        }
        let saved = support::execute(&f, &c, ledger.clone()).await.unwrap();
        assert!(matches!(
            saved.outcome,
            ReconciliationOutcome::Scheduled {
                receipt_only: false
            }
        ));
        new_claim(&f, &mut request).await;
        let frozen = all_tables(&f).await;
        let provider = LedgerProvider::new(&f, Delivery::Pending);
        let service = ActionService::new(PostgresActionDispatch::new(
            f.persistence().clone(),
            resource,
            Effect(0),
        ));
        assert!(
            service
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(entry_consumptions(&f).await, (1, 0));
        assert_eq!(
            all_tables(&f).await,
            frozen,
            "denied actual call leaves all durable facts unchanged"
        );
    }
}

#[path = "action_reconciliation_upgrade_tests.rs"]
mod upgrade_tests;
