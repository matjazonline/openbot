//! (i) A local action stalled in COMMIT: its effect, marker and receipt share the stalled
//! transaction, so the receipt row alone classifies it and the effect exists at most once.
//! (q)–(s) A remote reservation stalled in COMMIT (DESIGN v6, D2): adopted when its own entry
//! is proven under the original's locks and its fence still holds, never sent twice, and
//! Unknown when the fence lapsed in between.
use super::*;
use crate::adapters::persistence::workflow::maintenance::{RetirementCause, retire_jobs};
use crate::adapters::persistence::workflow::persistence_bound::recorder::{Injected, Observed};
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::*;
use sqlx::PgPool;
use std::future::Future;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

#[tokio::test]
async fn workflow_action_dispatch_resolution_stalled_local_effect_once() {
    let _injected = Injected::mark();
    for applies in [true, false] {
        let (f, request) = setup().await;
        let lease = request.lease;
        let (mut observer, _) = connect(&f).await;
        let mut gate = Gate::arm(
            &f,
            "workflow_action_receipts",
            "INSERT",
            "true",
            Stall::Commit,
        )
        .await;
        let spawn_local = || {
            let dispatch = adapter(&f, 0);
            let request = request.clone();
            tokio::spawn(async move { dispatch.local(&request).await })
        };
        let first = spawn_local();
        let dispatching = gate
            .stalled(&mut observer, &[&|| !first.is_finished()])
            .await;
        if !applies {
            gate.abort_on_release(Abort::Backend(dispatching)).await;
        }
        resolution_behind(
            &mut observer,
            dispatching,
            lease,
            &[&|| !first.is_finished()],
        )
        .await;
        let second = spawn_local();
        waiters(
            &mut observer,
            dispatching,
            2,
            "dispatch resolution and competing dispatch",
            &[&|| !first.is_finished(), &|| !second.is_finished()],
        )
        .await;
        gate.release().await;
        let first = first.await.unwrap();
        let LocalDispatchResult::Committed(receipt) = second.await.unwrap().unwrap() else {
            panic!("the competing dispatch finds or writes the one receipt")
        };
        assert_eq!(receipt.result, json!({"written":true}));
        if applies {
            let LocalDispatchResult::Committed(resolved) = first.unwrap() else {
                panic!("the stalled dispatch committed")
            };
            assert_eq!(resolved.result, receipt.result);
        } else {
            let Err(error) = first else {
                panic!("the aborted dispatch reported success")
            };
            assert!(not_applied(&error, "action.local"), "{error:?}");
        }
        assert_eq!(facts(&f).await, (1, 1, 1));
    }
}

async fn remote_rows(f: &AdmissionFixture) -> (i64, i64) {
    remote_rows_on(f.persistence().pool()).await
}

async fn remote_rows_on(pool: &PgPool) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM workflow_action_dispatches), (SELECT count(*) FROM workflow_action_remote_entries)")
        .fetch_one(pool).await.unwrap()
}

const RESERVE: &str = "action.reserve_remote";
/// action_dispatch's pre-COMMIT `conflict()`: enter_remote's refusal of a fence that is gone.
const REFUSAL: &str = "Workflow lifecycle revision or command conflict";

/// Counts its calls and checks that the marker and entry committed before it is polled.
#[derive(Clone)]
struct Counted {
    calls: Arc<AtomicUsize>,
    pool: PgPool,
}
impl Counted {
    fn new(f: &AdmissionFixture) -> Self {
        Self {
            calls: Arc::default(),
            pool: f.persistence().pool().clone(),
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}
#[async_trait]
impl RemoteAction for Counted {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, _: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            remote_rows_on(&self.pool).await,
            (1, 1),
            "marker and entry must be committed before polling the provider"
        );
        Ok(json!({"observed":true}))
    }
}

fn spawn_remote(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &Counted,
) -> JoinHandle<AppResult<RemoteDispatchObservation>> {
    let dispatch = adapter(f, 0);
    let (request, provider) = (request.clone(), provider.clone());
    tokio::spawn(async move {
        dispatch
            .remote(&request, &provider, &CancellationToken::new())
            .await
    })
}

