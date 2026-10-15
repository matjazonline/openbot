//! 04.8 group B2: actual receipt settlement against an actual CancelCommand in both orders, with
//! the exact run blockers observed (DEC-002/004); the observed-result token case; and the
//! ordinary cancelled late receipt redelivered and retried from a fresh service.
use super::*;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::cancellation_order_tests::{
    RunLock, assert_drained, blocked, probe_run,
};
use crate::adapters::persistence::workflow::persistence_bound::recorder::{Injected, Observed};
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{
    Gate, HANG_GUARD, Stall, connect, gated_policy, resolution_behind,
};
use std::sync::atomic::AtomicBool;

/// A real reservation, real final entry and the provider's real accepted effect, with the
/// observation not yet settled. `spare` is the same frozen action, read while the run was live,
/// for redelivering this exact entry later.
struct Accepted<'a> {
    request: ActionDispatchRequest,
    writer: Dispatch,
    marker: (Uuid, Uuid),
    result: Value,
    spare: FrozenAction,
    provider: DeduplicatingProvider<'a>,
}

async fn accepted(
    f: &AdmissionFixture,
    mut request: ActionDispatchRequest,
    lease: LeasePolicy,
) -> (Accepted<'_>, RemoteEntry) {
    assert!(
        f.persistence()
            .renew_io(request.fence, lease)
            .await
            .unwrap()
            .is_some()
    );
    request.lease = lease;
    let provider = provider(f);
    let writer = adapter(f, 0);
    let RemoteReservationResult::Reserved(reservation) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("a genuine reservation")
    };
    let entry = writer.enter_remote(*reservation).await.unwrap();
    let result = provider
        .invoke(&entry.action, &entry.provider)
        .await
        .unwrap();
    assert_eq!(effect_count(f).await, 1, "the provider accepted the effect");
    let spare = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    let accepted = Accepted {
        request,
        writer,
        marker: (entry.marker, entry.entry),
        result,
        spare,
        provider,
    };
    (accepted, entry)
}

/// The identical accepted observation delivered again for the exact same entry: same marker,
/// entry, request, frozen action and (Reconcile, unregistered) provider invocation. The real
/// receipt writer re-verifies every identity against durable facts before settling it.
fn redelivery(
    entry: (Uuid, Uuid),
    request: &ActionDispatchRequest,
    action: FrozenAction,
) -> RemoteEntry {
    let provider = prepare_provider(&action, None, Duration::from_secs(60)).unwrap();
    RemoteEntry {
        marker: entry.0,
        entry: entry.1,
        request: request.clone(),
        action,
        provider,
        deadline: tokio::time::Instant::now() + Duration::from_secs(1),
    }
}

/// A debuggable name for a settlement outcome (the production type has no `Debug`).
fn outcome(result: AppResult<RemoteDispatchObservation>) -> AppResult<&'static str> {
    result.map(|observation| match observation {
        RemoteDispatchObservation::Interrupted => "interrupted",
        RemoteDispatchObservation::Committed(_) => "committed",
        RemoteDispatchObservation::ApprovalRequired => "approval_required",
        RemoteDispatchObservation::PossibleDispatchExists => "possible_dispatch_exists",
    })
}

#[derive(Clone, Copy, Debug)]
enum First {
    Cancel,
    Receipt,
}

