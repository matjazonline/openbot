//! Outcome resolution of the bounded lease operations under real competing backends.
//!
//! Each test stalls one operation's COMMIT (or a statement) on the injection gate, lets the
//! client bound elapse, and checks the classification the read-only resolution reaches
//! under the original's own locks: applied, not applied or unknown. Delays come from the
//! lease policy and from observed lock waits, never from fixed sleeps.
use super::resolution_gate::*;
use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;

/// The bound hands the server its timeout as whole milliseconds, floored, so the server can
/// cancel a lock wait up to a millisecond before the client deadline (WF-P08-F094). The outcome
/// is "unknown" either way; only the exact lower bound needs the slack.
const SERVER_TIMEOUT_GRANULARITY: Duration = Duration::from_millis(1);
const CLAIM_ROW: &str = "true";
const RETIREMENT: &str = "OLD.status='processing' AND NEW.status<>'processing'";
const RENEWAL: &str = "OLD.status='processing' AND NEW.status='processing' AND NEW.lock_expires_at > OLD.lock_expires_at";

fn spawn_claim(
    f: &AdmissionFixture,
    scope: ActivationRequest,
    worker: WorkflowWorkerId,
) -> tokio::task::JoinHandle<AppResult<Option<ClaimedWorkflow>>> {
    let p = f.persistence().clone();
    tokio::spawn(async move { p.claim_io(scope, worker, gated_policy()).await })
}

async fn claimed(f: &AdmissionFixture, scope: ActivationRequest) -> ClaimedWorkflow {
    f.persistence()
        .settled()
        .claim_io(scope, worker(), gated_policy())
        .await
        .unwrap()
        .unwrap()
}

fn processing(rows: &[AttemptRow]) -> usize {
    rows.iter().filter(|row| row.status == "processing").count()
}

/// (a) The winner's COMMIT outlives A: its resolution adopts exactly the committed
/// generation, the loser sees no claim, and exactly one attempt is charged once.
#[tokio::test]
async fn workflow_lease_resolution_competing_claimants_adopt_committed_generation() {
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    let (mut observer, _) = connect(&f).await;
    let mut gate = Gate::arm(&f, "task_attempts", "INSERT", CLAIM_ROW, Stall::Commit).await;
    let left = spawn_claim(&f, scope, worker());
    let right = spawn_claim(&f, scope, worker());
    let winner = gate
        .stalled(
            &mut observer,
            &[&|| !left.is_finished() || !right.is_finished()],
        )
        .await;
    // The loser waits behind the stalled winner (and gives up at its own A); the winner's
    // attempt elapses at A and its resolution queues on the winner's own run lock.
    waiters(&mut observer, winner, 1, "losing claimant", &[]).await;
    resolution_behind(
        &mut observer,
        winner,
        gated_policy(),
        &[&|| !left.is_finished() || !right.is_finished()],
    )
    .await;
    assert_eq!(processing(&attempts(&f, scope).await), 0);
    gate.release().await;
    let mut adopted = None;
    for result in [left.await.unwrap(), right.await.unwrap()] {
        match result {
            Ok(Some(claim)) => assert!(adopted.replace(claim).is_none(), "two claims"),
            Ok(None) => {}
            Err(error) => assert!(
                matches!(error, AppError::DatabaseTimeout(_)),
                "loser: {error:?}"
            ),
        }
    }
    let adopted = adopted.expect("the stalled claim committed and was adopted");
    let saved = job(&f, scope).await;
    assert_eq!(saved.execution_generation, Some(adopted.fence.generation.0));
    assert_eq!(saved.worker_id, Some(adopted.fence.worker.0));
    let rows = attempts(&f, scope).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].execution_generation, adopted.fence.generation.0);
    assert_eq!(adopted.fence.attempt, WorkflowAttempt(1));
    assert!(adopted.window.expires > Instant::now());
    // The adopted claim is fully owned: its release charges the attempt exactly once.
    assert!(
        f.persistence()
            .settled()
            .release_io(
                adopted.fence,
                gated_policy(),
                LeaseReleaseCause::LocalInterruption
            )
            .await
            .unwrap()
    );
    assert_eq!(job(&f, scope).await.retry_count, saved.retry_count + 1);
}

