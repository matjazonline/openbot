//! Absolute conflict veto and actual receipt-before-completion ordering.
use super::*;

#[tokio::test]
async fn workflow_action_reconciliation_runtime_conflicted_sibling_vetoes_all_receipt_continuation()
{
    let (f, requests, ledger) = seed_siblings(&[SiblingTruth::Receipt; 2]).await;
    let first = reconcile(
        &f,
        &requests[0],
        scoped_marker(&f, &requests[0]).await,
        ledger.clone(),
        "first-positive",
    )
    .await;
    assert!(matches!(
        first.outcome,
        ReconciliationOutcome::Blocked { .. }
    ));
    let contradiction = Arc::new(ForcedVerifier {
        inner: ledger.clone(),
        final_not_applied: true,
        future: false,
    });
    let conflict = reconcile_with(
        &f,
        &requests[0],
        scoped_marker(&f, &requests[0]).await,
        contradiction,
        "contradict-first",
    )
    .await
    .unwrap();
    assert_eq!(conflict.outcome, ReconciliationOutcome::EvidenceConflict);
    let before = all_tables(&f).await;
    let second = reconcile(
        &f,
        &requests[1],
        scoped_marker(&f, &requests[1]).await,
        ledger,
        "second-positive",
    )
    .await;
    assert!(!matches!(
        second.outcome,
        ReconciliationOutcome::Scheduled { .. }
    ));
    let after = all_tables(&f).await;
    preserved_history(&before, &after);
    assert_eq!(before["background_tasks"], after["background_tasks"]);
    assert_eq!(
        after["workflow_action_receipts"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        before["workflow_action_evidence_conflicts"],
        after["workflow_action_evidence_conflicts"]
    );
    assert!(
        !after["workflow_action_evidence_conflicts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!retry_safe(&f, &requests[0]).await);
    let before_claim = all_tables(&f).await;
    assert!(
        f.persistence()
            .claim_io(requests[0].fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(before_claim, all_tables(&f).await);
    assert_eq!(entry_consumptions(&f).await, (2, 0));
}

async fn accepted_before_conflict(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
) -> Arc<LedgerVerifier> {
    let verifier = ledger(f, request).await;
    let provider = LedgerProvider::new(
        f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    assert!(matches!(
        invoke(f, request, &provider).await.unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    verifier
}

async fn contradict(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
) {
    let before = all_tables(f).await;
    let result = reconcile_with(
        f,
        request,
        scoped_marker(f, request).await,
        Arc::new(ForcedVerifier {
            inner: verifier,
            final_not_applied: true,
            future: false,
        }),
        "contradict-after-return",
    )
    .await
    .unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::EvidenceConflict);
    let after = all_tables(f).await;
    assert_eq!(
        before["workflow_action_receipts"],
        after["workflow_action_receipts"]
    );
    assert!(result.revision.0 > before["workflow_runs"][0]["revision"].as_u64().unwrap());
    assert_eq!(
        result.revision.0,
        after["workflow_runs"][0]["revision"].as_u64().unwrap()
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_runtime_conflict_after_return_before_completion_retires_exact_attempt()
 {
    let (f, request) = setup().await;
    let verifier = accepted_before_conflict(&f, &request).await;
    contradict(&f, &request, verifier).await;
    let before = all_tables(&f).await;
    assert!(adapter(&f, 0).reserve_remote(&request, None).await.is_err());
    assert_eq!(before, all_tables(&f).await);
    // No heartbeat runs first: shared completion itself must reject the returned result.
    assert!(
        f.persistence()
            .complete_io(FencedWorkflowResult {
                fence: request.fence,
                output: good()
            })
            .await
            .unwrap()
            .is_none()
    );
    let after = all_tables(&f).await;
    assert_eq!(
        before["workflow_action_receipts"],
        after["workflow_action_receipts"]
    );
    assert_eq!(
        after["workflow_executions"][0]["committed_output"],
        Value::Null
    );
    assert_eq!(
        after["workflow_executions"][0]["committed_route"],
        Value::Null
    );
    assert_eq!(
        after["workflow_executions"][0]["successor_execution_id"],
        Value::Null
    );
    assert_eq!(after["task_attempts"].as_array().unwrap().len(), 1);
    assert_eq!(
        after["task_attempts"][0]["workflow_failure_code"],
        "action.evidence_conflict"
    );
    assert!(
        f.persistence()
            .renew_io(request.fence, policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(after, all_tables(&f).await);
}

#[tokio::test]
async fn workflow_action_reconciliation_runtime_completion_before_conflict_preserves_committed_history()
 {
    let (f, request) = setup().await;
    let verifier = accepted_before_conflict(&f, &request).await;
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
    let before = all_tables(&f).await;
    contradict(&f, &request, verifier).await;
    let after = all_tables(&f).await;
    assert_eq!(before["background_tasks"], after["background_tasks"]);
    assert_eq!(before["workflow_executions"], after["workflow_executions"]);
    assert_eq!(before["task_attempts"], after["task_attempts"]);
    assert_eq!(
        before["workflow_budget_receipts"],
        after["workflow_budget_receipts"]
    );
    for key in ["state", "waiting_reason", "terminal_execution_id"] {
        assert_eq!(
            before["workflow_runs"][0][key],
            after["workflow_runs"][0][key]
        );
    }
    let before_claim = all_tables(&f).await;
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(before_claim, all_tables(&f).await);
}
