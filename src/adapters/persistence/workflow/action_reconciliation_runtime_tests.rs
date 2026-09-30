//! Genuine supported-provider truth, mixed-policy siblings and live conflict gates.
use super::*;
use crate::application::workflow::publication::ActionRecovery;

#[path = "action_reconciliation_consumption_identity_tests.rs"]
mod consumption_identity_tests;

#[path = "action_reconciliation_high_bounds_tests.rs"]
mod high_bounds_tests;

struct RegisteredLedger {
    inner: LedgerProvider,
    contract: ProviderReplayContract,
}
impl RegisteredLedger {
    async fn new(
        f: &AdmissionFixture,
        request: &ActionDispatchRequest,
        delivery: Delivery,
    ) -> Self {
        let saved = f
            .persistence()
            .action_authority(request.scope(), &request.subject)
            .await
            .unwrap()
            .action;
        Self {
            inner: LedgerProvider::new(f, delivery),
            contract: ProviderReplayContract::approve(
                TypeName::parse("fixture.safe-provider.v1").unwrap(),
                &saved.request().contract,
                &saved.request().target,
                ProviderReplayMode::SafeRepeat,
            )
            .unwrap(),
        }
    }
}
#[async_trait]
impl RemoteAction for RegisteredLedger {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        Some(&self.contract)
    }
    async fn invoke(
        &self,
        action: &FrozenAction,
        invocation: &ProviderInvocation,
    ) -> AppResult<Value> {
        self.inner.invoke(action, invocation).await
    }
}

struct InvalidRecovery(Arc<LedgerVerifier>);
#[async_trait]
impl ActionEvidenceVerifier for InvalidRecovery {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.0.registration()
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        let mut attestation = self.0.verify(snapshot, reference, cancellation).await?;
        if let VerifiedDisposition::Applied {
            recovered_result, ..
        } = &mut attestation.disposition
        {
            *recovered_result = Some(json!(42));
        }
        Ok(attestation)
    }
}

async fn rust_sql_equivalence(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    expected: bool,
) {
    assert_eq!(retry_safe(f, request).await, expected);
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(request.fence.scope.job.0).execute(&mut *tx).await.unwrap();
    recovery::retire_on(&mut tx, request.fence, lease::Retirement::Expired)
        .await
        .unwrap();
    let safety: String = sqlx::query_scalar(
        "SELECT workflow_retry_safety FROM task_attempts WHERE task_id=$1 AND attempt_number=$2",
    )
    .bind(request.fence.scope.job.0)
    .bind(request.fence.attempt.0)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(safety, if expected { "safe" } else { "unknown" });
    tx.rollback().await.unwrap();
    assert_eq!(all_tables(f).await, before);
}

async fn supported_positive(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    ledger: Arc<LedgerVerifier>,
    invalid: bool,
) {
    let provider = RegisteredLedger::new(
        f,
        request,
        Delivery::Apply {
            result: good(),
            recover: invalid,
            lose: true,
        },
    )
    .await;
    assert!(registered_invoke(f, request, &provider).await.is_err());
    rust_sql_equivalence(f, request, true).await;
    let verifier: Arc<dyn ActionEvidenceVerifier> = if invalid {
        Arc::new(InvalidRecovery(ledger))
    } else {
        ledger
    };
    let recorded = reconcile_with(
        f,
        request,
        scoped_marker(f, request).await,
        verifier,
        "positive-without-usable-result",
    )
    .await
    .unwrap();
    assert_eq!(
        recorded.outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: false }
    );
    rust_sql_equivalence(f, request, false).await;
    park(f, request).await;
    let before = all_tables(f).await;
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(before, all_tables(f).await);
    assert_eq!(before["workflow_action_receipts"], json!([]));
    assert_eq!(
        before["workflow_executions"][0]["committed_output"],
        Value::Null
    );
    assert_eq!(
        before["workflow_executions"][0]["successor_execution_id"],
        Value::Null
    );
    assert_eq!(entry_consumptions(f).await, (1, 0));
    assert_eq!(provider.inner.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_runtime_supported_positive_veto_then_genuine_valid_receipt()
{
    for invalid in [false, true] {
        let (f, mut request) = setup_action(ActionRecovery::SafeRepeat, json!({"value":1})).await;
        let verifier = ledger(&f, &request).await;
        supported_positive(&f, &request, verifier.clone(), invalid).await;
        // The durable effect is valid; output retrieval, rather than a new call, becomes available.
        sqlx::query(
            "UPDATE fixture_evidence_ledger SET recover_result=true,observed_at=clock_timestamp()",
        )
        .execute(f.persistence().pool())
        .await
        .unwrap();
        let before = all_tables(&f).await;
        let resolved = reconcile(
            &f,
            &request,
            scoped_marker(&f, &request).await,
            verifier,
            "valid-recovered-result",
        )
        .await;
        assert_eq!(
            resolved.outcome,
            ReconciliationOutcome::Scheduled { receipt_only: true }
        );
        let after = all_tables(&f).await;
        preserved_history(&before, &after);
        assert_eq!(
            resolved.revision.0,
            after["workflow_runs"][0]["revision"].as_u64().unwrap()
        );
        assert_eq!(
            after["workflow_action_receipts"].as_array().unwrap().len(),
            1
        );
        assert!(retry_safe(&f, &request).await);
        new_claim(&f, &mut request).await;
        rust_sql_equivalence(&f, &request, true).await;
        let restart = RegisteredLedger::new(&f, &request, Delivery::Pending).await;
        assert!(matches!(
            registered_invoke(&f, &request, &restart).await.unwrap(),
            RemoteDispatchObservation::Committed(_)
        ));
        assert_eq!(restart.inner.calls.load(Ordering::SeqCst), 0);
        assert_eq!(effects(&f).await, 1);
        assert_eq!(entry_consumptions(&f).await, (1, 0));
        assert!(
            f.persistence()
                .complete_io(FencedWorkflowResult {
                    fence: request.fence,
                    output: good()
                })
                .await
                .unwrap()
                .is_some()
        );
    }
}

#[path = "action_reconciliation_runtime_mixed_tests.rs"]
mod mixed_tests;

#[path = "action_reconciliation_runtime_conflict_tests.rs"]
mod conflict_tests;

async fn registered_invoke(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &RegisteredLedger,
) -> AppResult<RemoteDispatchObservation> {
    ActionService::new(adapter(f, 0))
        .dispatch_remote(request, provider, &CancellationToken::new())
        .await
}