/// The settled outcome is the same in both orders except the order witness: cancellation
/// records reconciliation uncertainty only for a marker that had no receipt when it committed.
fn settled_truth(
    state: &Value,
    request: &ActionDispatchRequest,
    entry: Uuid,
    result: &Value,
    first: First,
) {
    let one = |table: &str| {
        let rows = rows(state, table);
        assert_eq!(rows.len(), 1, "{table}");
        rows[0].clone()
    };
    for table in [
        "workflow_action_receipts",
        "workflow_action_actual_receipt_observations",
    ] {
        let row = one(table);
        assert_eq!(row["remote_entry_id"], json!(entry), "{table}");
        assert_eq!(&row["result"], result, "{table}");
        assert_eq!(
            row["invocation_id"],
            json!(request.subject.invocation.as_uuid())
        );
        assert_eq!(
            row["argument_digest"],
            json!(request.subject.argument_digest.as_str())
        );
    }
    assert_eq!(rows(state, "workflow_action_remote_entries").len(), 1);
    assert_eq!(rows(state, "workflow_action_dispatches").len(), 1);
    assert!(rows(state, "workflow_action_evidence_conflicts").is_empty());
    assert_eq!(one("workflow_runs")["state"], "cancelled");
    let attempt = one("task_attempts");
    assert_eq!(attempt["status"], "failed");
    assert_eq!(attempt["workflow_retirement"], "cancel");
    let execution = one("workflow_executions");
    for field in [
        "completed_at",
        "committed_output",
        "committed_route",
        "successor_execution_id",
    ] {
        assert!(
            execution[field].is_null(),
            "no cancelled advancement: {field}"
        );
    }
    let command = one("workflow_control_commands");
    assert_eq!(command["operation"], "cancel");
    assert_eq!(command["result"], "applied");
    let reconciliations = rows(state, "workflow_action_reconciliations").len();
    match first {
        First::Cancel => assert_eq!(reconciliations, 1, "marker was unresolved when cancelled"),
        First::Receipt => assert_eq!(reconciliations, 0, "receipt preceded cancellation"),
    }
}

/// After durable cancellation and late settlement: the identical observation redelivered and a
/// fresh service retry add no effect, entry, receipt, observation or conflict, poll nothing,
/// and advance nothing.
async fn late_truth_is_final(f: &AdmissionFixture, accepted: Accepted<'_>) {
    let settled = all_tables(f).await;
    let duplicate = redelivery(accepted.marker, &accepted.request, accepted.spare);
    assert!(matches!(
        accepted
            .writer
            .finish_remote(duplicate, accepted.result.clone())
            .await
            .unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    assert_eq!(
        all_tables(f).await,
        settled,
        "identical redelivery is idempotent"
    );
    refused(
        ActionService::new(adapter(f, 0))
            .dispatch_remote(
                &accepted.request,
                &accepted.provider,
                &CancellationToken::new(),
            )
            .await
            .map(|_| ()),
    );
    assert_eq!(accepted.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effect_count(f).await, 1);
    inert(f, &accepted.request, &settled).await;
}

/// DEC-002 cancel-first: actual cancellation stops at its execution gate owning company and
/// run; the actual receipt settlement then waits on that exact run behind the cancel backend.
#[tokio::test]
async fn workflow_action_cancellation_receipt_race_cancel_first() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let (accepted, entry) = accepted(&f, request, policy()).await;
    let request = accepted.request.clone();
    let command = cancel_command(&f, &request, "b2-cancel-first").await;
    {
        let mut gate = f.persistence().pool().begin().await.unwrap();
        let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *gate)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(request.scope().company.as_uuid())
            .bind(request.scope().execution.as_uuid())
            .execute(&mut *gate)
            .await
            .unwrap();
        let mut observer = f.persistence().pool().acquire().await.unwrap();
        probe_run(&mut observer, &request, RunLock::Free).await;
        let cancel = f.persistence().cancel(command);
        let receipt = async {
            outcome(
                accepted
                    .writer
                    .finish_remote(entry, accepted.result.clone())
                    .await,
            )
        };
        tokio::pin!(cancel, receipt);
        let cancel_pid = blocked(
            &mut observer,
            gate_pid,
            "SELECT id FROM workflow_executions",
            cancel.as_mut(),
        )
        .await;
        probe_run(&mut observer, &request, RunLock::Held).await;
        let receipt_pid = tokio::select! {
            result = cancel.as_mut() => panic!("cancel escaped its execution gate: {result:?}"),
            pid = blocked(&mut observer, cancel_pid, "SELECT id FROM workflow_runs WHERE company_id", receipt.as_mut()) => pid,
        };
        assert_ne!(cancel_pid, receipt_pid);
        gate.rollback().await.unwrap();
        let (cancelled, settled) =
            tokio::time::timeout(HANG_GUARD, async { tokio::join!(cancel, receipt) })
                .await
                .expect("both real operations drain");
        assert!(matches!(cancelled.unwrap(), CancelResult::Applied { .. }));
        assert!(
            settled.unwrap() == "interrupted",
            "late truth, no caller success"
        );
        probe_run(&mut observer, &request, RunLock::Free).await;
        assert_drained(&mut observer, cancel_pid, receipt_pid).await;
    }
    settled_truth(
        &all_tables(&f).await,
        &request,
        accepted.marker.1,
        &accepted.result,
        First::Cancel,
    );
    late_truth_is_final(&f, accepted).await;
}

