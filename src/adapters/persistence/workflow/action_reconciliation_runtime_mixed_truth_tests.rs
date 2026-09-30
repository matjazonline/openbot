//! A genuine unusable result vetoes a mixed execution until that same result is recovered.
use super::*;

#[path = "action_reconciliation_runtime_mixed_truth_assertions.rs"]
mod assertions;
use assertions::*;
#[path = "action_reconciliation_runtime_mixed_truth_evidence_assertions.rs"]
mod evidence_assertions;
use evidence_assertions::*;
#[path = "action_reconciliation_runtime_mixed_truth_schedule_assertions.rs"]
mod schedule_assertions;

#[path = "action_reconciliation_retained_truth_tests.rs"]
mod retained_truth_tests;
use schedule_assertions::*;

#[derive(Clone, Copy)]
enum UnusableResult {
    Missing,
    Invalid,
}
impl UnusableResult {
    fn diagnostic(self) -> ReconciliationBlockedReason {
        match self {
            Self::Missing => ReconciliationBlockedReason::MissingResult,
            Self::Invalid => ReconciliationBlockedReason::InvalidRecoveredResult,
        }
    }
    fn verifier(self, ledger: Arc<LedgerVerifier>) -> Arc<dyn ActionEvidenceVerifier> {
        match self {
            Self::Missing => ledger,
            Self::Invalid => Arc::new(InvalidRecovery(ledger)),
        }
    }
}

struct MixedTruth {
    fixture: AdmissionFixture,
    requests: Vec<ActionDispatchRequest>,
    ledger: Arc<LedgerVerifier>,
    first: LedgerProvider,
    second: LedgerProvider,
    supported: RegisteredLedger,
    unusable: UnusableResult,
}

async fn seeded(unusable: UnusableResult) -> MixedTruth {
    let (fixture, first_request) = Box::pin(setup()).await;
    let ledger = ledger(&fixture, &first_request).await;
    let requests = mixed_requests(&fixture, &first_request).await;
    let first = LedgerProvider::new(
        &fixture,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    let second = LedgerProvider::new(
        &fixture,
        Delivery::Apply {
            result: good(),
            recover: matches!(unusable, UnusableResult::Invalid),
            lose: true,
        },
    );
    let supported = RegisteredLedger::new(&fixture, &requests[2], Delivery::Pending).await;
    let service = ActionService::new(mixed_writer(&fixture));
    for (request, provider) in [(&requests[0], &first), (&requests[1], &second)] {
        assert!(matches!(
            Box::pin(service.dispatch_remote(request, provider, &CancellationToken::new())).await,
            Err(AppError::Timeout(_))
        ));
    }
    assert!(matches!(
        Box::pin(service.dispatch_remote(&requests[2], &supported, &CancellationToken::new()))
            .await,
        Err(AppError::Timeout(_))
    ));
    let state = all_tables(&fixture).await;
    assert_eq!(rows(&state, "workflow_action_remote_entries").len(), 3);
    assert_eq!(rows(&state, "fixture_evidence_ledger").len(), 3);
    assert_eq!(rows(&state, "fixture_provider_effects").len(), 2);
    assert!(
        rows(&state, "fixture_provider_effects")
            .iter()
            .all(|row| row["result"] == good())
    );
    assert_eq!(entry_consumptions(&fixture).await, (3, 0));
    MixedTruth {
        fixture,
        requests,
        ledger,
        first,
        second,
        supported,
        unusable,
    }
}

async fn evidence_command(h: &MixedTruth, index: usize, key: &str) -> ReconcileActionCommand {
    let request = &h.requests[index];
    let mut command = command(
        &h.fixture,
        request,
        scoped_marker(&h.fixture, request).await,
    )
    .await;
    command.command_key = IdempotencyKey::parse(key).unwrap();
    command.input = EvidenceInput::VerifiedReference {
        registration: h.ledger.registration().id().clone(),
        reference: EvidenceRecordReference::parse(key).unwrap(),
    };
    command
}

async fn record_live(h: &MixedTruth) -> ReconcileActionCommand {
    let first = evidence_command(h, 0, "mixed-genuine-first").await;
    let before = all_tables(&h.fixture).await;
    let recorded = Box::pin(run_command(&h.fixture, &first, h.ledger.clone()))
        .await
        .unwrap();
    assert_eq!(
        recorded.outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: true }
    );
    evidence_boundary(
        &before,
        &all_tables(&h.fixture).await,
        &first,
        &recorded,
        EvidenceChange::Receipt,
    );
    let second = evidence_command(h, 1, "mixed-unusable-result").await;
    let before = all_tables(&h.fixture).await;
    let recorded = Box::pin(run_command(
        &h.fixture,
        &second,
        h.unusable.verifier(h.ledger.clone()),
    ))
    .await
    .unwrap();
    assert_eq!(
        recorded.outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: false }
    );
    let after = all_tables(&h.fixture).await;
    evidence_boundary(&before, &after, &second, &recorded, EvidenceChange::Audit);
    let evidence = by_id(
        &after,
        "workflow_action_evidence",
        recorded.evidence.unwrap().as_uuid(),
    );
    assert_eq!(
        evidence["diagnostic"]["result"],
        json!(h.unusable.diagnostic())
    );
    assert_eq!(evidence["disposition"], "applied");
    assert_eq!(evidence["grant_eligible"], false);
    assert_eq!(rows(&after, "workflow_action_receipts").len(), 1);
    assert_eq!(rows(&after, "workflow_action_evidence_conflicts").len(), 0);
    assert_eq!(effects(&h.fixture).await, 2);
    Box::pin(live_recovery(h, false)).await;
    second
}

