//! 04.8 group B1: an actual CancelCommand at each undispatched and in-flight action boundary,
//! token-only interruption beside it, and action-bearing cancellation rollback (DEC-001/003/006).
//! Every case uses the real control transition, the real dispatch service/writer and
//! whole-database snapshots; refusals are asserted to write nothing at all.
use super::*;
use crate::application::workflow::lease::FencedWorkflowResult;
use tokio::sync::Notify;

/// A result valid both for the frozen action contract and as the step's committed output.
fn step_output() -> Value {
    json!({"items":[],"token_count":0})
}

fn provider(f: &AdmissionFixture) -> DeduplicatingProvider<'_> {
    let mut provider = dedup(f, None, false);
    provider.result = step_output();
    provider
}

async fn cancel_command(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    key: &str,
) -> CancelCommand {
    CancelCommand {
        company_id: request.scope().company,
        run_id: request.scope().run,
        actor: f.binding.target.actor,
        command_key: IdempotencyKey::parse(key).unwrap(),
        expected_revision: f
            .persistence()
            .head(request.scope().company, request.scope().run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    }
}

/// The durable control transition, committed.
async fn cancel(f: &AdmissionFixture, request: &ActionDispatchRequest, key: &str) {
    let result = f
        .persistence()
        .cancel(cancel_command(f, request, key).await)
        .await
        .unwrap();
    assert!(matches!(result, CancelResult::Applied { .. }), "{result:?}");
}

/// A worker's genuine heartbeat before a baseline, so a short fixture lease cannot lapse
/// inside the interval a case depends on.
async fn heartbeat(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    assert!(
        f.persistence()
            .renew_io(request.fence, policy())
            .await
            .unwrap()
            .is_some(),
        "the claimed fence is live"
    );
}

fn rows<'a>(state: &'a Value, table: &str) -> &'a Vec<Value> {
    state[table].as_array().unwrap()
}

/// `before` -> `after` is exactly one durable cancellation of this fence: the run, its job and
/// current attempt retired once, one control event and command receipt, and the uncertainty
/// record for any marker. Nothing else changes; prepared intent and every action fact remain.
fn retired_once(before: &Value, after: &Value, request: &ActionDispatchRequest) {
    let allowed = [
        "workflow_runs",
        "background_tasks",
        "task_attempts",
        "workflow_run_events",
        "workflow_control_commands",
        "workflow_action_reconciliations",
        "workflow_action_state_witnesses",
    ];
    for (table, rows) in before.as_object().unwrap() {
        if !allowed.contains(&table.as_str()) {
            assert_eq!(&after[table], rows, "cancellation preserves {table}");
        }
    }
    let run = &rows(after, "workflow_runs")[0];
    assert_eq!(run["state"], "cancelled");
    // The transition's own state witness: append-only, one row, recording the prior state.
    let (old, new) = (
        rows(before, "workflow_action_state_witnesses"),
        rows(after, "workflow_action_state_witnesses"),
    );
    assert_eq!(new.len(), old.len() + 1);
    assert!(old.iter().all(|row| new.contains(row)));
    let witness = new.iter().find(|row| !old.contains(row)).unwrap();
    assert_eq!(witness["run_id"], json!(request.scope().run.as_uuid()));
    assert_eq!(
        witness["initial_state"],
        rows(before, "workflow_runs")[0]["state"]
    );
    assert_eq!(
        rows(after, "task_attempts").len(),
        rows(before, "task_attempts").len()
    );
    let attempt = rows(after, "task_attempts")
        .iter()
        .find(|row| row["attempt_number"] == json!(request.fence.attempt.0))
        .unwrap();
    assert_eq!(attempt["status"], "failed");
    assert_eq!(attempt["workflow_retirement"], "cancel");
    assert_eq!(attempt["workflow_failure_code"], "workflow.cancelled");
    assert_eq!(attempt["worker_id"], json!(request.fence.worker.0));
    let job = &rows(after, "background_tasks")[0];
    assert_eq!(job["status"], "failed");
    assert!(job["worker_id"].is_null() && job["lock_expires_at"].is_null());
    assert_eq!(
        rows(after, "workflow_control_commands").len(),
        rows(before, "workflow_control_commands").len() + 1
    );
    let events = |state: &Value| {
        rows(state, "workflow_run_events")
            .iter()
            .filter(|row| row["event_kind"] == "control_cancel")
            .count()
    };
    assert_eq!(events(after), events(before) + 1);
    let execution = &rows(after, "workflow_executions")[0];
    for field in ["completed_at", "committed_output", "successor_execution_id"] {
        assert!(execution[field].is_null(), "no completion: {field}");
    }
}