/// The dispatch fixture on the 6 s gate policy (A = 2 s, R = 1 s), its fence renewed to match.
async fn gated() -> (AdmissionFixture, ActionDispatchRequest) {
    let (f, mut request) = setup().await;
    let lease = gated_policy();
    assert!(
        f.persistence()
            .settled()
            .renew_io(request.fence, lease)
            .await
            .unwrap()
            .is_some()
    );
    request.lease = lease;
    (f, request)
}

async fn entry_gate(f: &AdmissionFixture) -> Gate {
    Gate::arm(
        f,
        "workflow_action_remote_entries",
        "INSERT",
        "true",
        Stall::Commit,
    )
    .await
}

/// What one fresh-fixture attempt of a scenario showed.
enum Proof {
    /// The branch that proves the scenario's claim.
    Classified(&'static str),
    /// A load signature, with every backstop asserted: the attempt proves nothing.
    Load(&'static str),
}

/// Runs `attempt` on fresh fixtures until it reaches its classified branch, at most 3 times
/// (the VOID loops' rule), and panics otherwise. Branch lines go past libtest's capture.
async fn proven<F: Future<Output = Proof>>(scenario: &str, attempt: impl Fn() -> F) {
    for n in 1..=3 {
        match attempt().await {
            Proof::Classified(branch) => {
                uncaptured(&format!(
                    "PROVEN scenario={scenario} attempt={n} branch={branch}"
                ));
                return;
            }
            Proof::Load(signature) => uncaptured(&format!(
                "VOID attempt scenario={scenario} attempt={n} load={signature}"
            )),
        }
    }
    panic!("{scenario}: no classified branch within 3 fresh fixtures");
}

/// Our reservation's resolution: the one `action.reserve_remote` event of the attempt that had
/// to resolve an ambiguous COMMIT (a competitor's own attempt elapse is `not_applied`).
fn ours(injected: &Injected) -> Observed {
    let resolved: Vec<Observed> = injected
        .outcomes()
        .into_iter()
        .filter(|seen| {
            seen.op == RESERVE && !matches!(seen.outcome, "completed" | "refused" | "not_applied")
        })
        .collect();
    assert_eq!(resolved.len(), 1, "{resolved:?}");
    resolved[0]
}

/// The one load signature our reservation may show: its resolution elapsed (Q2(b)). A
/// `lapsed` Unknown means the fence died, which nothing does in (q) and (r).
fn elapsed(result: &AppResult<RemoteDispatchObservation>, ours: Observed) -> bool {
    ours.outcome == "unknown"
        && ours.cause == Some("unresolved")
        && matches!(result, Err(error) if unknown(error, RESERVE))
}

/// A competitor's NotApplied counts only as its own attempt timing out.
fn competitor_elapsed(injected: &Injected, result: &AppResult<RemoteDispatchObservation>) -> bool {
    matches!(result, Err(error) if not_applied(error, RESERVE))
        && injected
            .outcomes()
            .iter()
            .any(|seen| seen.op == RESERVE && seen.outcome == "not_applied")
}

fn committed(result: &AppResult<RemoteDispatchObservation>) -> bool {
    matches!(result, Ok(RemoteDispatchObservation::Committed(_)))
}

fn exists(result: &AppResult<RemoteDispatchObservation>) -> bool {
    matches!(
        result,
        Ok(RemoteDispatchObservation::PossibleDispatchExists)
    )
}

/// For failure messages: the observation has no Debug.
fn describe(result: &AppResult<RemoteDispatchObservation>) -> String {
    match result {
        Ok(RemoteDispatchObservation::Committed(_)) => "Committed".into(),
        Ok(RemoteDispatchObservation::PossibleDispatchExists) => "PossibleDispatchExists".into(),
        Ok(RemoteDispatchObservation::ApprovalRequired) => "ApprovalRequired".into(),
        Ok(RemoteDispatchObservation::Interrupted) => "Interrupted".into(),
        Err(error) => format!("{error:?}"),
    }
}

/// Stalls `remote()` in its reservation's COMMIT (aborting it once released unless `applies`)
/// and releases the gate once the resolution queued behind it.
async fn stalled_remote(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &Counted,
    applies: bool,
) -> AppResult<RemoteDispatchObservation> {
    let (mut observer, _) = connect(f).await;
    let mut gate = entry_gate(f).await;
    let first = spawn_remote(f, request, provider);
    let reserving = gate
        .stalled(&mut observer, &[&|| !first.is_finished()])
        .await;
    if !applies {
        gate.abort_on_release(Abort::Backend(reserving)).await;
    }
    let running: [&dyn Fn() -> bool; 1] = [&|| !first.is_finished()];
    resolution_behind(&mut observer, reserving, request.lease, &running).await;
    gate.release().await;
    let first = first.await.unwrap();
    gate.abort_on_release(Abort::Nothing).await;
    first
}

/// (q) A remote reservation stalled in COMMIT is resolved on the entry id minted before its
/// timer. Committed, it is our own reservation, adopted while its fence holds, and the call
/// reaches the provider exactly once. Aborted, it is a definite NotApplied with nothing
/// written. Either way the next dispatch of the invocation never sends a second time.
#[tokio::test]
async fn workflow_action_dispatch_resolution_stalled_reservation_adopted() {
    for applies in [true, false] {
        let scenario = format!("(q) applies={applies}");
        proven(&scenario, || Box::pin(adopted_attempt(applies))).await;
    }
}

async fn adopted_attempt(applies: bool) -> Proof {
    let injected = Injected::mark();
    let (f, request) = gated().await;
    let provider = Counted::new(&f);
    let first = stalled_remote(&f, &request, &provider, applies).await;
    let ours = ours(&injected);
    let proof = match ours.outcome {
        "resolved_adopted" if applies && committed(&first) => Proof::Classified("adopted"),
        "resolved_not_applied"
            if !applies && matches!(&first, Err(error) if not_applied(error, RESERVE)) =>
        {
            Proof::Classified("not_applied")
        }
        _ if elapsed(&first, ours) => Proof::Load("ours=unresolved"),
        _ => panic!("(q) applies={applies}: {ours:?} {}", describe(&first)),
    };
    assert_eq!(remote_rows(&f).await, if applies { (1, 1) } else { (0, 0) });
    let sent = matches!(proof, Proof::Classified("adopted"));
    assert_eq!(provider.calls(), usize::from(sent));
    // The next dispatch replays the one receipt, reserves for the first time (aborted), or
    // (a committed Unknown) sees PossibleDispatchExists: never a second send.
    let next = spawn_remote(&f, &request, &provider).await.unwrap();
    if applies && !sent {
        assert!(exists(&next), "{}", describe(&next));
        assert_eq!(provider.calls(), 0);
    } else {
        assert!(committed(&next), "{}", describe(&next));
        assert_eq!(provider.calls(), 1);
    }
    assert_eq!(remote_rows(&f).await, (1, 1));
    proof
}

/// (q) An adopted reservation whose fence is lost before entry (lease expiry, run cancel):
/// enter_remote re-checks the fence and refuses it, so no provider is ever polled for it.
#[tokio::test]
async fn workflow_action_dispatch_resolution_adopted_reservation_refused_after_fence_loss() {
    for (loss, sql) in [
        (
            "expiry",
            "UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds', lock_expires_at=clock_timestamp()-interval '1 second' WHERE company_id=$1",
        ),
        (
            "cancel",
            "UPDATE workflow_runs SET state='cancelled' WHERE company_id=$1",
        ),
    ] {
        let scenario = format!("(q) fence loss={loss}");
        proven(&scenario, || Box::pin(fence_loss_attempt(sql))).await;
    }
}

async fn fence_loss_attempt(sql: &'static str) -> Proof {
    let injected = Injected::mark();
    let (f, request) = gated().await;
    let (mut observer, _) = connect(&f).await;
    let mut gate = entry_gate(&f).await;
    let reserve = {
        let dispatch = adapter(&f, 0);
        let request = request.clone();
        tokio::spawn(async move { dispatch.reserve_remote(&request, None).await })
    };
    let reserving = gate
        .stalled(&mut observer, &[&|| !reserve.is_finished()])
        .await;
    let running: [&dyn Fn() -> bool; 1] = [&|| !reserve.is_finished()];
    resolution_behind(&mut observer, reserving, request.lease, &running).await;
    gate.release().await;
    let reserved = reserve.await.unwrap();
    gate.abort_on_release(Abort::Nothing).await;
    let ours = ours(&injected);
    assert_eq!(remote_rows(&f).await, (1, 1));
    let reservation = match reserved {
        Ok(RemoteReservationResult::Reserved(reservation))
            if ours.outcome == "resolved_adopted" =>
        {
            reservation
        }
        Err(error)
            if unknown(&error, RESERVE)
                && ours.outcome == "unknown"
                && ours.cause == Some("unresolved") =>
        {
            assert_eq!(facts(&f).await, (1, 0, 0));
            return Proof::Load("ours=unresolved");
        }
        _ => panic!("the committed reservation is adopted: {ours:?}"),
    };
    sqlx::query(sql)
        .bind(request.scope().company.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let Err(error) = adapter(&f, 0).enter_remote(*reservation).await else {
        panic!("enter_remote entered a reservation whose fence is gone")
    };
    assert!(
        matches!(&error, AppError::Conflict(message) if message == REFUSAL),
        "{error:?}"
    );
    assert_eq!(facts(&f).await, (1, 0, 0));
    assert_eq!(remote_rows(&f).await, (1, 1));
    Proof::Classified("adopted_then_refused")
}

/// (r) Competing reservers of one invocation under one fence against a slow original COMMIT.
/// The competitor queues on the run row either before the resolution (spawned at 3A/4, while
/// the original is stalled: it then sees the committed marker before we adopt) or after it.
/// At most one reservation is ever sent, and the classified branches send exactly one.
#[tokio::test]
async fn workflow_action_dispatch_resolution_competing_reservers_one_dispatch() {
    for (applies, early) in [(true, true), (true, false), (false, true), (false, false)] {
        let scenario = format!("(r) applies={applies} early={early}");
        proven(&scenario, || Box::pin(compete_attempt(applies, early))).await;
    }
}

async fn compete_attempt(applies: bool, early: bool) -> Proof {
    let injected = Injected::mark();
    let (f, request) = gated().await;
    let provider = Counted::new(&f);
    let (first, second) = Box::pin(compete(&f, &request, &provider, applies, early)).await;
    let ours = ours(&injected);
    // Ours: classified, or only its resolution elapsed. Never lapsed, never NotApplied when
    // the reservation applied: nothing here closes the fence.
    let ours_classified = match ours.outcome {
        "resolved_adopted" if applies && committed(&first) => true,
        "resolved_not_applied"
            if !applies && matches!(&first, Err(error) if not_applied(error, RESERVE)) =>
        {
            true
        }
        _ if elapsed(&first, ours) => false,
        _ => panic!("(r) ours: {ours:?} {}", describe(&first)),
    };
    // The competitor: it finds the committed marker (or its receipt), or reserves after the
    // abort and sends itself; or only its own attempt timed out.
    let competitor_classified = if applies {
        committed(&second) || exists(&second)
    } else {
        committed(&second)
    };
    assert!(
        competitor_classified || competitor_elapsed(&injected, &second),
        "(r) competitor: {}",
        describe(&second)
    );
    // Every branch: at most one entry, sent at most once, and nothing receipted unsent.
    let (_, entries) = remote_rows(&f).await;
    let calls = i64::try_from(provider.calls()).unwrap();
    assert!(entries <= 1, "one entry per attempt");
    assert!(
        calls <= entries,
        "only a committed entry is ever sent, at most once"
    );
    assert!(facts(&f).await.1 <= calls, "no receipt without a send");
    match (ours_classified, competitor_classified) {
        (true, true) => {
            assert_eq!(provider.calls(), 1);
            assert_eq!(remote_rows(&f).await, (1, 1));
            Proof::Classified(ours.outcome)
        }
        (true, false) => Proof::Load("competitor=own_attempt_elapsed"),
        (false, _) => Proof::Load("ours=unresolved"),
    }
}

/// Stalls the first dispatch in COMMIT and queues a competing dispatch of the same request on
/// the run row, before (`early`) or after the first's resolution; then releases the gate.
async fn compete(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &Counted,
    applies: bool,
    early: bool,
) -> (
    AppResult<RemoteDispatchObservation>,
    AppResult<RemoteDispatchObservation>,
) {
    let lease = request.lease;
    let (mut observer, _) = connect(f).await;
    let mut gate = entry_gate(f).await;
    let started = Instant::now();
    let first = spawn_remote(f, request, provider);
    let reserving = gate
        .stalled(&mut observer, &[&|| !first.is_finished()])
        .await;
    if !applies {
        gate.abort_on_release(Abort::Backend(reserving)).await;
    }
    let second = if early {
        // Spawned at 3A/4: before the first's attempt elapses, keeping most of its own A
        // for after the release.
        tokio::time::sleep_until(started + lease.attempt_bound() * 3 / 4).await;
        let second = spawn_remote(f, request, provider);
        let running: [&dyn Fn() -> bool; 2] = [&|| !first.is_finished(), &|| !second.is_finished()];
        let ahead = waiters(&mut observer, reserving, 1, "competitor", &running).await[0];
        let queue = waiters(
            &mut observer,
            reserving,
            2,
            "competitor, then resolution",
            &running,
        )
        .await;
        assert_eq!(
            queue[0], ahead,
            "the competitor queued before the resolution"
        );
        second
    } else {
        let running: [&dyn Fn() -> bool; 1] = [&|| !first.is_finished()];
        resolution_behind(&mut observer, reserving, lease, &running).await;
        let second = spawn_remote(f, request, provider);
        let running: [&dyn Fn() -> bool; 2] = [&|| !first.is_finished(), &|| !second.is_finished()];
        waiters(
            &mut observer,
            reserving,
            2,
            "resolution, then competitor",
            &running,
        )
        .await;
        second
    };
    gate.release().await;
    let outcomes = (first.await.unwrap(), second.await.unwrap());
    gate.abort_on_release(Abort::Nothing).await;
    outcomes
}

/// Serializes on the run row, as the production owners do, and closes our fence: a run cancel
/// (the cancel retirement owner, then the run state) or the lease's expiry.
async fn lapse(pool: PgPool, scope: ActivationRequest, cancel: bool) {
    let mut conn = pool.acquire().await.unwrap();
    let mut tx = sqlx::Connection::begin(&mut *conn).await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    if cancel {
        retire_jobs(&mut tx, scope.company, scope.run, RetirementCause::Cancel)
            .await
            .unwrap();
    }
    let sql = if cancel {
        "UPDATE workflow_runs SET state='cancelled' WHERE id=$1"
    } else {
        "UPDATE background_tasks SET lock_expires_at=clock_timestamp()-interval '1 second' WHERE workflow_execution_id IN (SELECT id FROM workflow_executions WHERE run_id=$1)"
    };
    sqlx::query(sql)
        .bind(scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// (s) The reservation commits, then a third party serializes on the run row between that
/// COMMIT and the resolution: a run cancel, or the lease's expiry. Our entry is proven but its
/// fence no longer holds, so the call stays Unknown (cause `lapsed`) and nothing is sent.
///
/// The order is fixed through the pool's FIFO, as in (l): the pool is exhausted beside the
/// stalled reservation and the third party queues for a connection before the attempt
/// elapses, so the resolution gets one only after the third party committed.
#[tokio::test]
async fn workflow_action_dispatch_resolution_lapsed_reservation_stays_unknown() {
    for cancel in [true, false] {
        let scenario = format!("(s) cancel={cancel}");
        proven(&scenario, || Box::pin(lapsed_attempt(cancel))).await;
    }
}

async fn lapsed_attempt(cancel: bool) -> Proof {
    let injected = Injected::mark();
    let (f, request) = gated().await;
    let provider = Counted::new(&f);
    let (mut observer, _) = connect(&f).await;
    let mut gate = entry_gate(&f).await;
    let first = spawn_remote(&f, &request, &provider);
    let reserving = gate
        .stalled(&mut observer, &[&|| !first.is_finished()])
        .await;
    let pool = f.persistence().pool().clone();
    let mut held = Vec::new();
    for _ in 1..pool.options().get_max_connections() {
        held.push(pool.acquire().await.unwrap());
    }
    let third_party = tokio::spawn(lapse(pool, request.fence.scope, cancel));
    let running: [&dyn Fn() -> bool; 1] = [&|| !first.is_finished()];
    attempt_elapsed(&mut observer, reserving, request.lease, &running).await;
    assert!(
        !third_party.is_finished(),
        "the third party waits for a connection"
    );
    gate.release().await;
    third_party.await.unwrap();
    let first = first.await.unwrap();
    drop(held);
    gate.abort_on_release(Abort::Nothing).await;
    let ours = ours(&injected);
    let Err(error) = &first else {
        panic!("a lapsed reservation is never adopted: {ours:?}")
    };
    assert!(unknown(error, RESERVE), "{error:?}");
    assert_eq!(ours.outcome, "unknown");
    let proof = match ours.cause {
        Some("lapsed") => Proof::Classified("lapsed"),
        Some("unresolved") => Proof::Load("ours=unresolved"),
        _ => panic!("(s) {ours:?}"),
    };
    assert_eq!(provider.calls(), 0);
    assert_eq!(remote_rows(&f).await, (1, 1));
    Box::pin(lapsed_backstops(&f, &request, &provider, cancel)).await;
    proof
}

/// Nothing was sent and nothing can be. The cancel retired our attempt. The expiry is charged
/// once and, since our entry may have reached the provider, retired as effect-outcome-unknown
/// into reconciliation: a competing claimant gets no lease, and the dead fence's own
/// dispatch is refused before any provider.
async fn lapsed_backstops(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &Counted,
    cancel: bool,
) {
    let scope = request.fence.scope;
    let rows = attempts(f, scope).await;
    assert_eq!(rows.len(), 1);
    if cancel {
        assert_eq!(rows[0].status, "failed");
        assert_eq!(rows[0].workflow_retirement.as_deref(), Some("cancel"));
    } else {
        assert!(
            f.persistence()
                .settled()
                .retire_expired_io(scope, request.lease)
                .await
                .unwrap()
        );
        let rows = attempts(f, scope).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].stop_reason.as_deref(), Some("lease_lost"));
        let safety: Option<String> = sqlx::query_scalar(
            "SELECT workflow_retry_safety FROM task_attempts WHERE task_id=$1 AND attempt_number=1",
        )
        .bind(scope.job.0)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
        assert_eq!(safety.as_deref(), Some("unknown"));
        due(f, scope).await;
        let claim = f
            .persistence()
            .settled()
            .claim_io(scope, worker(), request.lease)
            .await
            .unwrap();
        assert!(claim.is_none(), "no new owner before reconciliation");
    }
    let refused = spawn_remote(f, request, provider).await.unwrap();
    assert!(
        matches!(&refused, Err(AppError::Conflict(message)) if message == REFUSAL),
        "the dead fence dispatches nothing"
    );
    assert_eq!(provider.calls(), 0);
    assert_eq!(remote_rows(f).await, (1, 1));
}
