//! Narrow fixture-stall backstops; never turn a failed boundary assertion into a retry.
use super::*;

pub(super) async fn lapsed(f: &AdmissionFixture, request: &ActionDispatchRequest) -> bool {
    let fence = request.fence;
    sqlx::query_scalar("SELECT job.worker_id=$2 AND job.execution_generation=$3 AND job.retry_count+1=$4 AND LEAST(job.lock_expires_at, run.deadline) <= clock_timestamp() FROM background_tasks AS job JOIN workflow_runs AS run ON run.company_id=job.company_id AND run.id=$5 WHERE job.id=$1")
        .bind(fence.scope.job.0).bind(fence.worker.0).bind(fence.generation.0)
        .bind(fence.attempt.0).bind(fence.scope.run.as_uuid())
        .fetch_one(f.persistence().pool()).await.unwrap()
}

async fn no_result(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_receipts WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(receipts, 0, "an abandoned invocation cannot have a receipt");
    assert_eq!(effects(f).await, 0);
}

pub(super) async fn pending_loss(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &LedgerProvider,
) -> Option<()> {
    renew_fixture_lease(f, request.fence).await;
    // Box the actual dispatch seam rather than accumulating fixture poll frames.
    Box::pin(observe_pending(f, request, provider)).await
}

pub(super) async fn observe_pending(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &LedgerProvider,
) -> Option<()> {
    let rows = invocation_rows(f, request).await;
    let calls = provider.calls.load(Ordering::SeqCst);
    let observed = Box::pin(invoke(f, request, provider)).await;
    let after_calls = provider.calls.load(Ordering::SeqCst);
    // The local classifier is also exercised against controlled durable outcomes.
    Box::pin(classify_pending(
        f,
        request,
        rows,
        (calls, after_calls),
        observed,
    ))
    .await
}

pub(super) async fn classify_pending(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    rows: (i64, i64),
    calls: (usize, usize),
    observed: AppResult<RemoteDispatchObservation>,
) -> Option<()> {
    let (calls, after_calls) = calls;
    match observed {
        Err(error) if reserve_unknown(&error) => {
            let committed =
                assert_reserve_unknown_backstop(f, request, None, rows, (calls, after_calls)).await;
            no_result(f, request).await;
            uncaptured(&format!(
                "BACKSTOP prior_bounds reserve_remote=unknown entry_committed={committed}; no provider poll"
            ));
            None
        }
        Ok(RemoteDispatchObservation::Interrupted) if lapsed(f, request).await => {
            assert!(after_calls <= calls + 1, "only this invocation may poll");
            no_result(f, request).await;
            uncaptured("BACKSTOP prior_bounds interrupted=lapsed_exact_fence; no receipt/effect");
            None
        }
        observed => {
            assert!(
                matches!(observed, Err(AppError::Timeout(ref message)) if message == "provider request remains active"),
                "expected Pending provider's own loss, observed {}",
                observed
                    .as_ref()
                    .err()
                    .map_or("non-error dispatch outcome".into(), |error| format!(
                        "{error:?}"
                    ))
            );
            assert_eq!(
                after_calls,
                calls + 1,
                "the lost response follows one provider call"
            );
            Some(())
        }
    }
}

pub(super) async fn park_live(f: &AdmissionFixture, request: &ActionDispatchRequest) -> Option<()> {
    renew_fixture_lease(f, request.fence).await;
    Box::pin(park_observed(f, request)).await
}

pub(super) async fn park_observed(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
) -> Option<()> {
    let before = all_tables(f).await;
    let released = f
        .persistence()
        .settled()
        .release_io(
            request.fence,
            policy(),
            LeaseReleaseCause::LocalInterruption,
        )
        .await
        .unwrap();
    if released {
        return Some(());
    }
    assert!(
        lapsed(f, request).await,
        "release refusal without exact-owner expiry"
    );
    assert_eq!(all_tables(f).await, before, "refused release wrote nothing");
    no_result(f, request).await;
    uncaptured("BACKSTOP prior_bounds release=false lapsed_exact_fence; no durable delta");
    None
}