/// (a2) A claim whose COMMIT was sent and then aborted is settled with nothing owned
/// (`Ok(None)`), never "not applied": a claim can commit other writes (expiry retirement,
/// budget refusal, activation) without an attempt row, so the absent row proves only that
/// nothing is owned. Here the aborted transaction wrote nothing, and the job is claimed
/// afresh.
#[tokio::test]
async fn workflow_lease_resolution_aborted_claim_owns_nothing() {
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    let before = snapshot(&f).await;
    let (mut observer, _) = connect(&f).await;
    let mut gate = Gate::arm(&f, "task_attempts", "INSERT", CLAIM_ROW, Stall::Commit).await;
    let claim = spawn_claim(&f, scope, worker());
    let claiming = gate
        .stalled(&mut observer, &[&|| !claim.is_finished()])
        .await;
    gate.abort_on_release(Abort::Backend(claiming)).await;
    resolution_behind(
        &mut observer,
        claiming,
        gated_policy(),
        &[&|| !claim.is_finished()],
    )
    .await;
    gate.release().await;
    assert!(claim.await.unwrap().unwrap().is_none());
    assert!(attempts(&f, scope).await.is_empty());
    assert_eq!(snapshot(&f).await, before);
    gate.abort_on_release(Abort::Nothing).await;
    let claim = claimed(&f, scope).await;
    assert_eq!(claim.fence.attempt, WorkflowAttempt(1));
}

/// (b) A renewal stalled in COMMIT across its old expiry: a claimant arriving after that
/// expiry waits on the run and finds the renewed lease; the renewal resolves to the
/// authoritative server window and no second attempt exists.
#[tokio::test]
async fn workflow_lease_resolution_renewal_against_late_claimant() {
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    let owned = claimed(&f, scope).await;
    let (mut observer, _) = connect(&f).await;
    // The old expiry falls at the renewal's attempt bound, inside its stalled COMMIT.
    let old_expiry: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+make_interval(secs => $2) WHERE id=$1 RETURNING lock_expires_at")
        .bind(scope.job.0).bind(gated_policy().attempt_bound().as_secs_f64()).fetch_one(f.persistence().pool()).await.unwrap();
    let mut gate = Gate::arm(&f, "background_tasks", "UPDATE", RENEWAL, Stall::Commit).await;
    let p = f.persistence().clone();
    let renewal = tokio::spawn(async move { p.renew_io(owned.fence, gated_policy()).await });
    let renewing = gate
        .stalled(&mut observer, &[&|| !renewal.is_finished()])
        .await;
    let guard = Instant::now();
    loop {
        let passed: bool = sqlx::query_scalar("SELECT clock_timestamp() > $1")
            .bind(old_expiry)
            .fetch_one(&mut observer)
            .await
            .unwrap();
        if passed {
            break;
        }
        assert!(guard.elapsed() < HANG_GUARD);
        assert!(!renewal.is_finished(), "renewal ended inside its stall");
        tokio::time::sleep(gated_policy().resolution_bound() / 20).await;
    }
    let claimant = spawn_claim(&f, scope, worker());
    // Both the late claimant and the renewal's resolution queue on the renewal's locks.
    waiters(
        &mut observer,
        renewing,
        2,
        "late claimant and renewal resolution",
        &[&|| !claimant.is_finished(), &|| !renewal.is_finished()],
    )
    .await;
    resolution_behind(
        &mut observer,
        renewing,
        gated_policy(),
        &[&|| !renewal.is_finished()],
    )
    .await;
    gate.release().await;
    let window = renewal
        .await
        .unwrap()
        .unwrap()
        .expect("the renewal committed");
    assert!(claimant.await.unwrap().unwrap().is_none());
    let remaining: f64 = sqlx::query_scalar("SELECT extract(epoch FROM lock_expires_at-clock_timestamp())::float8 FROM background_tasks WHERE id=$1 AND lock_expires_at > $2")
        .bind(scope.job.0).bind(old_expiry).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(window.expires <= Instant::now() + Duration::from_secs_f64(remaining));
    let rows = attempts(&f, scope).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(processing(&rows), 1);
    assert_eq!(rows[0].execution_generation, owned.fence.generation.0);
}

