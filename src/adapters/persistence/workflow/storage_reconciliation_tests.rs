use super::*;
use crate::application::workflow::storage::{RetentionRoot, WorkflowStorageMaintenance};

async fn terminal_old(f: &AdmissionFixture, request: &ActionDispatchRequest) -> RetentionRoot {
    let scope = request.scope();
    let revision = f
        .persistence()
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    let result = f
        .persistence()
        .cancel(CancelCommand {
            company_id: scope.company,
            run_id: scope.run,
            actor: f.binding.target.actor,
            command_key: IdempotencyKey::parse("retention-cancel").unwrap(),
            expected_revision: revision,
        })
        .await
        .unwrap();
    assert!(matches!(result, CancelResult::Applied { .. }));
    let finished_at = sqlx::query_scalar(
        "UPDATE workflow_runs SET created_at=clock_timestamp()-interval '100 days',
        finished_at=clock_timestamp()-interval '91 days' WHERE id=$1 RETURNING finished_at",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    RetentionRoot {
        finished_at,
        company: scope.company,
        run: scope.run,
    }
}
#[tokio::test]
async fn run_retention_unknown_effect_is_protected_while_complete_not_applied_proof_closes_it() {
    let (f, request) = Box::pin(limited()).await;
    let _verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(invoke(&f, &request, &provider).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    park(&f, &request).await;
    let root = terminal_old(&f, &request).await;
    assert_eq!(f.persistence().retire_tree(root).await.unwrap(), None);
    let s = Box::pin(scheduled()).await;
    let root = terminal_old(&s.fixture, &s.request).await;
    assert!(
        s.fixture
            .persistence()
            .retire_tree(root)
            .await
            .unwrap()
            .is_some()
    );
}
#[tokio::test]
async fn run_retention_consumed_old_proof_cannot_close_a_later_remote_entry() {
    let mut s = Box::pin(scheduled()).await;
    let Scheduled {
        fixture, request, ..
    } = &mut s;
    new_claim(fixture, request).await;
    let provider = LedgerProvider::new(fixture, Delivery::Pending);
    assert!(invoke(fixture, request, &provider).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(fixture).await, (2, 1));
    park(fixture, request).await;
    let root = terminal_old(fixture, request).await;
    assert_eq!(fixture.persistence().retire_tree(root).await.unwrap(), None);
}

#[tokio::test]
async fn run_retention_evidence_conflict_keeps_terminal_tree_even_with_exact_receipt() {
    let (f, mut request) = setup().await;
    let verifier = ledger(&f, &request).await;
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reserved) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("marker")
    };
    let marker = reserved.marker;
    let old = writer.enter_remote(*reserved).await.unwrap();
    ledger_row(&f, &request, old.entry, None).await;
    park(&f, &request).await;
    assert!(matches!(
        reconcile(&f, &request, marker, verifier, "retention-proof")
            .await
            .outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    ));
    new_claim(&f, &mut request).await;
    let RemoteReservationResult::Reserved(reserved) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("proof marker")
    };
    let new = writer.enter_remote(*reserved).await.unwrap();
    writer
        .finish_remote(new, json!({"items":[],"token_count":0,"omitted":0}))
        .await
        .unwrap();
    writer
        .finish_remote(old, json!({"items":[],"token_count":1,"omitted":0}))
        .await
        .unwrap();
    let facts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM workflow_action_receipts),(SELECT count(*) FROM workflow_action_evidence_conflicts)")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(facts.0, 1);
    assert!(facts.1 > 0);
    let root = terminal_old(&f, &request).await;
    assert_eq!(f.persistence().retire_tree(root).await.unwrap(), None);
}

struct ExpiringVerifier(Arc<LedgerVerifier>);
#[async_trait]
impl ActionEvidenceVerifier for ExpiringVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.0.registration()
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        let mut proof = self.0.verify(snapshot, reference, cancellation).await?;
        proof.valid_until = proof.observed_at + chrono::Duration::seconds(3);
        Ok(proof)
    }
}
#[tokio::test]
async fn run_retention_historical_not_applied_closure_survives_retry_grant_expiry() {
    let (f, request) = Box::pin(limited()).await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(invoke(&f, &request, &provider).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    park(&f, &request).await;
    barrier(&f, &request).await;
    let command = proof_command(&f, &request, &verifier, "retention-expiring").await;
    assert!(matches!(
        run_command(&f, &command, Arc::new(ExpiringVerifier(verifier)))
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    ));
    let root = terminal_old(&f, &request).await;
    let until: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT max(valid_until) FROM workflow_action_evidence")
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    tokio::time::sleep(
        (until - chrono::Utc::now()).to_std().unwrap_or_default()
            + std::time::Duration::from_millis(20),
    )
    .await;
    let expired: bool = sqlx::query_scalar(
        "SELECT bool_and(valid_until<=clock_timestamp()) FROM workflow_action_evidence",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert!(expired);
    assert!(f.persistence().retire_tree(root).await.unwrap().is_some());
}
