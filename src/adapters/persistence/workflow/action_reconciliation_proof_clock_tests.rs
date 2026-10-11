//! DB-clock proof lifetime and owned verifier cancellation.
use super::*;

struct ActiveVerification(Arc<AtomicUsize>);
impl Drop for ActiveVerification {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
struct ControlledVerifier {
    ledger: Arc<LedgerVerifier>,
    calls: AtomicUsize,
    active: Arc<AtomicUsize>,
    lifetime: Option<Duration>,
    pause: Option<(Arc<Notify>, Arc<Notify>)>,
    stall: bool,
}
impl ControlledVerifier {
    fn new(ledger: Arc<LedgerVerifier>) -> Self {
        Self {
            ledger,
            calls: AtomicUsize::new(0),
            active: Arc::new(AtomicUsize::new(0)),
            lifetime: None,
            pause: None,
            stall: false,
        }
    }
}
#[async_trait]
impl ActionEvidenceVerifier for ControlledVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.ledger.registration()
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.active.fetch_add(1, Ordering::SeqCst);
        let _owned = ActiveVerification(self.active.clone());
        let mut attestation = self
            .ledger
            .verify(snapshot, reference, cancellation)
            .await?;
        if let Some(lifetime) = self.lifetime {
            attestation.valid_until =
                attestation.observed_at + chrono::Duration::from_std(lifetime).unwrap();
        }
        if let Some((started, release)) = &self.pause {
            started.notify_one();
            if self.stall {
                std::future::pending::<()>().await;
            }
            release.notified().await;
        }
        Ok(attestation)
    }
}