/// (c) A release stalled in COMMIT against a concurrent claim: the release resolves as
/// applied, and a next attempt exists only after the release committed, never beside it.
#[tokio::test]
async fn workflow_lease_resolution_release_against_concurrent_claim() {
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    let owned = claimed(&f, scope).await;
    let (mut observer, _) = connect(&f).await;
    let mut gate = Gate::arm(&f, "task_attempts", "UPDATE", RETIREMENT, Stall::Commit).await;
    let p = f.persistence().clone();
    let release = tokio::spawn(async move {
        p.release_io(
            owned.fence,
            gated_policy(),
            LeaseReleaseCause::LocalInterruption,
        )
        .await
    });
    let releasing = gate
        .stalled(&mut observer, &[&|| !release.is_finished()])
        .await;
    resolution_behind(
        &mut observer,
        releasing,
        gated_policy(),
        &[&|| !release.is_finished()],
    )
    .await;
    // The claimant competes from here, so its own attempt bound outlasts the stall.
    let claimant = spawn_claim(&f, scope, worker());
    waiters(
        &mut observer,
        releasing,
        2,
        "release resolution and concurrent claimant",
        &[&|| !claimant.is_finished(), &|| !release.is_finished()],
    )
    .await;
    assert_eq!(processing(&attempts(&f, scope).await), 1);
    gate.release().await;
    assert!(release.await.unwrap().unwrap(), "the release committed");
    let next = match claimant.await.unwrap() {
        Ok(next) => next,
        Err(error) => {
            assert!(
                matches!(error, AppError::DatabaseTimeout(_)),
                "claimant: {error:?}"
            );
            None
        }
    };
    let next = match next {
        Some(next) => next,
        None => {
            due(&f, scope).await;
            claimed(&f, scope).await
        }
    };
    assert_eq!(next.fence.attempt, WorkflowAttempt(2));
    let rows = attempts(&f, scope).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(processing(&rows), 1);
    assert_eq!(rows[0].status, "failed");
    assert_eq!(rows[0].workflow_retirement.as_deref(), Some("live"));
    assert_eq!(rows[1].attempt_number, 2);
    assert_eq!(rows[1].status, "processing");
}

/// The single release retry runs inside the first attempt's deadline T, so the whole call
/// ends inside the supervision guard G that supervise wraps it in: a stalled COMMIT that
/// aborts is retried and applies; a retry stalled past T is unknown at T, not at T + A.
#[tokio::test]
async fn workflow_lease_resolution_release_retry_stays_inside_deadline() {
    let _injected = Injected::mark();
    for retry_stalls in [false, true] {
        let (f, scope) = fixture().await;
        let owned = claimed(&f, scope).await;
        let (mut observer, _) = connect(&f).await;
        let mut gate = Gate::arm(&f, "task_attempts", "UPDATE", RETIREMENT, Stall::Commit).await;
        gate.abort_on_release(Abort::Every).await;
        let p = f.persistence().clone();
        let started = Instant::now();
        let release = tokio::spawn(async move {
            p.release_io(
                owned.fence,
                gated_policy(),
                LeaseReleaseCause::LocalInterruption,
            )
            .await
        });
        let first = gate
            .stalled(&mut observer, &[&|| !release.is_finished()])
            .await;
        resolution_behind(
            &mut observer,
            first,
            gated_policy(),
            &[&|| !release.is_finished()],
        )
        .await;
        // The first COMMIT aborts; its resolution finds the fence live and retries.
        gate.release().await;
        gate.retake().await;
        if retry_stalls {
            let retry = gate
                .stalled(&mut observer, &[&|| !release.is_finished()])
                .await;
            assert_ne!(
                retry, first,
                "the first release ended before the gate was retaken"
            );
        } else {
            gate.abort_on_release(Abort::Nothing).await;
            gate.release().await;
        }
        let result = release.await.unwrap();
        let elapsed = started.elapsed();
        assert!(
            elapsed < gated_policy().supervision_guard(),
            "release took {elapsed:?}"
        );
        if retry_stalls {
            let error = result.unwrap_err();
            assert!(unknown(&error, "lease.release"), "{error:?}");
            assert!(
                elapsed + SERVER_TIMEOUT_GRANULARITY >= gated_policy().persistence_deadline(),
                "{elapsed:?}"
            );
            // The stalled retry aborts too: nothing was released, the fence still owns.
            gate.release().await;
            gate.retake().await;
            gate.abort_on_release(Abort::Nothing).await;
            gate.release().await;
            assert_eq!(processing(&attempts(&f, scope).await), 1);
            assert!(
                f.persistence()
                    .settled()
                    .release_io(
                        owned.fence,
                        gated_policy(),
                        LeaseReleaseCause::LocalInterruption
                    )
                    .await
                    .unwrap()
            );
        } else {
            assert!(result.unwrap(), "the retry applied");
        }
        let rows = attempts(&f, scope).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "failed");
        assert_eq!(rows[0].workflow_retirement.as_deref(), Some("live"));
    }
}

