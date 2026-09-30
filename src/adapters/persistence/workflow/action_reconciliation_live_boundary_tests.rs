//! Genuine consuming entries remain live when an old actual response breaches finality.
use super::*;
use tokio::sync::Notify;

#[path = "action_reconciliation_completion_race_tests.rs"]
mod completion_race_tests;

async fn live_retry() -> HeldTruth {
    let mut h = Box::pin(held_truth()).await;
    Box::pin(schedule_final(&h, "live-boundary-final")).await;
    let old = h.request.fence;
    new_claim(&h.fixture, &mut h.request).await;
    assert_eq!(h.request.fence.scope, old.scope);
    assert_ne!(h.request.fence.worker, old.worker);
    assert_ne!(h.request.fence.generation, old.generation);
    assert_eq!(h.request.fence.attempt.0, old.attempt.0 + 1);
    assert_eq!(h.entry.as_ref().unwrap().request.fence, old);
    h
}

async fn live_snapshot(h: &HeldTruth) -> Value {
    assert!(
        h.fixture
            .persistence()
            .validate_io(h.request.fence, policy())
            .await
            .unwrap()
    );
    let state = all_tables(&h.fixture).await;
    let fence = h.request.fence;
    assert_eq!(state["workflow_runs"].as_array().unwrap().len(), 1);
    let job = &state["background_tasks"][0];
    assert_eq!(state["background_tasks"].as_array().unwrap().len(), 1);
    assert_eq!(job["id"], json!(fence.scope.job.0));
    assert_eq!(job["status"], "processing");
    assert_eq!(job["worker_id"], json!(fence.worker.0));
    assert_eq!(job["execution_generation"], json!(fence.generation.0));
    assert_eq!(job["retry_count"], json!(fence.attempt.0 - 1));
    assert_eq!(state["workflow_runs"][0]["state"], "running");
    assert!(state["workflow_runs"][0]["waiting_reason"].is_null());
    let attempts = state["task_attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    let current = attempts
        .iter()
        .find(|row| row["attempt_number"] == json!(fence.attempt.0))
        .unwrap();
    assert_eq!(current["status"], "processing");
    assert_eq!(current["worker_id"], json!(fence.worker.0));
    assert_eq!(current["execution_generation"], json!(fence.generation.0));
    assert!(current["finished_at"].is_null());
    for field in [
        "completed_at",
        "committed_output",
        "committed_route",
        "successor_execution_id",
    ] {
        assert!(
            state["workflow_executions"][0][field].is_null(),
            "no advancement {field}"
        );
    }
    assert_eq!(entry_consumptions(&h.fixture).await, (2, 1));
    let consumption = &state["workflow_action_evidence_consumptions"][0];
    let new = state["workflow_action_remote_entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == consumption["remote_entry_id"])
        .unwrap();
    assert_ne!(new["id"], json!(h.entry_id));
    assert_eq!(new["worker_id"], json!(fence.worker.0));
    assert_eq!(new["execution_generation"], json!(fence.generation.0));
    let coverage = state["workflow_action_evidence_coverage"]
        .as_array()
        .unwrap();
    assert_eq!(coverage.len(), 1);
    assert_eq!(coverage[0]["remote_entry_id"], json!(h.entry_id));
    assert_eq!(coverage[0]["evidence_id"], consumption["evidence_id"]);
    assert_ne!(coverage[0]["remote_entry_id"], new["id"]);
    state
}

fn same_except(before: &Value, after: &Value, fields: &[&str]) {
    let mut normalized = after.clone();
    for field in fields {
        normalized[*field] = before[*field].clone();
    }
    assert_eq!(*before, normalized);
}

fn same_table_except(before: &Value, after: &Value, changed_id: &Value, fields: &[&str]) {
    let prior = before.as_array().unwrap();
    let current = after.as_array().unwrap();
    assert_eq!(prior.len(), current.len(), "preserved table inventory");
    assert_eq!(
        prior.iter().filter(|row| row["id"] == *changed_id).count(),
        1
    );
    for old in prior {
        let row = current.iter().find(|row| row["id"] == old["id"]).unwrap();
        if old["id"] == *changed_id {
            same_except(old, row, fields);
        } else {
            assert_eq!(old, row, "unrelated row preserved");
        }
    }
}

async fn conflicting_old_receipt(h: &mut HeldTruth, before: &Value) -> Value {
    assert!(matches!(
        adapter(&h.fixture, 0)
            .finish_remote(h.entry.take().unwrap(), h.result.clone())
            .await
            .unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    let after = live_snapshot(h).await;
    unchanged_tables(
        before,
        &after,
        &[
            "workflow_runs",
            "workflow_action_receipts",
            "workflow_action_actual_receipt_observations",
            "workflow_action_evidence_conflicts",
            "workflow_action_effect_states",
        ],
    );
    same_table_except(
        &before["workflow_runs"],
        &after["workflow_runs"],
        &json!(h.request.fence.scope.run.as_uuid()),
        &["revision"],
    );
    assert!(
        after["workflow_runs"][0]["revision"].as_i64().unwrap()
            > before["workflow_runs"][0]["revision"].as_i64().unwrap()
    );
    assert_eq!(counts(&h.fixture).await, (1, 1, 1));
    let receipt = &after["workflow_action_receipts"][0];
    let observation = &after["workflow_action_actual_receipt_observations"][0];
    let conflict = &after["workflow_action_evidence_conflicts"][0];
    assert_eq!(receipt["remote_entry_id"], json!(h.entry_id));
    assert_eq!(receipt["result"], h.result);
    assert!(receipt["reconciliation_evidence_id"].is_null());
    assert_eq!(observation["remote_entry_id"], json!(h.entry_id));
    assert_eq!(observation["result"], h.result);
    assert_eq!(conflict["actual_observation_id"], observation["id"]);
    assert_eq!(conflict["remote_entry_id"], json!(h.entry_id));
    assert_eq!(
        conflict["evidence_id"],
        before["workflow_action_evidence"][0]["id"]
    );
    assert_eq!(conflict["reason"], "finality_breach");
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
        "dispatch_id",
    ] {
        assert_eq!(
            conflict[field],
            before["workflow_action_evidence"][0][field]
        );
        assert_eq!(receipt[field], conflict[field]);
    }
    assert_eq!(projection(&h.fixture).await, ("committed".into(), true));
    after
}

fn append_only(before: &Value, after: &Value, count: usize) {
    let prior = before.as_array().unwrap();
    let current = after.as_array().unwrap();
    assert_eq!(current.len(), prior.len() + count);
    assert!(
        prior.iter().all(|row| current.contains(row)),
        "immutable prior rows retained"
    );
}

async fn retire_live(h: &HeldTruth, before: &Value, effect_count: i64) {
    assert!(
        h.fixture
            .persistence()
            .release_io(
                h.request.fence,
                policy(),
                LeaseReleaseCause::LocalInterruption
            )
            .await
            .unwrap()
    );
    let after = all_tables(&h.fixture).await;
    unchanged_tables(
        before,
        &after,
        &[
            "workflow_runs",
            "background_tasks",
            "task_attempts",
            "workflow_run_events",
            "workflow_action_state_witnesses",
        ],
    );
    let run = &after["workflow_runs"][0];
    assert_eq!(run["state"], "waiting");
    assert_eq!(run["waiting_reason"], "reconciliation");
    same_table_except(
        &before["workflow_runs"],
        &after["workflow_runs"],
        &json!(h.request.fence.scope.run.as_uuid()),
        &["state", "waiting_reason", "revision"],
    );
    let job = &after["background_tasks"][0];
    assert_eq!(job["status"], "failed");
    assert_eq!(job["retry_count"], json!(h.request.fence.attempt.0));
    for field in [
        "worker_id",
        "execution_generation",
        "locked_at",
        "lock_expires_at",
    ] {
        assert!(job[field].is_null());
    }
    same_table_except(
        &before["background_tasks"],
        &after["background_tasks"],
        &json!(h.request.fence.scope.job.0),
        &[
            "status",
            "retry_count",
            "worker_id",
            "execution_generation",
            "locked_at",
            "lock_expires_at",
            "updated_at",
        ],
    );
    retired_attempts(h, before, &after);
    append_only(
        &before["workflow_run_events"],
        &after["workflow_run_events"],
        1,
    );
    state_witnesses(before, &after);
    assert_eq!(effects(&h.fixture).await, effect_count);
    Box::pin(inert_live(h, &after, effect_count)).await;
}

fn retired_attempts(h: &HeldTruth, before: &Value, after: &Value) {
    let prior = before["task_attempts"].as_array().unwrap();
    let current = after["task_attempts"].as_array().unwrap();
    assert_eq!(current.len(), prior.len());
    for old in prior {
        let row = current.iter().find(|row| row["id"] == old["id"]).unwrap();
        if old["attempt_number"] != json!(h.request.fence.attempt.0) {
            assert_eq!(row, old);
            continue;
        }
        assert_eq!(row["status"], "failed");
        assert!(!row["finished_at"].is_null());
        assert_eq!(row["workflow_failure_class"], "retryable");
        assert_eq!(row["workflow_failure_code"], "workflow.interrupted");
        assert_eq!(row["workflow_retry_safety"], "unknown");
        assert_eq!(row["workflow_retirement"], "live");
        same_except(
            old,
            row,
            &[
                "status",
                "finished_at",
                "workflow_failure_class",
                "workflow_failure_code",
                "workflow_retry_safety",
                "workflow_retirement",
            ],
        );
    }
}

async fn inert_live(h: &HeldTruth, saved: &Value, effect_count: i64) {
    assert!(!retry_safe(&h.fixture, &h.request).await);
    let (a, b) = tokio::join!(
        h.fixture
            .persistence()
            .claim_io(h.request.fence.scope, worker(), policy()),
        h.fixture
            .persistence()
            .claim_io(h.request.fence.scope, worker(), policy())
    );
    assert!(a.unwrap().is_none() && b.unwrap().is_none());
    assert!(
        !h.fixture
            .persistence()
            .validate_io(h.request.fence, policy())
            .await
            .unwrap()
    );
    assert!(
        !h.fixture
            .persistence()
            .release_io(
                h.request.fence,
                policy(),
                LeaseReleaseCause::LocalInterruption
            )
            .await
            .unwrap()
    );
    let provider = LedgerProvider::new(&h.fixture, Delivery::Pending);
    assert!(
        Box::pin(invoke(&h.fixture, &h.request, &provider))
            .await
            .is_err()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert!(
        h.fixture
            .persistence()
            .complete_io(FencedWorkflowResult {
                fence: h.request.fence,
                output: h.result.clone()
            })
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(*saved, all_tables(&h.fixture).await);
    assert_eq!(effects(&h.fixture).await, effect_count);
}

async fn entered_result(h: &HeldTruth) -> (RemoteEntry, Value) {
    let writer = adapter(&h.fixture, 0);
    let RemoteReservationResult::Reserved(reserved) =
        writer.reserve_remote(&h.request, None).await.unwrap()
    else {
        panic!("new consuming entry");
    };
    let entry = writer.enter_remote(*reserved).await.unwrap();
    assert_eq!(entry.request.fence, h.request.fence);
    let provider = LedgerProvider::new(
        &h.fixture,
        Delivery::Apply {
            result: good(),
            recover: false,
            lose: false,
        },
    );
    let result = Box::pin(provider.invoke(&entry.action, &entry.provider))
        .await
        .unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        effects(&h.fixture).await,
        2,
        "both genuine provider effects retained"
    );
    (entry, result)
}

#[tokio::test]
async fn workflow_action_reconciliation_live_return_without_conflict_commits() {
    let h = Box::pin(live_retry()).await;
    let (entry, result) = Box::pin(entered_result(&h)).await;
    let entry_id = entry.entry;
    let before = live_snapshot(&h).await;
    assert!(
        before["workflow_action_evidence_conflicts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        adapter(&h.fixture, 0)
            .finish_remote(entry, result.clone())
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    let returned = live_snapshot(&h).await;
    assert_eq!(returned["workflow_action_receipts"][0]["result"], result);
    assert_eq!(
        returned["workflow_action_receipts"][0]["remote_entry_id"],
        json!(entry_id)
    );
    assert!(
        returned["workflow_action_evidence_conflicts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(counts(&h.fixture).await, (1, 1, 0));
    assert!(
        h.fixture
            .persistence()
            .complete_io(FencedWorkflowResult {
                fence: h.request.fence,
                output: result
            })
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(effects(&h.fixture).await, 2);
}

#[tokio::test]
async fn workflow_action_reconciliation_live_return_conflict_vetoes_genuine_new_result() {
    let mut h = Box::pin(live_retry()).await;
    let (entry, result) = Box::pin(entered_result(&h)).await;
    let entry_id = entry.entry;
    let before = live_snapshot(&h).await;
    let conflicted = Box::pin(conflicting_old_receipt(&mut h, &before)).await;
    assert!(matches!(
        adapter(&h.fixture, 0)
            .finish_remote(entry, result.clone())
            .await
            .unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    let returned = live_snapshot(&h).await;
    unchanged_tables(
        &conflicted,
        &returned,
        &[
            "workflow_runs",
            "workflow_action_actual_receipt_observations",
        ],
    );
    same_table_except(
        &conflicted["workflow_runs"],
        &returned["workflow_runs"],
        &json!(h.request.fence.scope.run.as_uuid()),
        &["revision"],
    );
    append_only(
        &conflicted["workflow_action_actual_receipt_observations"],
        &returned["workflow_action_actual_receipt_observations"],
        1,
    );
    assert_eq!(counts(&h.fixture).await, (1, 2, 1));
    let new = returned["workflow_action_actual_receipt_observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["remote_entry_id"] != json!(h.entry_id))
        .unwrap();
    assert_eq!(new["result"], result);
    assert_eq!(new["remote_entry_id"], json!(entry_id));
    Box::pin(retire_live(&h, &returned, 2)).await;
}

struct PendingEffect {
    ledger: LedgerProvider,
    started: Notify,
    release: Notify,
    dropped: AtomicUsize,
    finished: AtomicUsize,
}
struct PendingDrop<'a>(&'a AtomicUsize);
impl Drop for PendingDrop<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl RemoteAction for PendingEffect {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(
        &self,
        action: &FrozenAction,
        invocation: &ProviderInvocation,
    ) -> AppResult<Value> {
        let _drop = PendingDrop(&self.dropped);
        assert!(matches!(
            self.ledger.invoke(action, invocation).await,
            Err(AppError::Timeout(_))
        ));
        self.started.notify_one();
        self.release.notified().await;
        let mut tx = self.ledger.persistence.pool().begin().await?;
        let entry: Uuid = sqlx::query_scalar("SELECT ledger.entry_id FROM fixture_evidence_ledger AS ledger JOIN workflow_action_intents AS intent ON intent.id=ledger.invocation_id WHERE intent.company_id=$1 AND intent.execution_id=$2 AND intent.argument_digest=$3 ORDER BY ledger.observed_at DESC LIMIT 1")
            .bind(action.scope().company.as_uuid()).bind(action.scope().execution.as_uuid()).bind(action.argument_digest().as_str()).fetch_one(&mut *tx).await?;
        assert!(apply_on(&mut tx, entry, &good(), true).await?);
        tx.commit().await?;
        self.finished.fetch_add(1, Ordering::SeqCst);
        Ok(good())
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_live_remote_heartbeat_drops_pending_provider_on_conflict() {
    let mut h = Box::pin(live_retry()).await;
    let provider = PendingEffect {
        ledger: LedgerProvider::new(&h.fixture, Delivery::Pending),
        started: Notify::new(),
        release: Notify::new(),
        dropped: AtomicUsize::new(0),
        finished: AtomicUsize::new(0),
    };
    let request = h.request.clone();
    let service = ActionService::new(adapter(&h.fixture, 0));
    let cancel = CancellationToken::new();
    let dispatch = Box::pin(service.dispatch_remote(&request, &provider, &cancel));
    let conflict = async {
        provider.started.notified().await;
        assert_eq!(provider.ledger.calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.dropped.load(Ordering::SeqCst), 0);
        let before = live_snapshot(&h).await;
        Box::pin(conflicting_old_receipt(&mut h, &before)).await
    };
    let (outcome, conflicted) = tokio::time::timeout(Duration::from_millis(1900), async {
        tokio::join!(dispatch, conflict)
    })
    .await
    .expect("conflict heartbeat interrupts before the three-second live lease expires");
    assert!(matches!(
        outcome.unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    assert!(!cancel.is_cancelled());
    assert_eq!(provider.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(provider.finished.load(Ordering::SeqCst), 0);
    assert_eq!(provider.ledger.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        conflicted,
        live_snapshot(&h).await,
        "heartbeat interrupts without stealing the live attempt"
    );
    provider.release.notify_one();
    tokio::task::yield_now().await;
    assert_eq!(provider.finished.load(Ordering::SeqCst), 0);
    assert_eq!(
        conflicted,
        all_tables(&h.fixture).await,
        "dropped provider cannot add an effect or receipt after release"
    );
    Box::pin(retire_live(&h, &conflicted, 1)).await;
}
