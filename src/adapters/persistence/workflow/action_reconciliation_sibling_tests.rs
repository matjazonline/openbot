//! Execution-wide safety must not hide an unresolved remote invocation.
use super::*;

#[path = "action_reconciliation_sibling_boundary_tests.rs"]
mod boundary_tests;

#[path = "action_reconciliation_runtime_tests.rs"]
mod runtime_tests;

#[path = "action_reconciliation_bounds_tests.rs"]
mod bounds_tests;

#[path = "action_reconciliation_eligibility_tests.rs"]
mod eligibility_tests;

#[path = "action_reconciliation_revision_tests.rs"]
mod revision_tests;

#[path = "action_reconciliation_revision_overflow_tests.rs"]
mod revision_overflow_tests;

#[path = "action_reconciliation_receipt_sql_tests.rs"]
mod receipt_sql_tests;

#[derive(Clone, Copy)]
enum SiblingTruth {
    Receipt,
    Final,
    Unknown,
    AppliedNoResult,
    InvalidResult,
}

async fn sibling(
    f: &AdmissionFixture,
    original: &ActionDispatchRequest,
    number: usize,
) -> ActionDispatchRequest {
    let saved = f
        .persistence()
        .action_authority(original.scope(), &original.subject)
        .await
        .unwrap()
        .action;
    let mut action = saved.request().clone();
    action.arguments = json!({"value":number});
    let intent = ActionService::new(f.persistence().clone())
        .prepare_step(action)
        .await
        .unwrap();
    let frozen = f
        .persistence()
        .action_authority(original.scope(), &intent.approval_subject())
        .await
        .unwrap()
        .action;
    sqlx::query(
        "INSERT INTO fixture_provider_operations(invocation_id,provider_key) VALUES($1,$2)",
    )
    .bind(intent.invocation.as_uuid())
    .bind(frozen.idempotency_key().as_str())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    ActionDispatchRequest {
        subject: intent.approval_subject(),
        ..original.clone()
    }
}