/// (d) An elapse in the statement phase is definitely not applied: the ROLLBACK is queued
/// before any COMMIT, so no attempt exists and the job stays claimable.
#[tokio::test]
async fn workflow_lease_resolution_statement_phase_elapse_not_applied() {
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    assert_eq!(job(&f, scope).await.status, "pending");
    let before = snapshot(&f).await;
    let mut gate = Gate::arm(
        &f,
        "task_attempts",
        "INSERT",
        CLAIM_ROW,
        Stall::Statement {
            table: "workflow_runs",
            events: "UPDATE",
            when: "OLD.state='queued' AND NEW.state='running'",
        },
    )
    .await;
    let error = f
        .persistence()
        .claim_io(scope, worker(), gated_policy())
        .await
        .unwrap_err();
    assert!(not_applied(&error, "lease.claim"), "{error:?}");
    assert_eq!(snapshot(&f).await, before);
    gate.drain().await;
    assert_eq!(snapshot(&f).await, before);
    let claim = claimed(&f, scope).await;
    assert_eq!(claim.fence.attempt, WorkflowAttempt(1));
}

/// (e) A COMMIT stalled past T is unknown. Once it lands the lease is well formed, and
/// expiry retirement charges it exactly once.
#[tokio::test]
async fn workflow_lease_resolution_stall_beyond_deadline_unknown_then_one_expiry() {
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    let mut gate = Gate::arm(&f, "task_attempts", "INSERT", CLAIM_ROW, Stall::Commit).await;
    let started = Instant::now();
    let error = f
        .persistence()
        .claim_io(scope, worker(), gated_policy())
        .await
        .unwrap_err();
    let elapsed = started.elapsed();
    assert!(unknown(&error, "lease.claim"), "{error:?}");
    assert!(
        elapsed + SERVER_TIMEOUT_GRANULARITY >= gated_policy().persistence_deadline(),
        "{elapsed:?}"
    );
    assert!(elapsed < gated_policy().supervision_guard(), "{elapsed:?}");
    gate.drain().await;
    let saved = job(&f, scope).await;
    let rows = attempts(&f, scope).await;
    assert_eq!(saved.status, "processing");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "processing");
    assert_eq!(
        saved.execution_generation,
        Some(rows[0].execution_generation)
    );
    assert_eq!(saved.worker_id, Some(rows[0].worker_id));
    expire(&f, scope).await;
    let p = f.persistence();
    assert!(
        p.settled()
            .retire_expired_io(scope, gated_policy())
            .await
            .unwrap()
    );
    assert!(
        !p.settled()
            .retire_expired_io(scope, gated_policy())
            .await
            .unwrap()
    );
    let rows = attempts(&f, scope).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].stop_reason.as_deref(), Some("lease_lost"));
    assert_eq!(job(&f, scope).await.retry_count, saved.retry_count + 1);
}