/// F-005 / DEC-012: what decides that receipt-first was the ordinary race, not a recovery.
struct ReceiptFirst {
    /// Every bounded operation the race finished, from the test-only recorder.
    events: Vec<Observed>,
    /// When the COMMIT gate was released (read after the release returned).
    released: tokio::time::Instant,
    /// A conservative primary-attempt deadline: the attempt bound from an instant read before
    /// the settlement future existed, so never later than the real attempt's own deadline.
    deadline: tokio::time::Instant,
}

/// The ordinary receipt-first oracle: exactly one `action.commit_remote` attempt, and it
/// completed (no resolved, adopted, unknown, not-applied or retried attempt), with the gate
/// released before the primary attempt's deadline.
fn ordinary_receipt(race: &ReceiptFirst) -> Result<(), String> {
    let commits: Vec<&Observed> = race
        .events
        .iter()
        .filter(|seen| seen.op == "action.commit_remote")
        .collect();
    if commits.len() != 1 || commits[0].outcome != "completed" {
        return Err(format!(
            "not one completed primary receipt attempt: {commits:?}"
        ));
    }
    if race.released >= race.deadline {
        return Err(format!(
            "gate released {:?} after the primary attempt deadline",
            race.released - race.deadline
        ));
    }
    Ok(())
}

/// DEC-002 receipt-first: the actual receipt transaction stalls inside its COMMIT (the
/// established resolution gate) after writing the observation and receipt while it owns the
/// run; actual cancellation, already holding company, then waits on that exact run.
/// `late` holds the gate until the primary attempt has elapsed and its ambiguous-COMMIT
/// resolution is observed queued on the run: the calibrated negative control.
async fn receipt_first<'a>(
    f: &'a AdmissionFixture,
    request: ActionDispatchRequest,
    injected: &Injected,
    late: bool,
) -> (Accepted<'a>, ReceiptFirst) {
    // DEC-012: the gate policy's 2 s attempt bound, through genuine renewal, for this fixture.
    let policy = gated_policy();
    let (accepted, entry) = accepted(f, request, policy).await;
    let request = accepted.request.clone();
    let command = cancel_command(f, &request, "b2-receipt-first").await;
    let race = {
        let (mut observer, _) = connect(f).await;
        probe_run(&mut observer, &request, RunLock::Free).await;
        let mut gate = Gate::arm(
            f,
            "workflow_action_receipts",
            "INSERT",
            "true",
            Stall::Commit,
        )
        .await;
        let recorded = injected.outcomes().len();
        let done = AtomicBool::new(false);
        let deadline = tokio::time::Instant::now() + policy.attempt_bound();
        let receipt = async {
            let result = outcome(
                accepted
                    .writer
                    .finish_remote(entry, accepted.result.clone())
                    .await,
            );
            done.store(true, Ordering::SeqCst);
            result
        };
        let cancel = f.persistence().cancel(command);
        tokio::pin!(receipt, cancel);
        let running = || !done.load(Ordering::SeqCst);
        let waiting: [&dyn Fn() -> bool; 1] = [&running];
        let receipt_pid = tokio::select! {
            result = receipt.as_mut() => panic!("receipt escaped its COMMIT gate: {result:?}"),
            pid = gate.stalled(&mut observer, &waiting) => pid,
        };
        probe_run(&mut observer, &request, RunLock::Held).await;
        // The cancel's direct run blocker: the stalled receipt, or in the late control the
        // primary attempt's resolution, which queued first on that row and holds its tuple lock.
        let mut run_blocker = receipt_pid;
        if late {
            // The primary attempt has elapsed: its resolution now waits on the stalled run.
            run_blocker = tokio::select! {
                result = receipt.as_mut() => panic!("receipt escaped its COMMIT gate: {result:?}"),
                pid = resolution_behind(&mut observer, receipt_pid, policy, &waiting) => pid,
            };
            eprintln!(
                "late-release control: resolution={run_blocker} behind stalled receipt={receipt_pid}"
            );
        }
        let cancel_pid = tokio::select! {
            result = receipt.as_mut() => panic!("receipt escaped its COMMIT gate: {result:?}"),
            pid = blocked(&mut observer, run_blocker, "FROM workflow_runs AS run", cancel.as_mut()) => pid,
        };
        assert_ne!(cancel_pid, receipt_pid);
        gate.release().await;
        let released = tokio::time::Instant::now();
        let (settled, cancelled) =
            tokio::time::timeout(HANG_GUARD, async { tokio::join!(receipt, cancel) })
                .await
                .expect("both real operations drain");
        assert!(matches!(cancelled.unwrap(), CancelResult::Applied { .. }));
        // Late truth persisted; the caller gets no success once cancellation retired the fence.
        assert_eq!(settled.unwrap(), "interrupted");
        probe_run(&mut observer, &request, RunLock::Free).await;
        assert_drained(&mut observer, receipt_pid, cancel_pid).await;
        ReceiptFirst {
            events: injected.outcomes()[recorded..].to_vec(),
            released,
            deadline,
        }
    };
    eprintln!(
        "receipt-first late={late} released_before_deadline_ms={} events={:?}",
        race.deadline
            .checked_duration_since(race.released)
            .map_or(-1, |margin| margin.as_millis() as i64),
        race.events
    );
    settled_truth(
        &all_tables(f).await,
        &request,
        accepted.marker.1,
        &accepted.result,
        First::Receipt,
    );
    (accepted, race)
}

