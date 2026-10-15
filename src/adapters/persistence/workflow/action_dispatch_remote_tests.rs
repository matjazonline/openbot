use super::*;
use sqlx::PgPool;
use tokio::sync::Notify;

// Intercept only AFTER the real adapter has committed or performed final entry.
// This exercises service orchestration with genuine SQL markers and authority checks.
struct Intercept<'a> {
    inner: Dispatch,
    pool: &'a PgPool,
    after_marker: Option<&'a str>,
    ambiguous_ack: bool,
}
#[async_trait]
impl ActionDispatch for Intercept<'_> {
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
impl RemoteDispatch for Intercept<'_> {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        let reserved = self.inner.reserve_remote(request, contract).await?;
        if matches!(reserved, RemoteReservationResult::Reserved(_)) {
            if let Some(sql) = self.after_marker {
                sqlx::query(sql)
                    .bind(request.scope().company.as_uuid())
                    .execute(self.pool)
                    .await?;
            }
            if self.ambiguous_ack {
                return Err(AppError::Timeout(
                    "Injected lost commit acknowledgement".into(),
                ));
            }
        }
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
async fn workflow_action_dispatch_service_post_marker_revoke_cancel_zero_provider_polls() {
    for sql in [
        "UPDATE fixture_action_resources SET enabled=false WHERE company_id=$1",
        "UPDATE fixture_action_policies SET approval=true WHERE company_id=$1",
        "DELETE FROM company_members WHERE company_id=$1",
        "UPDATE workflow_runs SET state='cancelled' WHERE company_id=$1",
    ] {
        let (f, request) = setup().await;
        let provider = Provider {
            f: &f,
            calls: AtomicUsize::new(0),
            lost: false,
        };
        let writer = Intercept {
            inner: adapter(&f, 0),
            pool: f.persistence().pool(),
            after_marker: Some(sql),
            ambiguous_ack: false,
        };
        assert!(
            ActionService::new(writer)
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(facts(&f).await, (1, 0, 0));
    }
}

/// A worker's genuine heartbeat at a quiescent setup boundary, so each case starts from a full
/// common-policy window rather than whatever the setup left.
async fn renew(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    assert!(
        f.persistence()
            .renew_io(request.fence, policy())
            .await
            .unwrap()
            .is_some(),
        "the claimed fence is live"
    );
}

/// What a test-only seam returns at entry: the genuine entry at once, or the same entry only
/// after its own captured deadline has passed.
#[derive(Clone, Copy)]
enum AtEntry {
    Return,
    AwaitDeadline,
}

/// What the observed writer saw: the real entry (when it returned, its frozen deadline, and when
/// the seam handed it to the service), and each real ownership probe's start and result (`None`
/// for an error).
#[derive(Default)]
struct Seen {
    entry: std::sync::Mutex<Option<Entered>>,
    windows: std::sync::Mutex<Option<Windows>>,
    probes: std::sync::Mutex<Vec<(Instant, Option<bool>)>>,
}
#[derive(Clone, Copy, Debug)]
struct Entered {
    at: Instant,
    deadline: Instant,
    handed_back: Instant,
}
/// The persisted run deadline and job lease, read right after the genuine entry returned, as
/// seconds remaining at one DB instant, bracketed by monotonic reads before and after the query.
#[derive(Clone, Copy, Debug)]
struct Windows {
    read_at: Instant,
    read_after: Instant,
    run_seconds: f64,
    lease_seconds: f64,
}
impl Seen {
    fn entered(&self) -> Option<Entered> {
        *self.entry.lock().unwrap()
    }
    fn windows(&self) -> Option<Windows> {
        *self.windows.lock().unwrap()
    }
    fn probes(&self) -> Vec<(Instant, Option<bool>)> {
        self.probes.lock().unwrap().clone()
    }
    /// Phase offsets from `origin`, for a labelled failure.
    fn timeline(&self, origin: Instant) -> String {
        let ms = |at: Instant| at.saturating_duration_since(origin).as_millis();
        let entry = self.entered().map_or("entry=none".to_string(), |entry| {
            format!(
                "entry={}ms deadline={}ms handed_back={}ms",
                ms(entry.at),
                ms(entry.deadline),
                ms(entry.handed_back)
            )
        });
        let probes: Vec<String> = self
            .probes()
            .iter()
            .map(|(at, owned)| format!("{}ms:{owned:?}", ms(*at)))
            .collect();
        let windows = self.windows().map_or("windows=none".to_string(), |w| {
            format!(
                "run_remaining={:.3}s lease_remaining={:.3}s at={}ms",
                w.run_seconds,
                w.lease_seconds,
                ms(w.read_at)
            )
        });
        format!("{entry} {windows} probes=[{}]", probes.join(", "))
    }
}

/// The real writer, observed; only the hand-back time of the entry is chosen by the test.
struct Recorded<'a> {
    inner: Dispatch,
    pool: &'a PgPool,
    seen: &'a Seen,
    at_entry: AtEntry,
}
#[async_trait]
impl ActionDispatch for Recorded<'_> {
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
impl RemoteDispatch for Recorded<'_> {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        self.inner.reserve_remote(request, contract).await
    }
    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry> {
        let entered = self.inner.enter_remote(reservation).await?;
        let at = Instant::now();
        *self.seen.windows.lock().unwrap() = Some(persisted_windows(self.pool, &entered).await);
        if matches!(self.at_entry, AtEntry::AwaitDeadline) {
            tokio::time::sleep_until(entered.deadline).await;
        }
        *self.seen.entry.lock().unwrap() = Some(Entered {
            at,
            deadline: entered.deadline,
            handed_back: Instant::now(),
        });
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
        let started = Instant::now();
        let owned = self.inner.owns_remote(request).await;
        let seen = owned.as_ref().ok().copied();
        self.seen.probes.lock().unwrap().push((started, seen));
        owned
    }
}