/// After cancellation the same fence and a fresh claimant are both inert: no completion, no
/// new attempt, and no write anywhere.
async fn inert(f: &AdmissionFixture, request: &ActionDispatchRequest, cancelled: &Value) {
    let completion = f
        .persistence()
        .settled()
        .complete_io(FencedWorkflowResult {
            policy: policy(),
            fence: request.fence,
            output: step_output(),
        })
        .await
        .unwrap();
    assert!(completion.is_none(), "a cancelled fence cannot complete");
    assert!(
        f.persistence()
            .settled()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(&all_tables(f).await, cancelled, "inert after cancellation");
}

fn refused(result: AppResult<impl std::fmt::Debug>) {
    assert!(
        matches!(&result, Err(AppError::Conflict(message)) if message == "Workflow lifecycle revision or command conflict"),
        "expected the cancelled-fence refusal: {result:?}"
    );
}

#[derive(Clone, Copy, Debug)]
enum Prepared {
    Local,
    Remote,
    ProtectedLocal,
    ProtectedRemote,
}

#[tokio::test]
async fn workflow_action_cancellation_prepared_and_protected_refuse_after_command() {
    for case in [
        Prepared::Local,
        Prepared::Remote,
        Prepared::ProtectedLocal,
        Prepared::ProtectedRemote,
    ] {
        let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
        let provider = provider(&f);
        if matches!(case, Prepared::ProtectedLocal | Prepared::ProtectedRemote) {
            sqlx::query("UPDATE fixture_action_policies SET approval=true")
                .execute(f.persistence().pool())
                .await
                .unwrap();
        }
        heartbeat(&f, &request).await;
        let before = all_tables(&f).await;
        assert_eq!(rows(&before, "workflow_action_intents").len(), 1);
        cancel(&f, &request, "b1-prepared").await;
        let cancelled = all_tables(&f).await;
        retired_once(&before, &cancelled, &request);
        // Every public service path, twice: the original service and a fresh one.
        for _ in 0..2 {
            let service = ActionService::new(adapter(&f, 0));
            match case {
                Prepared::Local | Prepared::ProtectedLocal => {
                    refused(service.dispatch_local(&request).await.map(|_| ()))
                }
                Prepared::Remote | Prepared::ProtectedRemote => refused(
                    service
                        .dispatch_remote(&request, &provider, &CancellationToken::new())
                        .await
                        .map(|_| ()),
                ),
            }
            assert_eq!(&all_tables(&f).await, &cancelled, "{case:?} writes nothing");
        }
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(effect_count(&f).await, 0);
        assert_eq!(facts(&f).await, (0, 0, 0));
        inert(&f, &request, &cancelled).await;
    }
}

/// Real reservation commits, then the actual cancellation commits before the service performs
/// its final entry authorization; nothing else is intercepted.
/// What the gate observed: reserved marker and entry, and the snapshot after cancellation.
type Reserved = std::sync::Mutex<Option<(Uuid, Uuid, Value)>>;
struct CancelAfterReservation<'a> {
    f: &'a AdmissionFixture,
    inner: Dispatch,
    reserved: &'a Reserved,
}
#[async_trait]
impl ActionDispatch for CancelAfterReservation<'_> {
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
impl RemoteDispatch for CancelAfterReservation<'_> {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        let reserved = self.inner.reserve_remote(request, contract).await?;
        let RemoteReservationResult::Reserved(reservation) = &reserved else {
            panic!("a genuine first reservation")
        };
        let before = all_tables(self.f).await;
        cancel(self.f, request, "b1-after-reservation").await;
        let cancelled = all_tables(self.f).await;
        retired_once(&before, &cancelled, request);
        *self.reserved.lock().unwrap() = Some((reservation.marker, reservation.entry, cancelled));
        Ok(reserved)
    }
    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry> {
        self.inner.enter_remote(reservation).await
    }
    async fn finish_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation> {
        self.inner.finish_remote(entry, result).await
    }
    async fn owns_remote(&self, request: &ActionDispatchRequest) -> AppResult<bool> {
        self.inner.owns_remote(request).await
    }
}

