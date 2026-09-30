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
    assert!(
        invoke(f, request, &LedgerProvider::new(f, Delivery::Pending))
            .await
            .is_err()
    );
    park(f, request).await;
    barrier(f, request).await;
    ledger
}

#[tokio::test]
async fn workflow_action_reconciliation_proof_expiry_reservation_enter_and_successful_replay() {
    for before_enter in [false, true] {
        let (f, mut request) = setup().await;
        let ledger = closed(&f, &request).await;
        let c = proof_command(&f, &request, &ledger, "short-final").await;
        let mut short = ControlledVerifier::new(ledger);
        short.lifetime = Some(Duration::from_millis(500));
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
        assert!(matches!(
            invoke(&f, &request, &provider).await.unwrap(),
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
    short.lifetime = Some(Duration::from_millis(300));
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
        tokio::time::sleep(Duration::from_millis(450)).await;
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
    let budget = if matches!(stop, StopVerification::Budget) {
        Duration::from_millis(100)
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
        if matches!(stop, StopVerification::Ceiling) {
            assert!(clock.elapsed() >= Duration::from_secs(4));
        }
        let elapsed_bound = if matches!(stop, StopVerification::Budget) {
            // Ten times the caller's budget allows scheduler delay while still
            // rejecting a regression to the independent five-second ceiling.
            Duration::from_secs(1)
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