pub(super) struct PausedVerifier {
    ledger: Arc<LedgerVerifier>,
    pub(super) calls: AtomicUsize,
    pub(super) started: Notify,
    pub(super) release: Notify,
    pub(super) finished: CancellationToken,
    pub(super) returned: std::sync::atomic::AtomicBool,
    pub(super) released_at: std::sync::Mutex<Option<std::time::Instant>>,
}
impl PausedVerifier {
    pub(super) async fn run(
        self: &Arc<Self>,
        f: &AdmissionFixture,
        command: &ReconcileActionCommand,
    ) -> AppResult<ReconciliationResult> {
        let result = run_command(f, command, self.clone()).await;
        self.finished.cancel();
        result
    }

    pub(super) async fn wait_started(&self) -> bool {
        tokio::select! {
            biased;
            _ = self.started.notified() => true,
            _ = self.finished.cancelled() => false,
        }
    }

    pub(super) fn new(ledger: Arc<LedgerVerifier>) -> Self {
        Self {
            ledger,
            calls: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
            finished: CancellationToken::new(),
            returned: std::sync::atomic::AtomicBool::new(false),
            released_at: std::sync::Mutex::new(None),
        }
    }
}
#[async_trait]
impl ActionEvidenceVerifier for PausedVerifier {
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
        let evidence = self
            .ledger
            .verify(snapshot, reference, cancellation)
            .await?;
        self.started.notify_one();
        tokio::select! {
            _ = cancellation.cancelled() => return Err(AppError::Conflict("fixture cancelled".into())),
            _ = self.release.notified() => {},
        }
        self.returned.store(true, Ordering::SeqCst);
        Ok(evidence)
    }
}

pub(super) fn withheld_timeout(
    result: &AppResult<ReconciliationResult>,
    elapsed: Duration,
    released_after: Option<Duration>,
) {
    assert!(
        matches!(result, Err(AppError::Timeout(message)) if message == "Workflow reconciliation verification"),
        "withheld verifier must end at its real verification budget: {result:?}"
    );
    assert!(
        elapsed >= Duration::from_secs(5),
        "verification budget was not exhausted"
    );
    assert!(
        released_after.is_none_or(|elapsed| elapsed >= Duration::from_secs(5)),
        "prompt verifier release cannot be blamed on the competitor"
    );
}

#[test]
fn prior_bounds_timeout_discriminator_rejects_prompt_release_and_wrong_errors() {
    let timeout = || {
        Err(AppError::Timeout(
            "Workflow reconciliation verification".into(),
        ))
    };
    withheld_timeout(&timeout(), Duration::from_secs(5), None);
    withheld_timeout(
        &timeout(),
        Duration::from_secs(6),
        Some(Duration::from_secs(5)),
    );
    for (error, elapsed, release) in [
        (
            AppError::Timeout("provider request remains active".into()),
            Duration::from_secs(5),
            None,
        ),
        (
            AppError::Timeout("Workflow reconciliation verification".into()),
            Duration::from_secs(4),
            None,
        ),
        (
            AppError::Timeout("Workflow reconciliation verification".into()),
            Duration::from_secs(6),
            Some(Duration::from_secs(4)),
        ),
        (
            AppError::Conflict("real failure".into()),
            Duration::from_secs(5),
            None,
        ),
    ] {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                withheld_timeout(&Err(error), elapsed, release);
            }))
            .is_err(),
            "a noncausal timeout must remain a hard failure"
        );
    }
}

/// Only a complete fresh fixture proves the boundary; exhaustion is an error.
pub(super) async fn fresh_proof<F, Fut>(mut attempt: F) -> Result<(), &'static str>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<()>>,
{
    for number in 1..=3 {
        if Box::pin(attempt()).await.is_some() {
            return Ok(());
        }
        void_attempt(
            "prior_bounds",
            number,
            "qualified fixture stall; backstop passed",
        );
    }
    Err("prior_bounds: all three fresh fixtures were void; boundary unproven")
}