/// The genuine persisted windows behind an entry: run deadline and job lease, from one statement.
async fn persisted_windows(pool: &PgPool, entry: &RemoteEntry) -> Windows {
    let scope = entry.request.fence.scope;
    let read_at = Instant::now();
    let (run_seconds, lease_seconds): (f64, f64) = sqlx::query_as("SELECT extract(epoch FROM (run.deadline-clock_timestamp()))::float8, extract(epoch FROM (job.lock_expires_at-clock_timestamp()))::float8 FROM workflow_runs AS run JOIN background_tasks AS job ON job.company_id=run.company_id AND job.id=$3 WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.job.0)
        .fetch_one(pool).await.unwrap();
    Windows {
        read_at,
        read_after: Instant::now(),
        run_seconds,
        lease_seconds,
    }
}

/// A debuggable name for a dispatch outcome (the production type has no `Debug`).
fn describe(result: &AppResult<RemoteDispatchObservation>) -> String {
    match result {
        Ok(RemoteDispatchObservation::Interrupted) => "Interrupted".into(),
        Ok(RemoteDispatchObservation::Committed(_)) => "Committed".into(),
        Ok(RemoteDispatchObservation::ApprovalRequired) => "ApprovalRequired".into(),
        Ok(RemoteDispatchObservation::PossibleDispatchExists) => "PossibleDispatchExists".into(),
        Err(error) => format!("Err({error:?})"),
    }
}

/// DEC-014's first-poll guard oracle: the genuine entry was reached and handed back only once
/// its own captured deadline had passed, the invocation token stayed live, and the service
/// returned Interrupted without polling the provider.
struct ExpiredEntry {
    entered: Option<Entered>,
    polls: usize,
    outcome: String,
    token_live: bool,
}
fn expired_entry_never_polled(seen: &ExpiredEntry) -> Result<(), String> {
    let entry = seen.entered.ok_or("the genuine entry was never reached")?;
    if entry.handed_back < entry.deadline {
        return Err(format!("handed back before its deadline: {entry:?}"));
    }
    if !seen.token_live {
        return Err("the token, not the expired window, interrupted".into());
    }
    if seen.polls != 0 {
        return Err(format!("the provider was polled {} times", seen.polls));
    }
    if seen.outcome != "Interrupted" {
        return Err(format!("expected Interrupted, got {}", seen.outcome));
    }
    Ok(())
}

/// DEC-014: the genuine authorized entry is held until its own captured monotonic deadline has
/// passed and then handed to the service unchanged; the final pre-poll guard must not poll.
/// No compressed lease or guessed sleep: a refusal before entry is a failed premise.
#[tokio::test]
async fn workflow_action_dispatch_delayed_first_poll_checks_monotonic_window() {
    let (f, request) = setup().await;
    renew(&f, &request).await;
    let provider = Provider {
        f: &f,
        calls: AtomicUsize::new(0),
        lost: false,
    };
    let seen = Seen::default();
    let token = CancellationToken::new();
    let writer = Recorded {
        inner: adapter(&f, 0),
        pool: f.persistence().pool(),
        seen: &seen,
        at_entry: AtEntry::AwaitDeadline,
    };
    let origin = Instant::now();
    let result = ActionService::new(writer)
        .dispatch_remote(&request, &provider, &token)
        .await;
    let observed = ExpiredEntry {
        entered: seen.entered(),
        polls: provider.calls.load(Ordering::SeqCst),
        outcome: describe(&result),
        token_live: !token.is_cancelled(),
    };
    expired_entry_never_polled(&observed)
        .unwrap_or_else(|error| panic!("{error}; {}", seen.timeline(origin)));
    assert_eq!(facts(&f).await, (1, 0, 0));
}

/// The oracle alone: only an expired, genuinely reached entry with no poll, a live token and
/// Interrupted passes; a polled provider is rejected.
#[test]
fn workflow_action_dispatch_expired_entry_oracle_rejects_polled_provider() {
    let at = Instant::now();
    let entry = |handed_back_ms: u64| Entered {
        at,
        deadline: at + Duration::from_millis(1000),
        handed_back: at + Duration::from_millis(handed_back_ms),
    };
    let case = |entered, polls, outcome: &str, token_live| ExpiredEntry {
        entered,
        polls,
        outcome: outcome.into(),
        token_live,
    };
    assert!(expired_entry_never_polled(&case(Some(entry(1000)), 0, "Interrupted", true)).is_ok());
    for (name, seen) in [
        (
            "polled provider",
            case(Some(entry(1001)), 1, "Interrupted", true),
        ),
        ("never entered", case(None, 0, "Interrupted", true)),
        (
            "not yet expired",
            case(Some(entry(999)), 0, "Interrupted", true),
        ),
        (
            "token interruption",
            case(Some(entry(1001)), 0, "Interrupted", false),
        ),
        (
            "refused before entry",
            case(None, 0, "Err(Conflict(\"lifecycle\"))", true),
        ),
        ("committed", case(Some(entry(1001)), 1, "Committed", true)),
    ] {
        assert!(expired_entry_never_polled(&seen).is_err(), "{name}");
    }
}

#[tokio::test]
async fn workflow_action_dispatch_committed_marker_ambiguous_ack_retry_restart_reclaim_never_send()
{
    let (f, request) = setup().await;
    let provider = Provider {
        f: &f,
        calls: AtomicUsize::new(0),
        lost: false,
    };
    let writer = Intercept {
        inner: adapter(&f, 0),
        pool: f.persistence().pool(),
        after_marker: None,
        ambiguous_ack: true,
    };
    let service = ActionService::new(writer);
    assert!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(facts(&f).await, (1, 0, 0));
    assert!(matches!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    let fresh = ActionService::new(adapter(&f, 0));
    assert!(matches!(
        fresh
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    expire(&f, request.fence.scope).await;
    assert!(
        f.persistence()
            .settled()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(request.scope().run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        state, "waiting",
        "Reconcile overrides the read-handler's historical safe default"
    );
    assert!(
        f.persistence()
            .settled()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(facts(&f).await, (1, 0, 0));
}

struct PendingProvider {
    started: Notify,
    polls: AtomicUsize,
    dropped: AtomicUsize,
}
struct PendingGuard<'a>(&'a AtomicUsize);
impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl RemoteAction for PendingProvider {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, _: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        let _guard = PendingGuard(&self.dropped);
        self.started.notify_one();
        std::future::pending().await
    }
}
/// How the pending invocation loses its permission, named in every failure.
#[derive(Clone, Copy, Debug)]
enum Loss {
    /// Another worker now owns the job.
    OwnershipTaken,
    /// The run is no longer live.
    RunCancelled,
    /// The run's own deadline, shorter than the lease, bounds the entry.
    RunDeadline,
    /// The invocation token, with ownership intact.
    Token,
}

/// The run-deadline mode's window, set after a fresh renewal so the run deadline (not the
/// three-second lease) is what the entry freezes. Wide enough for reserve and entry to finish.
const RUN_WINDOW: Duration = Duration::from_millis(1500);

/// When each phase of one pending case happened.
struct PendingRun {
    origin: Instant,
    run_deadline_set: Option<Instant>,
    polled: Instant,
    interrupted: Instant,
    returned: Instant,
    outcome: String,
    dropped_at_return: usize,
}

/// The causal witness for each mode, over the real entry and ownership probes.
fn interrupted_by(loss: Loss, seen: &Seen, run: &PendingRun) -> Result<(), String> {
    let entry = seen.entered().ok_or("no entry was recorded")?;
    if run.polled >= entry.deadline {
        return Err("the first poll was not inside the entry window".into());
    }
    let probes = seen.probes();
    let after: Vec<_> = probes
        .iter()
        .filter(|(started, _)| *started >= run.interrupted)
        .collect();
    match loss {
        Loss::OwnershipTaken | Loss::RunCancelled => {
            if after.iter().any(|(_, owned)| *owned != Some(false)) {
                return Err("a probe after the interruption did not refuse ownership".into());
            }
            let Some((refused, _)) = after.last() else {
                return Err("no ownership probe after the interruption".into());
            };
            if *refused > run.returned || run.returned >= entry.deadline {
                return Err("not ended by the ownership refusal before the deadline".into());
            }
        }
        Loss::RunDeadline => {
            let set = run.run_deadline_set.ok_or("the run deadline was not set")?;
            if entry.deadline > set + RUN_WINDOW {
                return Err("the entry was not bounded by the run deadline".into());
            }
            run_limits_entry(&entry, seen.windows())?;
            if probes.iter().any(|(_, owned)| *owned != Some(true)) {
                return Err("ownership was refused before the deadline".into());
            }
            if run.returned < entry.deadline {
                return Err("returned before the run deadline".into());
            }
        }
        Loss::Token => {
            if probes.iter().any(|(_, owned)| *owned != Some(true)) {
                return Err("ownership was refused, so the token is not the cause".into());
            }
            if run.returned >= entry.deadline {
                return Err("the deadline, not the token, ended it".into());
            }
        }
    }
    Ok(())
}

/// F-007: from the genuine persisted windows, the run deadline (not the lease) limits the entry.
/// `read_after + run` is no earlier than the true run expiry and `read_at + lease` no later than
/// the true lease expiry, so the frozen deadline must sit at or before the first and at least
/// `RUN_MARGIN` before the second. A lapsed or reversed premise fails before anything is credited.
fn run_limits_entry(entry: &Entered, windows: Option<Windows>) -> Result<(), String> {
    let w = windows.ok_or("no persisted windows at entry")?;
    if w.run_seconds <= 0.0 || w.lease_seconds <= 0.0 {
        return Err(format!("a window had already lapsed at entry: {w:?}"));
    }
    if w.lease_seconds <= w.run_seconds {
        return Err(format!(
            "the lease ends no later than the run deadline: {w:?}"
        ));
    }
    let run_by = w.read_after + Duration::from_secs_f64(w.run_seconds);
    let lease_from = w.read_at + Duration::from_secs_f64(w.lease_seconds);
    if run_by + RUN_MARGIN > lease_from {
        return Err(format!(
            "the run deadline is not clearly before the lease expiry: {w:?}"
        ));
    }
    if entry.deadline > run_by {
        return Err("the frozen entry deadline is later than the run deadline".into());
    }
    Ok(())
}

/// How far the run deadline must precede the lease expiry before the run is credited as the limit.
const RUN_MARGIN: Duration = Duration::from_millis(250);

/// Whether the run window is set right after the renewal (the run deadline is the limit), or only
/// once the lease has nearly run out (F-007's lease-first counterexample, the negative control).
#[derive(Clone, Copy, Debug)]
enum RunPremise {
    RunFirst,
    LeaseFirst,
}

async fn pending_case(loss: Loss) {
    let outcome = pending_run(loss, RunPremise::RunFirst).await;
    interrupted_by(loss, &outcome.seen, &outcome.run)
        .unwrap_or_else(|error| panic!("{error}; {}", outcome.phases));
}

/// One pending invocation, checked for everything except its causal witness, which the caller
/// judges with `interrupted_by`.
struct PendingOutcome {
    seen: Seen,
    run: PendingRun,
    phases: String,
}

async fn pending_run(loss: Loss, premise: RunPremise) -> PendingOutcome {
    let (f, request) = setup().await;
    renew(&f, &request).await;
    if matches!(premise, RunPremise::LeaseFirst) {
        // S's counterexample: the run window starts 2 s into the 3 s lease, so it ends after it.
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let mut run_deadline_set = None;
    if matches!(loss, Loss::RunDeadline) {
        sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+$1")
            .bind(RUN_WINDOW)
            .execute(f.persistence().pool())
            .await
            .unwrap();
        run_deadline_set = Some(Instant::now());
    }
    let provider = PendingProvider {
        started: Notify::new(),
        polls: AtomicUsize::new(0),
        dropped: AtomicUsize::new(0),
    };
    let seen = Seen::default();
    let token = CancellationToken::new();
    let service = ActionService::new(Recorded {
        inner: adapter(&f, 0),
        pool: f.persistence().pool(),
        seen: &seen,
        at_entry: AtEntry::Return,
    });
    let origin = Instant::now();
    let run = tokio::time::timeout(Duration::from_secs(4), async {
        let dispatch = service.dispatch_remote(&request, &provider, &token);
        tokio::pin!(dispatch);
        // A refused entry ends dispatch before any poll: fail at once, named, not at the watchdog.
        let polled = tokio::select! {
            result = &mut dispatch => panic!(
                "{loss:?}: dispatch returned {} before the first provider poll; {}",
                describe(&result),
                seen.timeline(origin)
            ),
            _ = provider.started.notified() => Instant::now(),
        };
        match loss {
            Loss::OwnershipTaken => {
                sqlx::query("UPDATE background_tasks SET worker_id=$1")
                    .bind(worker().0)
                    .execute(f.persistence().pool())
                    .await
                    .unwrap();
            }
            Loss::RunCancelled => {
                sqlx::query("UPDATE workflow_runs SET state='cancelled'")
                    .execute(f.persistence().pool())
                    .await
                    .unwrap();
            }
            Loss::RunDeadline => (),
            Loss::Token => token.cancel(),
        }
        let interrupted = Instant::now();
        let result = dispatch.await;
        PendingRun {
            origin,
            run_deadline_set,
            polled,
            interrupted,
            returned: Instant::now(),
            outcome: describe(&result),
            dropped_at_return: provider.dropped.load(Ordering::SeqCst),
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "{loss:?}: not settled within 4 s; polls={} {}",
            provider.polls.load(Ordering::SeqCst),
            seen.timeline(origin)
        )
    });
    let ms = |at: Instant| at.saturating_duration_since(run.origin).as_millis();
    let phases = format!(
        "{loss:?}: polled={}ms interrupted={}ms returned={}ms {}",
        ms(run.polled),
        ms(run.interrupted),
        ms(run.returned),
        seen.timeline(run.origin)
    );
    eprintln!("pending case {phases}");
    assert_eq!(run.outcome, "Interrupted", "{phases}");
    assert_eq!(provider.polls.load(Ordering::SeqCst), 1, "{phases}");
    assert_eq!(
        run.dropped_at_return, 1,
        "the actual future must be dropped before dispatch returns; {phases}"
    );
    assert!(
        !token.is_cancelled() || matches!(loss, Loss::Token),
        "{phases}"
    );
    assert_eq!(facts(&f).await, (1, 0, 0), "{phases}");
    PendingOutcome { seen, run, phases }
}

#[tokio::test]
async fn workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline() {
    for loss in [
        Loss::OwnershipTaken,
        Loss::RunCancelled,
        Loss::RunDeadline,
        Loss::Token,
    ] {
        pending_case(loss).await;
    }
}

/// F-007 negative control. The lease-first premise is an otherwise successful pending case
/// (Interrupted, one poll, drop before return, inside the post-update allowance, no refused
/// probe), but the lease ends it, so the run-deadline witness must reject it. The constructed
/// cases add the empty-probe and lapsed/reversed forms deterministically.
#[tokio::test]
async fn workflow_action_dispatch_pending_run_deadline_oracle_rejects_lease_limited_entry() {
    let outcome = pending_run(Loss::RunDeadline, RunPremise::LeaseFirst).await;
    let entry = outcome.seen.entered().expect("entered");
    let set = outcome.run.run_deadline_set.expect("run window set");
    assert!(entry.deadline <= set + RUN_WINDOW, "{}", outcome.phases);
    assert!(outcome.run.returned >= entry.deadline, "{}", outcome.phases);
    assert!(
        outcome
            .seen
            .probes()
            .iter()
            .all(|(_, owned)| *owned == Some(true)),
        "{}",
        outcome.phases
    );
    let verdict = interrupted_by(Loss::RunDeadline, &outcome.seen, &outcome.run);
    assert!(
        verdict.is_err(),
        "a lease-limited entry must not pass as run-deadline proof; {}",
        outcome.phases
    );
    eprintln!("lease-first control rejected: {}", verdict.unwrap_err());

    let at = Instant::now();
    let entered = Entered {
        at,
        deadline: at + Duration::from_millis(1000),
        handed_back: at,
    };
    let windows = |run_seconds: f64, lease_seconds: f64| Windows {
        read_at: at,
        read_after: at + Duration::from_millis(5),
        run_seconds,
        lease_seconds,
    };
    assert!(run_limits_entry(&entered, Some(windows(1.0, 2.9))).is_ok());
    for (case, seen) in [
        ("lease first, no probes", Some(windows(1.5, 1.0))),
        ("lease and run equal", Some(windows(1.2, 1.2))),
        (
            "run within the margin of the lease",
            Some(windows(1.0, 1.1)),
        ),
        ("run already lapsed", Some(windows(-0.1, 2.0))),
        ("lease already lapsed", Some(windows(1.0, -0.1))),
        ("entry later than the run deadline", Some(windows(0.5, 2.9))),
        ("no persisted windows", None),
    ] {
        assert!(run_limits_entry(&entered, seen).is_err(), "{case}");
    }
}