async fn scoped_marker(f: &AdmissionFixture, request: &ActionDispatchRequest) -> Uuid {
    sqlx::query_scalar("SELECT id FROM workflow_action_dispatches WHERE invocation_id=$1")
        .bind(request.subject.invocation.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

async fn seed_siblings(
    truths: &[SiblingTruth],
) -> (
    AdmissionFixture,
    Vec<ActionDispatchRequest>,
    Arc<LedgerVerifier>,
) {
    let (f, request) = setup().await;
    let verifier = ledger(&f, &request).await;
    let mut requests = vec![request];
    for number in 1..truths.len() {
        requests.push(sibling(&f, &requests[0], number + 10).await);
    }
    for (request, truth) in requests.iter().zip(truths) {
        let delivery = match truth {
            SiblingTruth::Receipt => Delivery::Apply {
                result: good(),
                recover: true,
                lose: true,
            },
            SiblingTruth::AppliedNoResult => Delivery::Apply {
                result: good(),
                recover: false,
                lose: true,
            },
            SiblingTruth::InvalidResult => Delivery::Apply {
                result: json!(42),
                recover: true,
                lose: true,
            },
            _ => Delivery::Pending,
        };
        assert!(
            invoke(&f, request, &LedgerProvider::new(&f, delivery))
                .await
                .is_err()
        );
        if matches!(truth, SiblingTruth::Final) {
            barrier(&f, request).await;
        }
    }
    park(&f, &requests[0]).await;
    (f, requests, verifier)
}

fn preserved_history(before: &Value, after: &Value) {
    for table in [
        "task_attempts",
        "workflow_budget_receipts",
        "workflow_root_budget_usage",
        "workflow_action_intents",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
    ] {
        assert_eq!(
            before[table], after[table],
            "reconciliation preserves {table}"
        );
    }
    let old_job = &before["background_tasks"][0];
    let new_job = &after["background_tasks"][0];
    assert_eq!(after["background_tasks"].as_array().unwrap().len(), 1);
    for key in [
        "id",
        "workflow_execution_id",
        "retry_count",
        "max_retries",
        "payload",
    ] {
        assert_eq!(old_job[key], new_job[key], "existing job preserves {key}");
    }
    for key in [
        "id",
        "activation",
        "activated_at",
        "lineage",
        "committed_output",
        "committed_route",
        "successor_execution_id",
        "completed_at",
    ] {
        assert_eq!(
            before["workflow_executions"][0][key], after["workflow_executions"][0][key],
            "existing execution preserves {key}"
        );
    }
}

async fn settle_siblings(
    f: &AdmissionFixture,
    requests: &[ActionDispatchRequest],
    verifier: Arc<LedgerVerifier>,
) -> Vec<ReconciliationResult> {
    let mut outcomes = Vec::new();
    for (number, request) in requests.iter().enumerate() {
        let saved = all_tables(f).await;
        let result = reconcile(
            f,
            request,
            scoped_marker(f, request).await,
            verifier.clone(),
            &format!("sibling-{number}"),
        )
        .await;
        preserved_history(&saved, &all_tables(f).await);
        outcomes.push(result);
    }
    outcomes
}

async fn continue_siblings(
    f: &AdmissionFixture,
    requests: &mut [ActionDispatchRequest],
    truths: &[SiblingTruth],
) {
    let before = all_tables(f).await;
    new_claim(f, &mut requests[0]).await;
    let fence = requests[0].fence;
    for (request, truth) in requests.iter_mut().zip(truths) {
        request.fence = fence;
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
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            usize::from(matches!(truth, SiblingTruth::Final))
        );
    }
    let expected_proofs = truths
        .iter()
        .filter(|truth| matches!(truth, SiblingTruth::Final))
        .count();
    assert_eq!(
        entry_consumptions(f).await,
        (
            (truths.len() + expected_proofs) as i64,
            expected_proofs as i64
        )
    );
    assert_eq!(effects(f).await, truths.len() as i64);
    let after = all_tables(f).await;
    for old in before["task_attempts"].as_array().unwrap() {
        assert!(
            after["task_attempts"].as_array().unwrap().contains(old),
            "old attempt remains byte-for-byte unchanged"
        );
    }
    assert!(
        f.persistence()
            .complete_io(FencedWorkflowResult {
                fence,
                output: good()
            })
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_siblings_all_receipts_all_proofs_and_mixed_continue_individually()
 {
    for truths in [
        vec![SiblingTruth::Receipt; 3],
        vec![SiblingTruth::Final; 3],
        vec![
            SiblingTruth::Receipt,
            SiblingTruth::Final,
            SiblingTruth::Receipt,
        ],
    ] {
        let (f, mut requests, verifier) = seed_siblings(&truths).await;
        let outcomes = settle_siblings(&f, &requests, verifier).await;
        assert!(
            outcomes[..2]
                .iter()
                .all(|result| matches!(result.outcome, ReconciliationOutcome::Blocked { .. }))
        );
        assert_eq!(
            outcomes[2].outcome,
            ReconciliationOutcome::Scheduled {
                receipt_only: truths
                    .iter()
                    .all(|truth| matches!(truth, SiblingTruth::Receipt))
            }
        );
        assert!(retry_safe(&f, &requests[0]).await);
        Box::pin(continue_siblings(&f, &mut requests, &truths)).await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_siblings_unknown_and_applied_without_output_keep_all_parked()
 {
    for blocker in [
        SiblingTruth::Unknown,
        SiblingTruth::AppliedNoResult,
        SiblingTruth::InvalidResult,
    ] {
        let truths = [SiblingTruth::Receipt, SiblingTruth::Final, blocker];
        let (f, requests, verifier) = seed_siblings(&truths).await;
        let outcomes = settle_siblings(&f, &requests, verifier).await;
        assert!(
            outcomes
                .iter()
                .all(|result| !matches!(result.outcome, ReconciliationOutcome::Scheduled { .. }))
        );
        assert!(!retry_safe(&f, &requests[0]).await);
        let before = all_tables(&f).await;
        assert!(
            f.persistence()
                .claim_io(requests[0].fence.scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            before,
            all_tables(&f).await,
            "blocked siblings cannot gain attempt, charge or entry"
        );
        assert_eq!(entry_consumptions(&f).await, (3, 0));
        assert_eq!(
            before["workflow_action_receipts"].as_array().unwrap().len(),
            1
        );
        assert_eq!(before["workflow_runs"][0]["state"], json!("waiting"));
        assert_eq!(
            before["workflow_runs"][0]["waiting_reason"],
            json!("reconciliation")
        );
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_siblings_later_valid_result_resolves_positive_truth() {
    let truths = [SiblingTruth::Receipt, SiblingTruth::AppliedNoResult];
    let (f, mut requests, verifier) = seed_siblings(&truths).await;
    let outcomes = settle_siblings(&f, &requests, verifier.clone()).await;
    assert!(
        outcomes
            .iter()
            .all(|result| !matches!(result.outcome, ReconciliationOutcome::Scheduled { .. }))
    );
    assert!(!retry_safe(&f, &requests[0]).await);
    // The real effect was always valid; the provider can now recover its saved result.
    sqlx::query("UPDATE fixture_evidence_ledger SET recover_result=true,observed_at=clock_timestamp() WHERE invocation_id=$1")
        .bind(requests[1].subject.invocation.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let before = all_tables(&f).await;
    let outcome = reconcile(
        &f,
        &requests[1],
        scoped_marker(&f, &requests[1]).await,
        verifier,
        "later-valid-result",
    )
    .await;
    assert_eq!(
        outcome.outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    );
    preserved_history(&before, &all_tables(&f).await);
    assert!(retry_safe(&f, &requests[0]).await);
    Box::pin(continue_siblings(
        &f,
        &mut requests,
        &[SiblingTruth::Receipt; 2],
    ))
    .await;
}