/// (f) An exhausted pool is not applied within the acquire bound, and writes nothing.
#[tokio::test]
async fn workflow_lease_resolution_pool_exhausted_not_applied_within_acquire_bound() {
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    let before = snapshot(&f).await;
    let pool = f.persistence().pool();
    let mut held = Vec::new();
    for _ in 0..pool.options().get_max_connections() {
        held.push(pool.acquire().await.unwrap());
    }
    let started = Instant::now();
    let error = f
        .persistence()
        .claim_io(scope, worker(), gated_policy())
        .await
        .unwrap_err();
    let elapsed = started.elapsed();
    assert!(not_applied(&error, "lease.claim"), "{error:?}");
    assert!(elapsed >= gated_policy().acquire_bound(), "{elapsed:?}");
    assert!(elapsed < gated_policy().attempt_bound(), "{elapsed:?}");
    drop(held);
    assert_eq!(snapshot(&f).await, before);
}

/// (j) Read-only work waits out a stalled renewal, returns the value after it resolves and
/// never commits: its backend's last statement is ROLLBACK.
#[tokio::test]
async fn workflow_lease_resolution_read_only_validate_rolls_back_under_stalled_renewal() {
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    let owned = claimed(&f, scope).await;
    let (mut observer, _) = connect(&f).await;
    // A one-connection pool pins the validation to a backend the test can name. It is the one
    // deliberately unguarded pool (WF-C004 r2): the leaked-transaction guard runs a statement on
    // every release, which would replace the ROLLBACK this test reads back. Nothing here cancels
    // a `begin`.
    let single = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*f.persistence().pool().connect_options()).clone())
        .await
        .unwrap();
    let reader_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&single)
        .await
        .unwrap();
    let mut gate = Gate::arm(&f, "background_tasks", "UPDATE", RENEWAL, Stall::Commit).await;
    let p = f.persistence().clone();
    let renewal = tokio::spawn(async move { p.renew_io(owned.fence, gated_policy()).await });
    let renewing = gate
        .stalled(&mut observer, &[&|| !renewal.is_finished()])
        .await;
    let reader = PostgresPersistence::new(single.clone());
    let validation =
        tokio::spawn(async move { reader.validate_io(owned.fence, gated_policy()).await });
    let queued = waiters(
        &mut observer,
        renewing,
        2,
        "validation and renewal resolution",
        &[&|| !validation.is_finished(), &|| !renewal.is_finished()],
    )
    .await;
    assert!(queued.contains(&reader_pid), "{queued:?} {reader_pid}");
    gate.release().await;
    assert!(validation.await.unwrap().unwrap());
    assert!(renewal.await.unwrap().unwrap().is_some());
    sqlx::query("SELECT pg_stat_clear_snapshot()")
        .execute(&mut observer)
        .await
        .unwrap();
    let (state, query): (String, String) =
        sqlx::query_as("SELECT state, query FROM pg_stat_activity WHERE pid=$1")
            .bind(reader_pid)
            .fetch_one(&mut observer)
            .await
            .unwrap();
    assert_eq!((state.as_str(), query.as_str()), ("idle", "ROLLBACK"));
}