async fn live_recovery(h: &MixedTruth, expected: bool) {
    let request = &h.requests[0];
    assert_eq!(retry_safe(&h.fixture, request).await, expected);
    let before = all_tables(&h.fixture).await;
    live_fence(&before, request.fence);
    let mut tx = h.fixture.persistence().pool().begin().await.unwrap();
    let lease_live: bool = sqlx::query_scalar(
        "SELECT lock_expires_at > clock_timestamp() FROM background_tasks WHERE id=$1",
    )
    .bind(request.fence.scope.job.0)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(
        lease_live,
        "actual Rust recovery starts from the exact live processing attempt"
    );
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(request.fence.scope.job.0).execute(&mut *tx).await.unwrap();
    Box::pin(recovery::retire_on(
        &mut tx,
        request.fence,
        lease::Retirement::Expired,
    ))
    .await
    .unwrap();
    let attempt: Value = sqlx::query_scalar("SELECT to_jsonb(attempt) FROM task_attempts AS attempt WHERE task_id=$1 AND attempt_number=$2")
        .bind(request.fence.scope.job.0).bind(request.fence.attempt.0).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(attempt["worker_id"], json!(request.fence.worker.0));
    assert_eq!(
        attempt["execution_generation"],
        json!(request.fence.generation.0)
    );
    assert_eq!(attempt["status"], "failed");
    assert_eq!(attempt["workflow_failure_code"], "workflow.lease_expired");
    assert_eq!(attempt["workflow_retirement"], "expired");
    assert_eq!(
        attempt["workflow_retry_safety"],
        if expected { "safe" } else { "unknown" }
    );
    assert!(attempt["finished_at"].is_string());
    tx.rollback().await.unwrap();
    assert_eq!(
        all_tables(&h.fixture).await,
        before,
        "actual retirement rolls back every public row"
    );
}