#[tokio::test]
async fn workflow_action_cancellation_after_reservation_refuses_entry_zero_polls() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let provider = provider(&f);
    heartbeat(&f, &request).await;
    let reserved = Reserved::default();
    let writer = CancelAfterReservation {
        f: &f,
        inner: adapter(&f, 0),
        reserved: &reserved,
    };
    refused(
        ActionService::new(writer)
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .map(|_| ()),
    );
    let (marker, entry, cancelled) = reserved.lock().unwrap().take().expect("gate reached");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(effect_count(&f).await, 0);
    let after = all_tables(&f).await;
    assert_eq!(after, cancelled, "refused entry writes nothing");
    let ids = |table: &str| -> Vec<Value> {
        rows(&after, table)
            .iter()
            .map(|row| row["id"].clone())
            .collect()
    };
    assert_eq!(ids("workflow_action_dispatches"), vec![json!(marker)]);
    assert_eq!(ids("workflow_action_remote_entries"), vec![json!(entry)]);
    assert!(rows(&after, "workflow_action_receipts").is_empty());
    // The retained marker is unresolved; cancellation records it for reconciliation.
    assert_eq!(rows(&after, "workflow_action_reconciliations").len(), 1);
    refused(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .map(|_| ()),
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    inert(&f, &request, &cancelled).await;
}

/// A genuine first poll that never resolves. `applied` records the real external effect before
/// pending (accepted, response never arrives); otherwise the provider never applies it.
struct Pending<'a> {
    f: &'a AdmissionFixture,
    applied: bool,
    started: Notify,
    polls: AtomicUsize,
    dropped: AtomicUsize,
}
struct DropGuard<'a>(&'a AtomicUsize);
impl Drop for DropGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl<'a> Pending<'a> {
    fn new(f: &'a AdmissionFixture, applied: bool) -> Self {
        Self {
            f,
            applied,
            started: Notify::new(),
            polls: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
        }
    }
}
#[async_trait]
impl RemoteAction for Pending<'_> {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, action: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        let _guard = DropGuard(&self.dropped);
        if self.applied {
            sqlx::query("INSERT INTO fixture_remote_effects VALUES ($1)")
                .bind(action.idempotency_key().as_str())
                .execute(self.f.persistence().pool())
                .await?;
        }
        self.started.notify_one();
        std::future::pending().await
    }
}

/// Dispatch, recording how many provider futures were dropped at the moment it returned.
async fn dispatch_observed(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &Pending<'_>,
    token: &CancellationToken,
) -> (AppResult<RemoteDispatchObservation>, usize) {
    let result = ActionService::new(adapter(f, 0))
        .dispatch_remote(request, provider, token)
        .await;
    (result, provider.dropped.load(Ordering::SeqCst))
}

fn interrupted(result: AppResult<RemoteDispatchObservation>) {
    assert!(
        matches!(result, Ok(RemoteDispatchObservation::Interrupted)),
        "expected Interrupted"
    );
}

/// What the delegating supervision seam saw: the deadline the real `enter_remote` froze, and
/// each ownership probe's start time and real result (`None` for an error).
#[derive(Default)]
struct Supervision {
    deadline: std::sync::Mutex<Option<tokio::time::Instant>>,
    probes: std::sync::Mutex<Vec<(tokio::time::Instant, Option<bool>)>>,
}

