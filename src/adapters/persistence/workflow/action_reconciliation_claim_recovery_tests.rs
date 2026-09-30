//! Lost claim acknowledgements recover normally; exhaustion cannot erase accepted work.
use super::*;
use crate::application::workflow::lease::{FencedWorkflowResult, WorkflowLeaseRecovery};

#[derive(Clone, Copy)]
enum ClockBoundary {
    Lease,
    Retry,
}

async fn wait_for_clock(s: &Scheduled, boundary: ClockBoundary) {
    let query = match boundary {
        ClockBoundary::Lease => {
            "SELECT lock_expires_at<=clock_timestamp() FROM background_tasks WHERE id=$1"
        }
        ClockBoundary::Retry => {
            "SELECT run_at<=clock_timestamp() FROM background_tasks WHERE id=$1"
        }
    };
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let reached: bool = sqlx::query_scalar(query)
                .bind(s.request.fence.scope.job.0)
                .fetch_one(s.fixture.persistence().pool())
                .await
                .unwrap();
            if reached {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("database clock must reach the unchanged lease/backoff boundary");
}

async fn pending_episode(s: &Scheduled) -> Option<String> {
    let scope = s.request.fence.scope;
    sqlx::query_scalar("SELECT workflow_action_pending_claim_episode($1,$2,$3,$4)")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(scope.execution.as_uuid())
        .bind(scope.job.0)
        .fetch_one(s.fixture.persistence().pool())
        .await
        .unwrap()
}

fn subject_attempt(state: &Value, s: &Scheduled, ordinal: i32) -> Value {
    let rows: Vec<_> = state["task_attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["task_id"] == json!(s.request.fence.scope.job.0)
                && row["attempt_number"] == json!(ordinal)
        })
        .collect();
    assert_eq!(rows.len(), 1, "one real scoped attempt {ordinal}");
    rows[0].clone()
}

async fn exhaust_sibling(s: &Scheduled) {
    let scope = child(&s.fixture, s.request.fence.scope).await;
    let fence = s
        .fixture
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap()
        .fence;
    debit(&s.fixture, fence).await;
    assert_eq!(
        s.fixture
            .persistence()
            .reserve_budget(
                BudgetReservation::new(
                    fence,
                    BudgetReservationKey::parse("claim-recovery-real-exhaustion").unwrap(),
                    BudgetCharge::new(BudgetResource::ModelCall, 1).unwrap(),
                )
                .unwrap(),
                policy(),
            )
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Exhausted)
    );
    let state = all_tables(&s.fixture).await;
    assert_eq!(state["workflow_root_budget_usage"][0]["model_calls"], 2);
    assert_eq!(state["workflow_root_budgets"][0]["model_calls"], 2);
    assert!(
        state["workflow_budget_receipts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["run_id"] == json!(scope.run.as_uuid())
                && row["disposition"] == "exhausted")
    );
    assert!(!eligible(&s.fixture, s.request.fence.scope).await);
    assert!(pending_episode(s).await.is_none());
}