async fn blocked(h: &MixedTruth) {
    let live = all_tables(&h.fixture).await;
    park(&h.fixture, &h.requests[0]).await;
    let parked = all_tables(&h.fixture).await;
    parked_fence(&live, &parked, &h.requests[0]);
    let command = evidence_command(h, 0, "mixed-first-receipt-cannot-reopen").await;
    let recorded = Box::pin(run_command(&h.fixture, &command, h.ledger.clone()))
        .await
        .unwrap();
    assert_eq!(
        recorded.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob
        }
    );
    let before = all_tables(&h.fixture).await;
    evidence_boundary(&parked, &before, &command, &recorded, EvidenceChange::Audit);
    let (left, right) = tokio::join!(
        h.fixture
            .persistence()
            .claim_io(h.requests[0].fence.scope, worker(), policy()),
        h.fixture
            .persistence()
            .claim_io(h.requests[0].fence.scope, worker(), policy())
    );
    assert!(left.unwrap().is_none() && right.unwrap().is_none());
    let service = ActionService::new(mixed_writer(&h.fixture));
    assert!(!matches!(
        Box::pin(service.dispatch_remote(&h.requests[2], &h.supported, &CancellationToken::new()))
            .await,
        Ok(RemoteDispatchObservation::Committed(_))
    ));
    assert_eq!(h.supported.inner.calls.load(Ordering::SeqCst), 1);
    assert!(
        h.fixture
            .persistence()
            .complete_io(FencedWorkflowResult {
                fence: h.requests[0].fence,
                output: good(),
            })
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        all_tables(&h.fixture).await,
        before,
        "no premature claimant, debit, entry, poll, output, route or successor"
    );
    assert_eq!(effects(&h.fixture).await, 2);
    assert_initial_calls(h);
}

async fn recover_same_result(h: &MixedTruth, old: &ReconcileActionCommand) {
    let before = all_tables(&h.fixture).await;
    let request = &h.requests[1];
    assert_eq!(sqlx::query("UPDATE fixture_evidence_ledger SET recover_result=true,observed_at=clock_timestamp() WHERE company_id=$1 AND invocation_id=$2 AND result IS NOT NULL")
        .bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid())
        .execute(h.fixture.persistence().pool()).await.unwrap().rows_affected(), 1);
    let available = all_tables(&h.fixture).await;
    retrieval_boundary(&before, &available, request);
    let command = evidence_command(h, 1, "mixed-later-genuine-valid-result").await;
    let recorded = Box::pin(run_command(&h.fixture, &command, h.ledger.clone()))
        .await
        .unwrap();
    assert_eq!(
        recorded.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let after = all_tables(&h.fixture).await;
    evidence_boundary(
        &available,
        &after,
        &command,
        &recorded,
        EvidenceChange::Schedule,
    );
    assert!(retry_safe(&h.fixture, &h.requests[0]).await);
    assert_eq!(rows(&after, "workflow_action_receipts").len(), 2);
    let replay = Box::pin(run_command(&h.fixture, old, h.ledger.clone()))
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        replay.outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: false }
    );
    assert_eq!(
        replay.evidence,
        Some(ActionEvidenceId::new(
            by_command(&after, old)["evidence_id"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap()
        ))
    );
    assert_eq!(
        all_tables(&h.fixture).await,
        after,
        "old unusable command never reinterprets now available output"
    );
    assert_initial_calls(h);
}

async fn continue_genuine(h: &mut MixedTruth) {
    let before = all_tables(&h.fixture).await;
    let retired = h.requests[0].fence;
    new_claim(&h.fixture, &mut h.requests[0]).await;
    let fence = h.requests[0].fence;
    assert_eq!(fence.scope, retired.scope);
    assert_eq!(fence.attempt.0, retired.attempt.0 + 1);
    assert_ne!(fence.generation, retired.generation);
    for request in &mut h.requests[1..] {
        request.fence = fence;
    }
    let claimed = all_tables(&h.fixture).await;
    claim_boundary(&before, &claimed, fence);
    Box::pin(live_recovery(h, true)).await;
    let applied = Box::pin(dispatch_genuine(h, &claimed)).await;
    Box::pin(complete_genuine(h, &applied, fence)).await;
}