/// The real writer, observed. `always_owned` is the negative control: ownership supervision
/// disabled, every heartbeat affirming ownership without asking the database.
struct Supervised<'a> {
    inner: Dispatch,
    always_owned: bool,
    seen: &'a Supervision,
}
#[async_trait]
impl ActionDispatch for Supervised<'_> {
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
impl RemoteDispatch for Supervised<'_> {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        self.inner.reserve_remote(request, contract).await
    }
    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry> {
        let entered = self.inner.enter_remote(reservation).await?;
        *self.seen.deadline.lock().unwrap() = Some(entered.deadline);
        Ok(entered)
    }
    async fn finish_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation> {
        self.inner.finish_remote(entry, result).await
    }
    async fn owns_remote(&self, request: &ActionDispatchRequest) -> AppResult<bool> {
        let started = tokio::time::Instant::now();
        let owned = if self.always_owned {
            Ok(true)
        } else {
            self.inner.owns_remote(request).await
        };
        let seen = owned.as_ref().ok().copied();
        self.seen.probes.lock().unwrap().push((started, seen));
        owned
    }
}

/// F-004's causal oracle. The invocation ended because a real ownership probe, started after
/// the cancellation committed, was refused, and it ended before the frozen entry deadline, so
/// neither that deadline nor a supervision-guard timeout (which records no result) can pass.
fn ownership_interrupted(
    seen: &Supervision,
    cancelled_at: tokio::time::Instant,
    returned_at: tokio::time::Instant,
) -> Result<(), String> {
    let deadline = seen
        .deadline
        .lock()
        .unwrap()
        .ok_or("no frozen entry deadline")?;
    let probes = seen.probes.lock().unwrap();
    let after: Vec<_> = probes
        .iter()
        .filter(|(started, _)| *started >= cancelled_at)
        .collect();
    if after.iter().any(|(_, owned)| *owned != Some(false)) {
        return Err(format!(
            "a post-cancel probe did not refuse ownership: {probes:?}"
        ));
    }
    let Some((refused_at, _)) = after.last() else {
        return Err(format!("no ownership probe after cancellation: {probes:?}"));
    };
    if *refused_at > returned_at {
        return Err("the refusal followed the return".into());
    }
    if returned_at >= deadline {
        return Err(format!(
            "returned {:?} at or after the frozen deadline",
            returned_at - deadline
        ));
    }
    Ok(())
}

struct PendingRun {
    result: AppResult<RemoteDispatchObservation>,
    dropped: usize,
    cancelled_at: tokio::time::Instant,
    returned_at: tokio::time::Instant,
    cancelled: Value,
}

/// DEC-003 durable case: the token stays live; the real CancelCommand commits only after the
/// live pending premise holds, and the service is observed through `Supervised`.
async fn run_pending(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &Pending<'_>,
    seen: &Supervision,
    always_owned: bool,
) -> PendingRun {
    let token = CancellationToken::new();
    let service = ActionService::new(Supervised {
        inner: adapter(f, 0),
        always_owned,
        seen,
    });
    let dispatch = async {
        let result = service.dispatch_remote(request, provider, &token).await;
        (
            result,
            provider.dropped.load(Ordering::SeqCst),
            tokio::time::Instant::now(),
        )
    };
    let durable = async {
        provider.started.notified().await;
        // The required premise, before cancellation: genuinely polled, still pending, owned.
        assert_eq!(provider.polls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.dropped.load(Ordering::SeqCst), 0, "pending");
        let deadline = seen.deadline.lock().unwrap().expect("entered");
        assert!(tokio::time::Instant::now() < deadline, "inside the window");
        assert!(
            adapter(f, 0).owns_remote(request).await.unwrap(),
            "live ownership"
        );
        let entered = all_tables(f).await;
        cancel(f, request, "b1-pending").await;
        let cancelled_at = tokio::time::Instant::now();
        let cancelled = all_tables(f).await;
        retired_once(&entered, &cancelled, request);
        (cancelled_at, cancelled)
    };
    let ((result, dropped, returned_at), (cancelled_at, cancelled)) =
        tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(dispatch, durable)
        })
        .await
        .expect("bounded pending invocation");
    assert!(!token.is_cancelled(), "durable, not token, interruption");
    PendingRun {
        result,
        dropped,
        cancelled_at,
        returned_at,
        cancelled,
    }
}