async fn finish_accepted(s: &Scheduled, receipt: ActionReceipt) {
    let before = all_tables(&s.fixture).await;
    let committed = s
        .fixture
        .persistence()
        .complete_io(FencedWorkflowResult {
            fence: s.request.fence,
            output: receipt.result.clone(),
        })
        .await
        .unwrap()
        .expect("already accepted live result completes after sibling exhaustion");
    assert_eq!(committed.output, good());
    let after = all_tables(&s.fixture).await;
    unchanged(
        &before,
        &after,
        &[
            "workflow_root_budget_usage",
            "workflow_budget_receipts",
            "workflow_action_claim_episodes",
            "workflow_action_claim_budget_refusals",
            "workflow_action_receipts",
            "workflow_action_remote_entries",
            "workflow_action_evidence_consumptions",
        ],
    );
    let execution = after["workflow_executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(s.request.scope().execution.as_uuid()))
        .unwrap();
    assert_eq!(execution["committed_output"], good());
    assert_ne!(execution["completed_at"], Value::Null);
    assert_eq!(
        subject_attempt(&after, s, s.request.fence.attempt.0)["status"],
        "completed"
    );
    assert_eq!(entry_consumptions(&s.fixture).await, (2, 1));
    assert_eq!(effects(&s.fixture).await, 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_recovery_lost_response_automatic_saved_retry() {
    // Box real fixture/service seams to preserve stock 2 MiB test stacks.
    let (f, request) = Box::pin(limited()).await;
    reserve(&f, request.fence, BudgetResource::ModelCall, 1).await;
    let mut s = Box::pin(schedule(f, request, Delivery::Pending)).await;
    let before = all_tables(&s.fixture).await;
    assert_eq!(
        pending_episode(&s).await,
        Some(s.command.command_key.as_str().into())
    );
    let lost_worker = worker();
    // The API commits successfully, then its returned activation/fence is discarded.
    assert!(
        s.fixture
            .persistence()
            .claim_io(s.request.fence.scope, lost_worker, policy())
            .await
            .unwrap()
            .is_some()
    );
    let installed = all_tables(&s.fixture).await;
    unchanged(&before, &installed, CLAIM_SAVED_FACTS);
    let lost = subject_attempt(&installed, &s, 2);
    assert_eq!(lost["status"], "processing");
    assert_eq!(lost["worker_id"], json!(lost_worker.0));
    assert!(pending_episode(&s).await.is_none());
    assert!(
        claims(&s).await.is_empty(),
        "live lost claim cannot be stolen"
    );
    assert_eq!(installed, all_tables(&s.fixture).await);
    let descendant = child(&s.fixture, s.request.fence.scope).await;
    let sibling = s
        .fixture
        .persistence()
        .claim_io(descendant, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    reserve(&s.fixture, sibling.fence, BudgetResource::ModelCall, 1).await;
    let charged = all_tables(&s.fixture).await;
    assert_eq!(charged["workflow_root_budget_usage"][0]["model_calls"], 2);
    Box::pin(recover_discarded(&mut s, charged, lost)).await;
}

async fn recover_discarded(s: &mut Scheduled, charged: Value, lost: Value) {
    Box::pin(wait_for_clock(s, ClockBoundary::Lease)).await;
    let (a, b) = tokio::join!(
        s.fixture
            .persistence()
            .retire_expired_io(s.request.fence.scope, policy()),
        s.fixture
            .persistence()
            .retire_expired_io(s.request.fence.scope, policy()),
    );
    assert_eq!(usize::from(a.unwrap()) + usize::from(b.unwrap()), 1);
    let retired = all_tables(&s.fixture).await;
    unchanged(&charged, &retired, CLAIM_SAVED_FACTS);
    let attempt = subject_attempt(&retired, s, 2);
    assert_eq!(attempt["status"], "failed");
    assert_eq!(attempt["workflow_failure_code"], "workflow.lease_expired");
    assert_eq!(attempt["workflow_retry_safety"], "safe");
    assert_eq!(attempt["workflow_retirement"], "expired");
    assert_eq!(
        subject_attempt(&charged, s, 1),
        subject_attempt(&retired, s, 1)
    );
    assert!(pending_episode(s).await.is_none());
    assert!(
        claims(s).await.is_empty(),
        "engine backoff blocks immediate hot retry"
    );
    assert_eq!(retired, all_tables(&s.fixture).await);
    Box::pin(wait_for_clock(s, ClockBoundary::Retry)).await;
    let next = claims(s).await;
    assert_eq!(
        next.len(),
        1,
        "ordinary automatic retry survives model equality"
    );
    s.request.fence = next[0];
    assert_eq!(s.request.fence.attempt.0, 3);
    assert_ne!(
        json!(s.request.fence.generation.0),
        lost["execution_generation"]
    );
    let recovered = all_tables(&s.fixture).await;
    unchanged(&retired, &recovered, CLAIM_SAVED_FACTS);
    assert_eq!(
        subject_attempt(&retired, s, 2),
        subject_attempt(&recovered, s, 2)
    );
    assert_eq!(
        s.fixture
            .persistence()
            .reserve_budget(
                BudgetReservation::new(
                    s.request.fence,
                    BudgetReservationKey::parse("claim-boundary-logical-work").unwrap(),
                    BudgetCharge::new(BudgetResource::ModelCall, 1).unwrap(),
                )
                .unwrap(),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::Replayed(BudgetDisposition::Granted)
    );
    assert_eq!(recovered, all_tables(&s.fixture).await);
    assert_eq!(entry_consumptions(&s.fixture).await, (1, 0));
    assert_eq!(effects(&s.fixture).await, 0);
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_recovery_late_receipt_after_exhaustion() {
    for timing in [ReceiptTiming::Live, ReceiptTiming::Expired] {
        Box::pin(late_receipt(timing)).await;
    }
}

#[derive(Clone, Copy)]
enum ReceiptTiming {
    Live,
    Expired,
}

async fn late_receipt(timing: ReceiptTiming) {
    let (f, request) = Box::pin(limited()).await;
    let mut s = Box::pin(schedule(f, request, Delivery::Pending)).await;
    new_claim(&s.fixture, &mut s.request).await;
    let writer = adapter(&s.fixture, 0);
    let RemoteReservationResult::Reserved(reserved) =
        writer.reserve_remote(&s.request, None).await.unwrap()
    else {
        panic!("one genuine proof entry");
    };
    let entered = writer.enter_remote(*reserved).await.unwrap();
    let provider = LedgerProvider::new(
        &s.fixture,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    let result = provider
        .invoke(&entered.action, &entered.provider)
        .await
        .unwrap();
    assert_eq!(entry_consumptions(&s.fixture).await, (2, 1));
    Box::pin(exhaust_sibling(&s)).await;
    if matches!(timing, ReceiptTiming::Expired) {
        Box::pin(wait_for_clock(&s, ClockBoundary::Lease)).await;
        assert!(
            s.fixture
                .persistence()
                .retire_expired_io(s.request.fence.scope, policy())
                .await
                .unwrap()
        );
    }
    let before = all_tables(&s.fixture).await;
    assert_eq!(before["workflow_action_receipts"], json!([]));
    let result = writer.finish_remote(entered, result).await.unwrap();
    let after = all_tables(&s.fixture).await;
    assert_eq!(
        after["workflow_action_receipts"].as_array().unwrap().len(),
        1
    );
    assert_eq!(after["workflow_action_receipts"][0]["result"], good());
    unchanged(
        &before,
        &after,
        &[
            "workflow_root_budget_usage",
            "workflow_budget_receipts",
            "workflow_action_claim_episodes",
            "workflow_action_claim_budget_refusals",
            "workflow_executions",
            "background_tasks",
            "task_attempts",
        ],
    );
    Box::pin(accepted_observation(&s, timing, result, after)).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&s.fixture).await, 1);
}

async fn accepted_observation(
    s: &Scheduled,
    timing: ReceiptTiming,
    observation: RemoteDispatchObservation,
    after: Value,
) {
    match observation {
        RemoteDispatchObservation::Committed(receipt) => {
            assert!(matches!(timing, ReceiptTiming::Live));
            Box::pin(finish_accepted(s, receipt)).await;
        }
        RemoteDispatchObservation::Interrupted => {
            assert!(matches!(timing, ReceiptTiming::Expired));
            assert!(
                s.fixture
                    .persistence()
                    .complete_io(FencedWorkflowResult {
                        fence: s.request.fence,
                        output: good(),
                    })
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(after, all_tables(&s.fixture).await);
            let executions: Vec<_> = after["workflow_executions"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["id"] == json!(s.request.scope().execution.as_uuid()))
                .collect();
            assert_eq!(executions.len(), 1);
            assert_eq!(executions[0]["completed_at"], Value::Null);
            assert_eq!(executions[0]["committed_output"], Value::Null);
            assert_eq!(executions[0]["successor_execution_id"], Value::Null);
        }
        _ => panic!("accepted actual result must be retained"),
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_recovery_returned_result_completes_after_exhaustion()
{
    let (f, request) = Box::pin(limited()).await;
    let mut s = Box::pin(schedule(f, request, Delivery::Pending)).await;
    new_claim(&s.fixture, &mut s.request).await;
    let provider = LedgerProvider::new(
        &s.fixture,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    let RemoteDispatchObservation::Committed(receipt) =
        invoke(&s.fixture, &s.request, &provider).await.unwrap()
    else {
        panic!("result returned to genuine live claimant");
    };
    let accepted = all_tables(&s.fixture).await;
    Box::pin(exhaust_sibling(&s)).await;
    let exhausted = all_tables(&s.fixture).await;
    unchanged(
        &accepted,
        &exhausted,
        &[
            "workflow_action_receipts",
            "workflow_action_remote_entries",
            "workflow_action_evidence_consumptions",
            "workflow_action_claim_episodes",
            "workflow_action_claim_budget_refusals",
        ],
    );
    let execution_rows = |state: &Value| {
        let rows: Vec<Value> = state["workflow_executions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["id"] == json!(s.request.scope().execution.as_uuid()))
            .cloned()
            .collect();
        assert_eq!(rows.len(), 1, "one genuine scoped parent execution");
        rows
    };
    assert_eq!(execution_rows(&accepted), execution_rows(&exhausted));
    Box::pin(finish_accepted(&s, receipt)).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