async fn dispatch_genuine(h: &MixedTruth, claimed: &Value) -> Value {
    let service = ActionService::new(mixed_writer(&h.fixture));
    let saved = LedgerProvider::new(&h.fixture, Delivery::Pending);
    for request in &h.requests[..2] {
        let RemoteDispatchObservation::Committed(receipt) =
            Box::pin(service.dispatch_remote(request, &saved, &CancellationToken::new()))
                .await
                .unwrap()
        else {
            panic!("canonical genuine recovered receipt");
        };
        assert_eq!(receipt.result, good());
    }
    assert_eq!(saved.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        all_tables(&h.fixture).await,
        *claimed,
        "receipt-only siblings append no entry and poll no provider"
    );
    let supported = RegisteredLedger::new(
        &h.fixture,
        &h.requests[2],
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    )
    .await;
    let RemoteDispatchObservation::Committed(receipt) =
        Box::pin(service.dispatch_remote(&h.requests[2], &supported, &CancellationToken::new()))
            .await
            .unwrap()
    else {
        panic!("supported sibling genuine completion");
    };
    assert_eq!(receipt.result, good());
    assert_eq!(supported.inner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&h.fixture).await, 3);
    assert_eq!(entry_consumptions(&h.fixture).await, (4, 0));
    let applied = all_tables(&h.fixture).await;
    supported_boundary(claimed, &applied, &h.requests[2]);
    let digest_matches: bool = sqlx::query_scalar("SELECT result_digest=encode(sha256(jsonb_send($2::jsonb)),'hex') FROM workflow_action_actual_receipt_observations WHERE invocation_id=$1")
        .bind(h.requests[2].subject.invocation.as_uuid()).bind(good())
        .fetch_one(h.fixture.persistence().pool()).await.unwrap();
    assert!(
        digest_matches,
        "actual observation digest binds the authentic good result"
    );
    applied
}

async fn complete_genuine(h: &MixedTruth, applied: &Value, fence: WorkflowFence) {
    let committed = h
        .fixture
        .persistence()
        .complete_io(FencedWorkflowResult {
            fence,
            output: good(),
        })
        .await
        .unwrap()
        .unwrap();
    assert!(committed.successor.is_none());
    let done = all_tables(&h.fixture).await;
    completed_boundary(applied, &done, fence);
    Box::pin(completed_replay(h, &done, fence)).await;
}

async fn completed_replay(h: &MixedTruth, done: &Value, fence: WorkflowFence) {
    let service = ActionService::new(mixed_writer(&h.fixture));
    let saved = LedgerProvider::new(&h.fixture, Delivery::Pending);
    let replay = h
        .fixture
        .persistence()
        .complete_io(FencedWorkflowResult {
            fence,
            output: good(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replay.output, good());
    assert!(replay.successor.is_none());
    // Shared completion returns historical truth. Remote receipt return still
    // requires a live lease, so terminal dispatch cannot become a new I/O grant.
    for request in &h.requests {
        assert!(matches!(
            Box::pin(service.dispatch_remote(request, &saved, &CancellationToken::new())).await,
            Err(crate::application::app_error::AppError::Conflict(_))
        ));
    }
    assert!(
        h.fixture
            .persistence()
            .claim_io(fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(saved.calls.load(Ordering::SeqCst), 0);

    assert_eq!(
        *done,
        all_tables(&h.fixture).await,
        "completed receipt replay remains historical truth"
    );
    assert_initial_calls(h);
}

fn assert_initial_calls(h: &MixedTruth) {
    assert_eq!(h.first.calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.second.calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.supported.inner.calls.load(Ordering::SeqCst), 1);
}

async fn case(unusable: UnusableResult) {
    // Box genuine fixture/provider/settlement seams to retain stock 2 MiB test stacks.
    let mut h = Box::pin(seeded(unusable)).await;
    let command = Box::pin(record_live(&h)).await;
    Box::pin(blocked(&h)).await;
    Box::pin(recover_same_result(&h, &command)).await;
    Box::pin(continue_genuine(&mut h)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_runtime_mixed_missing_then_same_genuine_result() {
    Box::pin(case(UnusableResult::Missing)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_runtime_mixed_invalid_then_same_genuine_result() {
    Box::pin(case(UnusableResult::Invalid)).await;
}