#[tokio::test]
async fn workflow_action_cancellation_pending_provider_dropped_by_ownership_supervision() {
    for applied in [false, true] {
        let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
        heartbeat(&f, &request).await;
        let provider = Pending::new(&f, applied);
        let seen = Supervision::default();
        let run = run_pending(&f, &request, &provider, &seen, false).await;
        interrupted(run.result);
        ownership_interrupted(&seen, run.cancelled_at, run.returned_at).unwrap();
        assert_eq!(provider.polls.load(Ordering::SeqCst), 1);
        assert_eq!(
            run.dropped, 1,
            "the actual future is dropped before dispatch returns"
        );
        let after = all_tables(&f).await;
        assert_eq!(after, run.cancelled, "interruption fabricates nothing");
        assert_eq!(rows(&after, "workflow_action_dispatches").len(), 1);
        assert_eq!(rows(&after, "workflow_action_remote_entries").len(), 1);
        assert!(rows(&after, "workflow_action_receipts").is_empty());
        assert_eq!(rows(&after, "workflow_action_reconciliations").len(), 1);
        assert_eq!(
            effect_count(&f).await,
            i64::from(applied),
            "actual ledger truth"
        );
        refused(
            ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .map(|_| ()),
        );
        assert_eq!(provider.polls.load(Ordering::SeqCst), 1);
        inert(&f, &request, &run.cancelled).await;
    }
}

/// Negative control: with ownership supervision disabled the frozen deadline still ends the
/// invocation, and that must fail the causal oracle rather than pass.
#[tokio::test]
async fn workflow_action_cancellation_pending_oracle_rejects_deadline_only_interruption() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    heartbeat(&f, &request).await;
    let provider = Pending::new(&f, false);
    let seen = Supervision::default();
    let run = run_pending(&f, &request, &provider, &seen, true).await;
    interrupted(run.result);
    assert_eq!(run.dropped, 1);
    let deadline = seen.deadline.lock().unwrap().unwrap();
    assert!(
        run.returned_at >= deadline,
        "only the frozen deadline ended it"
    );
    let verdict = ownership_interrupted(&seen, run.cancelled_at, run.returned_at);
    assert!(
        verdict.is_err(),
        "deadline-only interruption must fail the oracle"
    );
    eprintln!("negative control rejected: {}", verdict.unwrap_err());
}

/// The oracle alone, over constructed observations: only a refusal probe started after the
/// cancellation and returning before the deadline passes.
#[test]
fn workflow_action_cancellation_pending_oracle_discriminates_interruption_causes() {
    let start = tokio::time::Instant::now();
    let at = |millis: u64| start + Duration::from_millis(millis);
    let seen = |probes: Vec<(u64, Option<bool>)>| {
        let seen = Supervision::default();
        *seen.deadline.lock().unwrap() = Some(at(3000));
        *seen.probes.lock().unwrap() = probes
            .into_iter()
            .map(|(t, owned)| (at(t), owned))
            .collect();
        seen
    };
    let (cancelled, returned) = (at(100), at(1010));
    // Refused by a post-cancel probe; an earlier pre-cancel affirmation is irrelevant.
    assert!(
        ownership_interrupted(
            &seen(vec![(50, Some(true)), (1000, Some(false))]),
            cancelled,
            returned
        )
        .is_ok()
    );
    for (case, probes, returned) in [
        (
            "guard timeout records no result",
            vec![(50, Some(true))],
            returned,
        ),
        ("probe error", vec![(1000, None)], returned),
        (
            "still affirmed",
            vec![(1000, Some(true)), (2000, Some(false))],
            at(2010),
        ),
        (
            "deadline-only",
            vec![(1000, Some(true)), (2000, Some(true))],
            at(3000),
        ),
        ("refusal after return", vec![(1000, Some(false))], at(900)),
        (
            "refused but returned at deadline",
            vec![(2990, Some(false))],
            at(3000),
        ),
    ] {
        assert!(
            ownership_interrupted(&seen(probes), cancelled, returned).is_err(),
            "{case}"
        );
    }
}

/// Control facts a token-only interruption must leave as they were.
const OWNERSHIP: [&str; 7] = [
    "workflow_runs",
    "workflow_executions",
    "background_tasks",
    "task_attempts",
    "workflow_run_events",
    "workflow_control_commands",
    "workflow_action_reconciliations",
];