/// (l) The claim committed, then a third party serialized on the run before the resolution:
/// the resolution classifies on the immutable attempt row (applied, nothing owned), never
/// on the job's current state, and reports no "not applied".
///
/// Every claim bumps the run's revision, so row-lock waiters on the run are not granted in
/// queue order. The order is fixed one step earlier instead: the pool is exhausted beside
/// the stalled claim and the third party queues for a connection (a FIFO semaphore) before
/// the claim's attempt elapses, so the resolution gets a connection only after the third
/// party committed and returned it.
///
/// Under load the resolution may end Unknown (Q2(b)); both branches assert the backstops
/// (cancel retirement; exactly one Expired) and log which branch ran. Runs of this test
/// alone must still show the classified branch, which is what proves D-2.
#[tokio::test]
async fn workflow_lease_resolution_third_party_before_resolution_owns_nothing() {
    let _injected = Injected::mark();
    for cancel in [true, false] {
        let (f, scope) = fixture().await;
        let (mut observer, _) = connect(&f).await;
        let mut gate = Gate::arm(&f, "task_attempts", "INSERT", CLAIM_ROW, Stall::Commit).await;
        let claim = spawn_claim(&f, scope, worker());
        let claiming = gate
            .stalled(&mut observer, &[&|| !claim.is_finished()])
            .await;
        let pool = f.persistence().pool().clone();
        let mut held = Vec::new();
        for _ in 1..pool.options().get_max_connections() {
            held.push(pool.acquire().await.unwrap());
        }
        let third_party = tokio::spawn(async move {
            let mut conn = pool.acquire().await.unwrap();
            let mut tx = sqlx::Connection::begin(&mut *conn).await.unwrap();
            sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
                .bind(scope.run.as_uuid())
                .execute(&mut *tx)
                .await
                .unwrap();
            if cancel {
                // The production cancel's retirement owner, then the run state.
                crate::adapters::persistence::workflow::maintenance::retire_jobs(
                    &mut tx,
                    scope.company,
                    scope.run,
                    crate::adapters::persistence::workflow::maintenance::RetirementCause::Cancel,
                )
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
        });
        // The claim's client bound started before its transaction did: once the stalled
        // transaction is older than A, the attempt has elapsed and its resolution queues for
        // a connection behind the third party (this runtime polls it while the test waits).
        attempt_elapsed(
            &mut observer,
            claiming,
            gated_policy(),
            &[&|| !claim.is_finished()],
        )
        .await;
        assert!(
            !third_party.is_finished(),
            "the third party waits for a connection"
        );
        gate.release().await;
        third_party.await.unwrap();
        // Q2(b): after A only R remains, and a = R for every lease, so the stalled COMMIT,
        // the third party's COMMIT and the resolution's read must all fit in R. On a loaded
        // host the resolution may end Unknown instead, the honest outcome; then the expiry
        // and cancel backstops below must hold all the same. Neither branch holds a fence.
        let branch = match claim.await.unwrap() {
            Ok(None) => "classified",
            Err(error) if unknown(&error, "lease.claim") => "unknown",
            other => panic!("(l) neither classified nor unknown: {other:?}"),
        };
        eprintln!("(l) cancel={cancel} branch={branch}");
        drop(held);
        let rows = attempts(&f, scope).await;
        assert_eq!(rows.len(), 1);
        if cancel {
            // Retired by the cancel, not by any resolution.
            assert_eq!(rows[0].status, "failed");
            assert_eq!(rows[0].workflow_retirement.as_deref(), Some("cancel"));
        } else {
            let p = f.persistence();
            assert!(
                p.settled()
                    .retire_expired_io(scope, gated_policy())
                    .await
                    .unwrap()
            );
            let rows = attempts(&f, scope).await;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].stop_reason.as_deref(), Some("lease_lost"));
            assert_eq!(job(&f, scope).await.retry_count, 1);
        }
    }
}

/// (m) A statement-phase lock wait ends at the server bound (what remains of A): the claim
/// is definitely not applied, its wait is gone at once, and the next claimant proceeds.
#[tokio::test]
async fn workflow_lease_resolution_lock_wait_ends_at_server_bound() {
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    let before = snapshot(&f).await;
    let (mut observer, _) = connect(&f).await;
    let (mut holder, holder_pid) = connect(&f).await;
    let mut held = sqlx::Connection::begin(&mut holder).await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(scope.run.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    let started = Instant::now();
    let error = f
        .persistence()
        .claim_io(scope, worker(), gated_policy())
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::DatabaseTimeout(_)), "{error:?}");
    assert!(started.elapsed() < gated_policy().persistence_deadline());
    // No orphaned server transaction keeps waiting (or holding) behind the client.
    no_waiter_behind(&mut observer, holder_pid, gated_policy().resolution_bound()).await;
    held.rollback().await.unwrap();
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(claimed(&f, scope).await.fence.attempt, WorkflowAttempt(1));
}