#[tokio::test]
async fn workflow_action_cancellation_receipt_race_receipt_first() {
    let injected = Injected::mark();
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let (accepted, race) = receipt_first(&f, request, &injected, false).await;
    ordinary_receipt(&race).unwrap();
    late_truth_is_final(&f, accepted).await;
}

/// Calibrated negative control: release only after the primary attempt elapsed and its
/// ambiguous-COMMIT resolution queued. Every business outcome still matches the ordinary race,
/// and the ordinary oracle must reject it.
#[tokio::test]
async fn workflow_action_cancellation_receipt_race_late_release_fails_ordinary_oracle() {
    let injected = Injected::mark();
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let (accepted, race) = receipt_first(&f, request, &injected, true).await;
    let verdict = ordinary_receipt(&race);
    assert!(
        verdict.is_err(),
        "a resolution path is not the ordinary race"
    );
    assert!(
        race.events
            .iter()
            .any(|seen| seen.op == "action.commit_remote" && seen.outcome == "resolved_applied"),
        "the control exercised resolution: {:?}",
        race.events
    );
    eprintln!("late-release control rejected: {}", verdict.unwrap_err());
    late_truth_is_final(&f, accepted).await;
}

/// The oracle alone over constructed recorder events.
#[test]
fn workflow_action_cancellation_receipt_ordinary_oracle_discriminates_attempt_paths() {
    let now = tokio::time::Instant::now();
    let commit = |outcome: &'static str| Observed {
        op: "action.commit_remote",
        outcome,
        cause: None,
    };
    let other = Observed {
        op: "action.return_authorized",
        outcome: "completed",
        cause: None,
    };
    let race = |events: Vec<Observed>, released_ms: u64| ReceiptFirst {
        events,
        released: now + Duration::from_millis(released_ms),
        deadline: now + Duration::from_millis(2000),
    };
    assert!(ordinary_receipt(&race(vec![commit("completed"), other], 100)).is_ok());
    for (case, events, released_ms) in [
        ("resolved applied", vec![commit("resolved_applied")], 100),
        ("resolved adopted", vec![commit("resolved_adopted")], 100),
        ("unknown", vec![commit("unknown")], 100),
        (
            "not applied then retried",
            vec![commit("not_applied"), commit("completed")],
            100,
        ),
        ("no attempt recorded", vec![other], 100),
        ("late release", vec![commit("completed")], 2000),
    ] {
        assert!(
            ordinary_receipt(&race(events, released_ms)).is_err(),
            "{case}"
        );
    }
}