async fn expire_proof(f: &AdmissionFixture) {
    let seconds: f64 = sqlx::query_scalar("SELECT GREATEST(EXTRACT(EPOCH FROM (max(valid_until)-clock_timestamp())),0)::float8 FROM workflow_action_evidence")
        .fetch_one(f.persistence().pool()).await.unwrap();
    tokio::time::sleep(Duration::from_secs_f64(seconds) + Duration::from_millis(30)).await;
    let expired: bool = sqlx::query_scalar(
        "SELECT bool_and(valid_until<=clock_timestamp()) FROM workflow_action_evidence",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert!(expired);
}

async fn closed(f: &AdmissionFixture, request: &ActionDispatchRequest) -> Arc<LedgerVerifier> {
    let ledger = ledger(f, request).await;
    let setup_provider = LedgerProvider::new(f, Delivery::Pending);
    assert!(invoke(f, request, &setup_provider).await.is_err());
    // Setup premise: the provider was called and its outcome is uncertain. A reservation
    // Unknown or a definite NotApplied leaves no call, so the test must not continue.
    assert_eq!(setup_provider.calls.load(Ordering::SeqCst), 1);
    park(f, request).await;
    barrier(f, request).await;
    ledger
}

async fn expiry_diagnostic(f: &AdmissionFixture, request: &ActionDispatchRequest) -> Value {
    sqlx::query_scalar(include_str!(
        "action_reconciliation_proof_clock_diagnostic.sql"
    ))
    .bind(request.fence.scope.company.as_uuid())
    .bind(request.fence.scope.run.as_uuid())
    .bind(request.fence.scope.execution.as_uuid())
    .bind(request.fence.scope.job.0)
    .bind(request.fence.worker.0)
    .bind(request.fence.generation.0)
    .bind(request.fence.attempt.0)
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap()
}

async fn diagnosed_expired_dispatch(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &LedgerProvider,
) -> AppResult<RemoteDispatchObservation> {
    let before = expiry_diagnostic(f, request).await;
    let started = Instant::now();
    let result = invoke(f, request, provider).await;
    let elapsed = started.elapsed();
    let after = expiry_diagnostic(f, request).await;
    let diagnostic = result
        .as_ref()
        .map(|_| "Ok")
        .map_err(|error| format!("{error:?}"));
    eprintln!(
        "proof_expiry_dispatch elapsed={elapsed:?} result={diagnostic:?} calls={} before={before} after={after}",
        provider.calls.load(Ordering::SeqCst)
    );
    result
}

#[tokio::test]
async fn workflow_action_reconciliation_proof_expiry_reservation_enter_and_successful_replay() {
    for before_enter in [false, true] {
        let (f, mut request) = setup().await;
        let ledger = closed(&f, &request).await;
        let c = proof_command(&f, &request, &ledger, "short-final").await;
        let mut short = ControlledVerifier::new(ledger);
        // Two wall-clock windows meet here: the proof must outlive the steps up to the
        // reservation that consumes it (reconciliation commit, claim, reservation), and it
        // must lapse inside the lease that claim takes, so the replay after it still runs
        // under that lease. Split the lease: each side gets d/2, at least the persistence
        // deadline T of one bounded operation (equal at d = 3 s). expire_proof then waits
        // for the expiry on the DB clock.
        let lifetime = policy().duration() / 2;
        assert!(lifetime >= policy().persistence_deadline());
        short.lifetime = Some(lifetime);
        let verifier = Arc::new(short);
        let saved = run_command(&f, &c, verifier.clone()).await.unwrap();
        assert!(matches!(
            saved.outcome,
            ReconciliationOutcome::Scheduled {
                receipt_only: false
            }
        ));
        new_claim(&f, &mut request).await;
        let writer = adapter(&f, 0);
        let reservation = if before_enter {
            let RemoteReservationResult::Reserved(reserved) =
                writer.reserve_remote(&request, None).await.unwrap()
            else {
                panic!("live proof reserves once")
            };
            assert_eq!(entry_consumptions(&f).await, (2, 1));
            Some(reserved)
        } else {
            None
        };
        expire_proof(&f).await;
        let expired = all_tables(&f).await;
        if let Some(reserved) = reservation {
            assert!(writer.enter_remote(*reserved).await.is_err());
        } else {
            assert!(matches!(
                writer.reserve_remote(&request, None).await.unwrap(),
                RemoteReservationResult::PossibleDispatchExists
            ));
            assert_eq!(entry_consumptions(&f).await, (1, 0));
        }
        assert_eq!(
            all_tables(&f).await,
            expired,
            "expiry refusal mutates no fact"
        );
        let duplicate = run_command(&f, &c, verifier.clone()).await.unwrap();
        assert!(duplicate.replayed);
        assert_eq!(duplicate.outcome, saved.outcome);
        assert_eq!(duplicate.revision, saved.revision);
        assert_eq!(duplicate.evidence, saved.evidence);
        assert_eq!(
            verifier.calls.load(Ordering::SeqCst),
            1,
            "expired exact replay never verifies again"
        );
        assert_eq!(
            all_tables(&f).await,
            expired,
            "expired successful replay is immutable"
        );
        let provider = LedgerProvider::new(&f, Delivery::Pending);
        eprintln!("proof_expiry_dispatch before_enter={before_enter}");
        assert!(matches!(
            // Box the diagnostic seam to preserve the stock two-MiB test stack.
            Box::pin(diagnosed_expired_dispatch(&f, &request, &provider))
                .await
                .unwrap(),
            RemoteDispatchObservation::PossibleDispatchExists
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(effects(&f).await, 0);
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_proof_expiry_while_settlement_waits_for_run_lock() {
    let (f, request) = setup().await;
    let ledger = closed(&f, &request).await;
    let c = proof_command(&f, &request, &ledger, "expired-at-settlement").await;
    let started = Arc::new(Notify::new());
    let locked = Arc::new(Notify::new());
    let mut short = ControlledVerifier::new(ledger);
    // The proof must survive the lock holder's own bounded transaction start (begin plus
    // one FOR UPDATE) while the verifier pauses, and lapse while settlement waits.
    let lifetime = policy().persistence_deadline();
    short.lifetime = Some(lifetime);
    short.pause = Some((started.clone(), locked.clone()));
    let verifier = Arc::new(short);
    let before = all_tables(&f).await;
    let lock_holder = async {
        started.notified().await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
            .bind(c.scope.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        locked.notify_one();
        // Structural, not a fixed sleep: hold the run until settlement is observed queued
        // behind it, then for one whole proof lifetime, which began before this wait.
        let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let guard = Instant::now();
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut *tx)
                .await
                .unwrap();
            let queued: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event_type='Lock' AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(holder).fetch_one(&mut *tx).await.unwrap();
            if queued {
                break;
            }
            assert!(
                guard.elapsed() < Duration::from_secs(10),
                "settlement never queued on the run"
            );
            tokio::time::sleep(lifetime / 100).await;
        }
        tokio::time::sleep(lifetime).await;
        tx.commit().await.unwrap();
    };
    let (result, ()) = tokio::join!(run_command(&f, &c, verifier.clone()), lock_holder);
    let result = result.unwrap();
    assert!(matches!(
        result.outcome,
        ReconciliationOutcome::Blocked { .. }
    ));
    assert_eq!(result.evidence, None);
    assert_eq!(result.revision, c.expected_revision);
    let mut after = all_tables(&f).await;
    assert_eq!(
        after["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    after["workflow_action_evidence_commands"] =
        before["workflow_action_evidence_commands"].clone();
    assert_eq!(
        after, before,
        "expired settlement writes only immutable refusal receipt"
    );
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert!(!retry_safe(&f, &request).await);
    assert_eq!(verifier.active.load(Ordering::SeqCst), 0);
}

async fn execute_budget(
    f: &AdmissionFixture,
    c: &ReconcileActionCommand,
    verifier: Arc<dyn ActionEvidenceVerifier>,
    cancellation: &CancellationToken,
    budget: Duration,
) -> AppResult<ReconciliationResult> {
    let p = f.persistence().clone();
    ActionReconciliationService::new(
        PostgresActionReconciliation::new(p.clone(), Resources),
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        Directory(p),
        Some(verifier),
    )
    .reconcile(c, cancellation, budget)
    .await
}

/// The reconciliation service's own verification ceiling (service.rs `reconcile`), which
/// applies whatever budget the caller passes.
const VERIFICATION_CEILING: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
enum StopVerification {
    Budget,
    Ceiling,
    Cancel,
    Drop,
}

async fn assert_stopped(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    ledger: Arc<LedgerVerifier>,
    stop: StopVerification,
) {
    let c = proof_command(f, request, &ledger, "owned-verification").await;
    let started = Arc::new(Notify::new());
    let mut controlled = ControlledVerifier::new(ledger);
    controlled.pause = Some((started.clone(), Arc::new(Notify::new())));
    controlled.stall = true;
    let verifier = Arc::new(controlled);
    let cancellation = CancellationToken::new();
    let before = all_tables(f).await;
    // The caller's budget includes the bounded authority and snapshot reads before the
    // verifier starts; each may take up to the persistence deadline T.
    let budget = if matches!(stop, StopVerification::Budget) {
        policy().persistence_deadline() * 2
    } else {
        Duration::from_secs(10)
    };
    // Own the actual service/verifier future; abandoning it drops its live guard.
    let mut operation = Box::pin(execute_budget(
        f,
        &c,
        verifier.clone(),
        &cancellation,
        budget,
    ));
    let clock = Instant::now();
    // The caller's deadline includes authority/snapshot reads. Observe an early
    // service result too: joining an ended operation to this signal can hang.
    tokio::select! {
        biased;
        () = started.notified() => {},
        result = &mut operation => panic!("{stop:?}: service ended before verifier paused: {result:?}"),
        () = tokio::time::sleep(Duration::from_secs(6)) => panic!("{stop:?}: verifier never paused"),
    }
    assert_eq!(verifier.active.load(Ordering::SeqCst), 1);
    if matches!(stop, StopVerification::Drop) {
        drop(operation);
    } else {
        if matches!(stop, StopVerification::Cancel) {
            cancellation.cancel();
        }
        let result = tokio::time::timeout(Duration::from_secs(6), operation)
            .await
            .expect("owned verifier did not stop within the observation bound");
        if matches!(stop, StopVerification::Cancel) {
            assert!(matches!(result, Err(AppError::Conflict(_))));
        } else {
            assert!(matches!(result, Err(AppError::Timeout(_))));
        }
        // The independent five-second ceiling is observed at no less than this.
        let ceiling_floor = Duration::from_secs(4);
        if matches!(stop, StopVerification::Ceiling) {
            assert!(clock.elapsed() >= ceiling_floor);
        }
        let elapsed_bound = if matches!(stop, StopVerification::Budget) {
            // The caller's budget stopped it, not the ceiling: at or after the budget and
            // strictly before the ceiling itself. Both timers start inside the operation,
            // after `clock`, so a ceiling stop is never observed before VERIFICATION_CEILING.
            assert!(budget < VERIFICATION_CEILING);
            assert!(clock.elapsed() >= budget);
            VERIFICATION_CEILING
        } else {
            Duration::from_secs(6)
        };
        assert!(
            clock.elapsed() < elapsed_bound,
            "verification exceeded its caller-budget or ceiling bound"
        );
    }
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        verifier.active.load(Ordering::SeqCst),
        0,
        "no detached verifier after caller ends"
    );
    assert_eq!(
        all_tables(f).await,
        before,
        "timeout/cancellation/drop creates no grant or partial fact"
    );
    assert_eq!(entry_consumptions(f).await, (1, 0));
}

#[tokio::test]
async fn workflow_action_reconciliation_proof_verifier_budget_ceiling_cancel_and_drop_are_owned() {
    for stop in [
        StopVerification::Budget,
        StopVerification::Ceiling,
        StopVerification::Cancel,
        StopVerification::Drop,
    ] {
        let (f, request) = setup().await;
        let ledger = closed(&f, &request).await;
        Box::pin(assert_stopped(&f, &request, ledger, stop)).await;
    }
}