/// (o) A completion stalled in COMMIT against a claimant: the resolution replays the saved
/// step exactly once when it applied; when it aborted it is not applied, and only then does
/// expiry retire the attempt.
#[tokio::test]
async fn workflow_lease_resolution_completion_replays_or_expires_once() {
    use crate::application::workflow::completion::*;
    let _injected = Injected::mark();
    for applies in [true, false] {
        let (f, scope) = fixture().await;
        let owned = claimed(&f, scope).await;
        let (mut observer, _) = connect(&f).await;
        let mut gate = Gate::arm(
            &f,
            "task_attempts",
            "UPDATE",
            "OLD.status='processing' AND NEW.status='completed'",
            Stall::Commit,
        )
        .await;
        let output = json!({"items":[],"token_count":0,"omitted":0});
        let result = FencedWorkflowResult {
            policy: gated_policy(),
            fence: owned.fence,
            output: output.clone(),
        };
        let p = f.persistence().clone();
        let completion = tokio::spawn(async move { p.complete_io(result).await });
        let completing = gate
            .stalled(&mut observer, &[&|| !completion.is_finished()])
            .await;
        if !applies {
            gate.abort_on_release(Abort::Backend(completing)).await;
        }
        resolution_behind(
            &mut observer,
            completing,
            gated_policy(),
            &[&|| !completion.is_finished()],
        )
        .await;
        let claimant = spawn_claim(&f, scope, worker());
        waiters(
            &mut observer,
            completing,
            2,
            "completion resolution and claimant",
            &[&|| !completion.is_finished(), &|| !claimant.is_finished()],
        )
        .await;
        gate.release().await;
        let completed = completion.await.unwrap();
        assert!(claimant.await.unwrap().unwrap().is_none());
        let events: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workflow_run_events WHERE event_kind='io_step_completed'",
        )
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
        let p = f.persistence();
        if applies {
            let step = completed.unwrap().expect("the completion committed");
            // Resolved through the read-only replay of the saved step.
            assert_eq!(step.disposition, CommitDisposition::Replayed);
            assert_eq!(step.output, output);
            assert_eq!(events, 1);
            assert!(
                !p.settled()
                    .retire_expired_io(scope, gated_policy())
                    .await
                    .unwrap()
            );
            let rows = attempts(&f, scope).await;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].status, "completed");
        } else {
            let error = completed.unwrap_err();
            assert!(not_applied(&error, "completion"), "{error:?}");
            assert_eq!(events, 0);
            expire(&f, scope).await;
            assert!(
                p.settled()
                    .retire_expired_io(scope, gated_policy())
                    .await
                    .unwrap()
            );
            assert!(
                !p.settled()
                    .retire_expired_io(scope, gated_policy())
                    .await
                    .unwrap()
            );
            let rows = attempts(&f, scope).await;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].stop_reason.as_deref(), Some("lease_lost"));
        }
    }
}

/// (o2) A completion that commits a retirement instead of a step (the handler's result is
/// invalid) and whose COMMIT outlives A: the execution is not completed, so the resolution
/// classifies on our attempt row, which the original retired. Settled, nothing owned; never
/// "not applied", which would claim that nothing was written.
#[tokio::test]
async fn workflow_lease_resolution_completion_retirement_owns_nothing() {
    use crate::application::workflow::completion::*;
    let _injected = Injected::mark();
    let (f, scope) = fixture().await;
    let owned = claimed(&f, scope).await;
    let (mut observer, _) = connect(&f).await;
    let mut gate = Gate::arm(&f, "task_attempts", "UPDATE", RETIREMENT, Stall::Commit).await;
    let result = FencedWorkflowResult {
        policy: gated_policy(),
        fence: owned.fence,
        output: json!({"items":[],"token_count":-1,"omitted":0}),
    };
    let p = f.persistence().clone();
    let completion = tokio::spawn(async move { p.complete_io(result).await });
    let completing = gate
        .stalled(&mut observer, &[&|| !completion.is_finished()])
        .await;
    resolution_behind(
        &mut observer,
        completing,
        gated_policy(),
        &[&|| !completion.is_finished()],
    )
    .await;
    gate.release().await;
    assert!(completion.await.unwrap().unwrap().is_none());
    let rows = attempts(&f, scope).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "failed");
    assert_eq!(rows[0].workflow_retirement.as_deref(), Some("live"));
}
