//! Fresh terminal truth retains genuine completed or ordinarily failed history.
use super::super::super::super::revision_tests::{replay_and_stale, truth_delta};
use super::*;
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};

struct FailedAction {
    pending: PendingAction,
    provider: LedgerProvider,
}

async fn receipted_action() -> FailedAction {
    // Box genuine admission/dispatch seams to retain stock 2 MiB test stacks.
    let (f, scope) = Box::pin(fixture_source(source())).await;
    let (f, request) = Box::pin(setup_action_on_fixture(
        f,
        scope,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    let RemoteDispatchObservation::Committed(receipt) =
        invoke(&f, &request, &provider).await.unwrap()
    else {
        panic!("actual provider return must commit a usable receipt");
    };
    assert_eq!(receipt.result, good());
    assert_eq!(effects(&f).await, 1);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    FailedAction {
        pending: PendingAction {
            fixture: f,
            request,
            verifier,
        },
        provider,
    }
}

async fn proven_action() -> FailedAction {
    let (f, mut request) = Box::pin(setup()).await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(matches!(
        invoke(&f, &request, &provider).await,
        Err(AppError::Timeout(_))
    ));
    park(&f, &request).await;
    barrier(&f, &request).await;
    let scheduled = reconcile(
        &f,
        &request,
        marker(&f).await,
        verifier.clone(),
        "before-terminal-final",
    )
    .await;
    assert_eq!(
        scheduled.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    new_claim(&f, &mut request).await;
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert_eq!(effects(&f).await, 0);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    FailedAction {
        pending: PendingAction {
            fixture: f,
            request,
            verifier,
        },
        provider,
    }
}

async fn fail_live(pending: &PendingAction) -> Value {
    let f = &pending.fixture;
    assert!(retry_safe(f, &pending.request).await);
    let claimed = all_tables(f).await;
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("fixture.terminal").unwrap(),
            None,
        )
        .unwrap(),
        // The recovery owner must derive safety from the receipt or final proof.
        safety: RetrySafety::EffectOutcomeUnknown,
    });
    assert!(
        f.persistence()
            .release_io(pending.request.fence, policy(), cause.clone())
            .await
            .unwrap()
    );
    let failed = all_tables(f).await;
    assert!(
        !f.persistence()
            .release_io(pending.request.fence, policy(), cause)
            .await
            .unwrap()
    );
    assert_eq!(
        failed,
        all_tables(f).await,
        "exact retired fence cannot release twice"
    );
    failed_history(&claimed, &failed, pending);
    let future: bool = sqlx::query_scalar(
        "SELECT deadline>clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(pending.request.scope().company.as_uuid())
    .bind(pending.request.scope().run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert!(future, "ordinary Failed is distinct from deadline expiry");
    assert!(retry_safe(f, &pending.request).await);
    failed
}

fn failed_history(claimed: &Value, failed: &Value, pending: &PendingAction) {
    let fence = pending.request.fence;
    assert_eq!(failed["workflow_runs"][0]["state"], "failed");
    assert_eq!(
        failed["workflow_runs"][0]["deadline"],
        claimed["workflow_runs"][0]["deadline"]
    );
    assert_eq!(
        failed["workflow_runs"][0]["terminal_execution_id"],
        json!(fence.scope.execution.as_uuid())
    );
    retired_attempt(claimed, failed, fence);
    retained_action(claimed, failed);
}

fn retired_attempt(claimed: &Value, failed: &Value, fence: WorkflowFence) {
    let job = row(failed, "background_tasks", fence.scope.job.0);
    assert_eq!(job["status"], "failed");
    for column in [
        "worker_id",
        "locked_at",
        "lock_expires_at",
        "execution_generation",
    ] {
        assert!(job[column].is_null(), "retired {column}");
    }
    let attempts = failed["task_attempts"].as_array().unwrap();
    assert_eq!(
        attempts.len(),
        claimed["task_attempts"].as_array().unwrap().len()
    );
    let attempt = attempts
        .iter()
        .find(|row| {
            row["task_id"] == json!(fence.scope.job.0)
                && row["attempt_number"] == json!(fence.attempt.0)
        })
        .unwrap();
    assert_eq!(attempt["worker_id"], json!(fence.worker.0));
    assert_eq!(attempt["execution_generation"], json!(fence.generation.0));
    assert_eq!(attempt["status"], "failed");
    assert_eq!(attempt["workflow_failure_class"], "terminal");
    assert_eq!(attempt["workflow_failure_code"], "fixture.terminal");
    assert_eq!(attempt["workflow_retry_safety"], "safe");
    assert_eq!(attempt["workflow_retirement"], "live");
    assert!(attempt["finished_at"].is_string());
    for old in claimed["task_attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["attempt_number"] != json!(fence.attempt.0))
    {
        assert!(attempts.contains(old), "prior retired attempt survives");
    }
}

fn retained_action(claimed: &Value, failed: &Value) {
    assert_eq!(
        failed["workflow_executions"],
        claimed["workflow_executions"]
    );
    assert_eq!(failed["workflow_executions"].as_array().unwrap().len(), 1);
    let execution = &failed["workflow_executions"][0];
    for column in [
        "completed_at",
        "committed_output",
        "committed_route",
        "successor_execution_id",
    ] {
        assert!(execution[column].is_null(), "no ordinary failure {column}");
    }
    for table in [
        "workflow_action_receipts",
        "workflow_action_remote_entries",
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_commands",
        "workflow_action_evidence_consumptions",
        "fixture_provider_effects",
        "fixture_provider_operations",
        "fixture_evidence_ledger",
        "workflow_budget_receipts",
        "workflow_root_budget_usage",
    ] {
        assert_eq!(
            failed[table], claimed[table],
            "terminal owner preserves {table}"
        );
    }
}

async fn failed_case(history: FailedAction, truth: SiblingTruth) {
    let pending = &history.pending;
    let f = &pending.fixture;
    let before = Box::pin(fail_live(pending)).await;
    let mut command = history_command(pending, &pending.request, "failed-terminal-truth").await;
    let expected = match truth {
        SiblingTruth::Unknown => {
            command.input = EvidenceInput::UnknownNote {
                note: "no additional provider conclusion".into(),
                claimed: ClaimedDisposition::Unknown,
            };
            ReconciliationOutcome::UnknownRecorded
        }
        SiblingTruth::Receipt => ReconciliationOutcome::AppliedRecorded { receipt: true },
        SiblingTruth::Final => ReconciliationOutcome::NotAppliedRecorded,
        _ => panic!("only reachable ordinary Failed truths"),
    };
    let result = run_command(f, &command, pending.verifier.clone())
        .await
        .unwrap();
    assert!(!result.replayed);
    assert_eq!(result.outcome, expected);
    truth_delta(&before, &all_tables(f).await, &command, &result, truth);
    Box::pin(replay_and_stale(
        f,
        &command,
        pending.verifier.clone(),
        &result,
    ))
    .await;
    let saved = all_tables(f).await;
    assert!(
        f.persistence()
            .claim_io(pending.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        saved,
        all_tables(f).await,
        "ordinary Failed truth cannot create an attempt or debit"
    );
    assert_eq!(history.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(f).await, (1, 0));
    assert_eq!(
        effects(f).await,
        i64::from(!matches!(truth, SiblingTruth::Final))
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_revision_failed_receipt_unknown_is_audit_only() {
    Box::pin(failed_case(
        Box::pin(receipted_action()).await,
        SiblingTruth::Unknown,
    ))
    .await;
}

#[tokio::test]
async fn workflow_action_reconciliation_revision_failed_receipt_applied_replay_and_stale_refusal() {
    Box::pin(failed_case(
        Box::pin(receipted_action()).await,
        SiblingTruth::Receipt,
    ))
    .await;
}

#[tokio::test]
async fn workflow_action_reconciliation_revision_failed_unconsumed_final_replay_and_stale_refusal()
{
    Box::pin(failed_case(
        Box::pin(proven_action()).await,
        SiblingTruth::Final,
    ))
    .await;
}

async fn succeeded_case(truth: SiblingTruth) {
    // Reuse the actual provider-return/completion owner, boxed for stock test stacks.
    let completed = Box::pin(completed_action(source())).await;
    assert!(completed.successor.is_none());
    let pending = &completed.pending;
    let f = &pending.fixture;
    let before = all_tables(f).await;
    completion_history(&before, &completed);
    let mut command = history_command(pending, &pending.request, "terminal-revision").await;
    let expected = match truth {
        SiblingTruth::Unknown => {
            command.input = EvidenceInput::UnknownNote {
                note: "no additional provider conclusion".into(),
                claimed: ClaimedDisposition::Unknown,
            };
            ReconciliationOutcome::UnknownRecorded
        }
        SiblingTruth::Receipt => ReconciliationOutcome::AppliedRecorded { receipt: true },
        _ => panic!("final absence conflicts with the durable actual receipt"),
    };
    let result = run_command(f, &command, pending.verifier.clone())
        .await
        .unwrap();
    assert!(!result.replayed);
    assert_eq!(result.outcome, expected);
    truth_delta(&before, &all_tables(f).await, &command, &result, truth);
    Box::pin(replay_and_stale(
        f,
        &command,
        pending.verifier.clone(),
        &result,
    ))
    .await;
    let saved = all_tables(f).await;
    assert!(
        f.persistence()
            .claim_io(pending.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        saved,
        all_tables(f).await,
        "terminal truth never creates an attempt or debit"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_revision_succeeded_unknown_is_audit_only() {
    Box::pin(succeeded_case(SiblingTruth::Unknown)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_revision_succeeded_applied_replay_and_stale_refusal() {
    Box::pin(succeeded_case(SiblingTruth::Receipt)).await;
}