fn still_owned(before: &Value, after: &Value, request: &ActionDispatchRequest) {
    for table in OWNERSHIP {
        assert_eq!(
            after[table], before[table],
            "token interruption preserves {table}"
        );
    }
    assert_eq!(rows(after, "workflow_runs")[0]["state"], "running");
    let job = &rows(after, "background_tasks")[0];
    assert_eq!(job["status"], "processing");
    assert_eq!(job["worker_id"], json!(request.fence.worker.0));
}

/// The original Reconcile contract after an unresolved marker: no replay permission.
async fn retry_needs_reconciliation(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &dyn RemoteAction,
) {
    let before = all_tables(f).await;
    let retry = ActionService::new(adapter(f, 0))
        .dispatch_remote(request, &Forward(provider), &CancellationToken::new())
        .await
        .unwrap();
    assert!(matches!(
        retry,
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    assert_eq!(
        all_tables(f).await,
        before,
        "a refused retry writes nothing"
    );
}
struct Forward<'a>(&'a dyn RemoteAction);
#[async_trait]
impl RemoteAction for Forward<'_> {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        self.0.replay_contract()
    }
    async fn invoke(
        &self,
        action: &FrozenAction,
        invocation: &ProviderInvocation,
    ) -> AppResult<Value> {
        self.0.invoke(action, invocation).await
    }
}

#[tokio::test]
async fn workflow_action_cancellation_token_precancelled_never_reserves() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let provider = provider(&f);
    heartbeat(&f, &request).await;
    let before = all_tables(&f).await;
    let token = CancellationToken::new();
    token.cancel();
    interrupted(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &provider, &token)
            .await,
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        all_tables(&f).await,
        before,
        "no reservation, marker or entry"
    );
}

/// The real final entry authorization succeeds, then the token is cancelled before the
/// service's monotonic/token check that precedes the first poll.
struct CancelAfterEntry<'a> {
    inner: Dispatch,
    token: &'a CancellationToken,
}
#[async_trait]
impl ActionDispatch for CancelAfterEntry<'_> {
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
impl RemoteDispatch for CancelAfterEntry<'_> {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        self.inner.reserve_remote(request, contract).await
    }
    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry> {
        let entered = self.inner.enter_remote(reservation).await?;
        self.token.cancel();
        Ok(entered)
    }
    async fn finish_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation> {
        self.inner.finish_remote(entry, result).await
    }
    async fn owns_remote(&self, request: &ActionDispatchRequest) -> AppResult<bool> {
        self.inner.owns_remote(request).await
    }
}

#[tokio::test]
async fn workflow_action_cancellation_token_after_entry_suppresses_first_poll() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let provider = provider(&f);
    heartbeat(&f, &request).await;
    let before = all_tables(&f).await;
    let token = CancellationToken::new();
    let writer = CancelAfterEntry {
        inner: adapter(&f, 0),
        token: &token,
    };
    interrupted(
        ActionService::new(writer)
            .dispatch_remote(&request, &provider, &token)
            .await,
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0, "no first poll");
    assert_eq!(effect_count(&f).await, 0);
    let after = all_tables(&f).await;
    still_owned(&before, &after, &request);
    assert_eq!(rows(&after, "workflow_action_dispatches").len(), 1);
    assert_eq!(rows(&after, "workflow_action_remote_entries").len(), 1);
    assert!(rows(&after, "workflow_action_receipts").is_empty());
    retry_needs_reconciliation(&f, &request, &provider).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn workflow_action_cancellation_token_pending_drop_keeps_ownership() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    heartbeat(&f, &request).await;
    let before = all_tables(&f).await;
    let provider = Pending::new(&f, false);
    let token = CancellationToken::new();
    let interrupt = async {
        provider.started.notified().await;
        let entered = all_tables(&f).await;
        token.cancel();
        entered
    };
    let ((result, dropped), entered) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(
            dispatch_observed(&f, &request, &provider, &token),
            interrupt
        )
    })
    .await
    .expect("token interruption ends the pending invocation");
    interrupted(result);
    assert_eq!(provider.polls.load(Ordering::SeqCst), 1);
    assert_eq!(
        dropped, 1,
        "the actual future is dropped before dispatch returns"
    );
    let after = all_tables(&f).await;
    assert_eq!(after, entered, "token interruption writes nothing");
    still_owned(&before, &after, &request);
    assert_eq!(rows(&after, "workflow_action_dispatches").len(), 1);
    assert_eq!(rows(&after, "workflow_action_remote_entries").len(), 1);
    assert!(rows(&after, "workflow_action_receipts").is_empty());
    assert_eq!(effect_count(&f).await, 0);
    retry_needs_reconciliation(&f, &request, &provider).await;
    assert_eq!(
        provider.polls.load(Ordering::SeqCst),
        1,
        "no replay permission"
    );
}