/// The ungated live positive for both races: with no cancellation the same settlement returns
/// the committed receipt, so neither race oracle can be met by a refusal-only implementation.
#[tokio::test]
async fn workflow_action_cancellation_receipt_without_cancel_commits() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let (accepted, entry) = accepted(&f, request, policy()).await;
    let RemoteDispatchObservation::Committed(receipt) = accepted
        .writer
        .finish_remote(entry, accepted.result.clone())
        .await
        .unwrap()
    else {
        panic!("a live owner receives its receipt")
    };
    assert_eq!(receipt.result, accepted.result);
    let state = all_tables(&f).await;
    assert_eq!(rows(&state, "workflow_runs")[0]["state"], "running");
    assert!(rows(&state, "workflow_control_commands").is_empty());
}

/// DEC-004: the token is cancelled at `finish_remote` entry, after the service selected and
/// validated the provider result. Settlement still finishes; only caller success is suppressed.
struct CancelAtSettlement<'a> {
    inner: Dispatch,
    token: &'a CancellationToken,
    observed: &'a std::sync::Mutex<Option<Value>>,
}
#[async_trait]
impl ActionDispatch for CancelAtSettlement<'_> {
    async fn local(&self, request: &ActionDispatchRequest) -> AppResult<LocalDispatchResult> {
        self.inner.local(request).await
    }
    async fn remote(
        &self,
        request: &ActionDispatchRequest,
        provider: &dyn RemoteAction,
        cancel: &CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        supervise_remote(self, request, provider, cancel).await
    }
}
#[async_trait]
impl RemoteDispatch for CancelAtSettlement<'_> {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        self.inner.reserve_remote(request, contract).await
    }
    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry> {
        self.inner.enter_remote(reservation).await
    }
    async fn finish_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation> {
        assert!(
            !self.token.is_cancelled(),
            "the service observed the result first"
        );
        *self.observed.lock().unwrap() = Some(result.clone());
        self.token.cancel();
        let settled = self.inner.finish_remote(entry, result).await?;
        assert!(
            matches!(settled, RemoteDispatchObservation::Committed(_)),
            "settlement finished despite the cancelled token"
        );
        Ok(settled)
    }
    async fn owns_remote(&self, request: &ActionDispatchRequest) -> AppResult<bool> {
        self.inner.owns_remote(request).await
    }
}

#[tokio::test]
async fn workflow_action_cancellation_observed_result_token_settles_then_interrupts() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let provider = provider(&f);
    heartbeat(&f, &request).await;
    let before = all_tables(&f).await;
    let token = CancellationToken::new();
    let observed = std::sync::Mutex::new(None);
    let writer = CancelAtSettlement {
        inner: adapter(&f, 0),
        token: &token,
        observed: &observed,
    };
    interrupted(
        ActionService::new(writer)
            .dispatch_remote(&request, &provider, &token)
            .await,
    );
    assert_eq!(observed.lock().unwrap().take(), Some(step_output()));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effect_count(&f).await, 1);
    let after = all_tables(&f).await;
    still_owned(&before, &after, &request);
    let entry = rows(&after, "workflow_action_remote_entries")[0]["id"].clone();
    for table in [
        "workflow_action_receipts",
        "workflow_action_actual_receipt_observations",
    ] {
        let settled = rows(&after, table);
        assert_eq!(settled.len(), 1, "{table}");
        assert_eq!(settled[0]["remote_entry_id"], entry, "{table}");
        assert_eq!(settled[0]["result"], step_output(), "{table}");
        assert_eq!(
            settled[0]["argument_digest"],
            json!(request.subject.argument_digest.as_str())
        );
    }
    // The live owner keeps the canonical receipt: a fresh service returns it without a call.
    let RemoteDispatchObservation::Committed(receipt) = ActionService::new(adapter(&f, 0))
        .dispatch_remote(&request, &provider, &CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("the settled receipt is returned to the live owner")
    };
    assert_eq!(receipt.result, step_output());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(all_tables(&f).await, after);
}