/// DEC-006: a fault deferred to COMMIT, after every cancellation write has executed.
async fn install_commit_fault(f: &AdmissionFixture) {
    sqlx::raw_sql("CREATE FUNCTION b1_cancel_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'b1 cancellation commit fault'; END $$; CREATE CONSTRAINT TRIGGER b1_cancel_fault AFTER INSERT ON workflow_control_commands DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION b1_cancel_fault();")
        .execute(f.persistence().pool()).await.unwrap();
}
async fn remove_commit_fault(f: &AdmissionFixture) {
    sqlx::raw_sql("DROP TRIGGER b1_cancel_fault ON workflow_control_commands; DROP FUNCTION b1_cancel_fault();")
        .execute(f.persistence().pool()).await.unwrap();
}
async fn faulted_cancel_rolls_back(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    install_commit_fault(f).await;
    heartbeat(f, request).await;
    let baseline = all_tables(f).await;
    let error = f
        .persistence()
        .cancel(cancel_command(f, request, "b1-faulted").await)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("b1 cancellation commit fault"),
        "{error}"
    );
    assert_eq!(all_tables(f).await, baseline, "complete rollback");
    remove_commit_fault(f).await;
}

async fn completes_once(f: &AdmissionFixture, request: &ActionDispatchRequest, output: &Value) {
    let committed = f
        .persistence()
        .settled()
        .complete_io(FencedWorkflowResult {
            policy: policy(),
            fence: request.fence,
            output: output.clone(),
        })
        .await
        .unwrap()
        .expect("the original fence completes");
    assert_eq!(&committed.output, output);
    let state = all_tables(f).await;
    assert_eq!(
        rows(&state, "task_attempts").len(),
        1,
        "no reclaim or new attempt"
    );
    assert_eq!(rows(&state, "workflow_runs")[0]["state"], "succeeded");
}

#[tokio::test]
async fn workflow_action_cancellation_rollback_keeps_prepared_work_usable() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let provider = provider(&f);
    faulted_cancel_rolls_back(&f, &request).await;
    let service = ActionService::new(adapter(&f, 0));
    for _ in 0..2 {
        let RemoteDispatchObservation::Committed(receipt) = service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap()
        else {
            panic!("the same fence dispatches the prepared action")
        };
        assert_eq!(receipt.result, step_output());
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1, "dispatched once");
    assert_eq!(effect_count(&f).await, 1);
    completes_once(&f, &request, &step_output()).await;
}

#[tokio::test]
async fn workflow_action_cancellation_rollback_keeps_accepted_effect_settleable() {
    let (f, request, _) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let provider = provider(&f);
    let writer = adapter(&f, 0);
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
    assert_eq!(effect_count(&f).await, 1, "accepted external effect");
    faulted_cancel_rolls_back(&f, &request).await;
    let RemoteDispatchObservation::Committed(receipt) =
        writer.finish_remote(entry, result.clone()).await.unwrap()
    else {
        panic!("the same fence settles the accepted effect")
    };
    assert_eq!(receipt.result, result);
    assert_eq!(facts(&f).await, (1, 1, 0));
    completes_once(&f, &request, &result).await;
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        1,
        "no further provider call"
    );
    assert_eq!(effect_count(&f).await, 1);
}

#[path = "action_cancellation_receipt_tests.rs"]
mod receipt_tests;

#[path = "action_cancellation_completion_tests.rs"]
mod completion_tests;
