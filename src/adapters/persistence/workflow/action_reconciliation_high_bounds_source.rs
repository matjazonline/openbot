use super::*;

const ALLOWANCE: i32 = 132;

/// Every dispatch here expects the pending provider's own Timeout. A fail-closed
/// reserve_remote Unknown instead is never retried with a new key (its entry may have
/// committed and the scenario counts entries): its backstop is asserted and the whole
/// attempt is void (`None`); `high_source` rebuilds it on fresh state. So is an
/// Interrupted whose fence window had already lapsed by the DB clock when it returned
/// (host load outran the lease): that is the correct fail-closed answer, not the premise.
#[allow(clippy::too_many_arguments)]
async fn provider_timeout(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    contract: Option<&ProviderReplayContract>,
    observed: AppResult<RemoteDispatchObservation>,
    rows: (i64, i64),
    calls: (usize, usize),
    attempt: usize,
    site: &str,
) -> Option<()> {
    match observed {
        Err(error) if reserve_unknown(&error) => {
            let committed =
                assert_reserve_unknown_backstop(f, request, contract, rows, calls).await;
            void_attempt(
                "high_source",
                attempt,
                &format!("site={site} reserve_remote=unknown entry_committed={committed}"),
            );
            None
        }
        Ok(RemoteDispatchObservation::Interrupted) if window_lapsed(f, request).await => {
            assert_lapsed_backstop(f, request, calls).await;
            void_attempt(
                "high_source",
                attempt,
                &format!("site={site} interrupted=lapsed_window"),
            );
            None
        }
        observed => {
            assert!(
                matches!(&observed, Err(AppError::Timeout(_))),
                "{site}: expected the provider Timeout, observed {}",
                observation(&observed)
            );
            Some(())
        }
    }
}

/// The observation names its variant: RemoteDispatchObservation has no Debug.
fn observation(observed: &AppResult<RemoteDispatchObservation>) -> String {
    match observed {
        Err(error) => format!("Err({error:?})"),
        Ok(RemoteDispatchObservation::ApprovalParked(_)) => "ApprovalParked".into(),
        Ok(RemoteDispatchObservation::ApprovalRequired) => "Ok(ApprovalRequired)".into(),
        Ok(RemoteDispatchObservation::PossibleDispatchExists) => {
            "Ok(PossibleDispatchExists)".into()
        }
        Ok(RemoteDispatchObservation::Committed(receipt)) => {
            format!("Ok(Committed(result={}))", receipt.result)
        }
        Ok(RemoteDispatchObservation::Interrupted) => "Ok(Interrupted)".into(),
    }
}

/// The fence still names this exact attempt, but its window (lease expiry or run
/// deadline) has passed by the DB clock: only expiry, not a new owner, ended it.
async fn window_lapsed(f: &AdmissionFixture, request: &ActionDispatchRequest) -> bool {
    let fence = request.fence;
    sqlx::query_scalar("SELECT job.worker_id=$2 AND job.execution_generation=$3 AND job.retry_count+1=$4 AND LEAST(job.lock_expires_at, run.deadline) <= clock_timestamp() FROM background_tasks AS job JOIN workflow_runs AS run ON run.company_id=job.company_id AND run.id=$5 WHERE job.id=$1")
        .bind(fence.scope.job.0)
        .bind(fence.worker.0)
        .bind(fence.generation.0)
        .bind(fence.attempt.0)
        .bind(fence.scope.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

/// A lapsed-window Interrupted adopted nothing: no receipt for the invocation, and at
/// most the one provider call this dispatch made (dropped when the window closed).
async fn assert_lapsed_backstop(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    calls: (usize, usize),
) {
    assert!(
        calls.1 <= calls.0 + 1,
        "at most this dispatch's provider call: {calls:?}"
    );
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_receipts WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(receipts, 0, "a lapsed-window Interrupted wrote no receipt");
}

async fn supported_fixture() -> (AdmissionFixture, ActionDispatchRequest) {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object","properties":{"value":{"type":"integer","minimum":1,"maximum":1000}},"required":["value"]});
    source["steps"]["start"]["with"]["max_tokens"] = json!({"ref":"/input/value"});
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":10,"repetitions":132});
    let (f, scope) = Box::pin(fixture_source(source)).await;
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(attempts, 0, "allowance is fixed before the first claim");
    assert_eq!(
        sqlx::query("UPDATE background_tasks SET max_retries=$2 WHERE id=$1")
            .bind(scope.job.0)
            .bind(ALLOWANCE)
            .execute(f.persistence().pool())
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    Box::pin(setup_action_on_fixture(
        f,
        scope,
        ActionRecovery::SafeRepeat,
        json!({"value":1}),
    ))
    .await
}

pub(super) async fn retire_retry(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    number: i32,
) {
    assert_subject(f, request, number, "processing").await;
    // This deliberately chosen caller cause is independent of the observed Timeout.
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("provider.rejected").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    });
    assert!(
        f.persistence()
            .settled()
            .release_io(request.fence, policy(), cause)
            .await
            .unwrap()
    );
    assert_subject(f, request, number, "failed").await;
    let before = all_tables(f).await;
    let command = RetryCommand {
        company_id: request.scope().company,
        run_id: request.scope().run,
        actor: f.binding.target.actor,
        command_key: IdempotencyKey::parse(format!("high-retry-{number}")).unwrap(),
        expected_revision: f
            .persistence()
            .head(request.scope().company, request.scope().run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    assert!(matches!(
        f.persistence().retry(command).await.unwrap(),
        RetryResult::Applied { .. }
    ));
    let after = all_tables(f).await;
    retry_delta(&before, &after, request, number);
    assert_subject(f, request, number, "pending").await;
}

async fn real_history(
    f: &AdmissionFixture,
    request: &mut ActionDispatchRequest,
    provider: &RegisteredLedger,
    attempt: usize,
) -> Option<()> {
    for number in 1..=128 {
        assert_subject(f, request, number, "processing").await;
        let rows = invocation_rows(f, request).await;
        let calls = provider.inner.calls.load(Ordering::SeqCst);
        // Quiescent point: the dispatch below needs the whole lease window for the pending
        // provider's own Timeout, not what setup and the previous retry left of it.
        renew_fixture_lease(f, request.fence).await;
        let observed = Box::pin(registered_invoke(f, request, provider)).await;
        provider_timeout(
            f,
            request,
            Some(&provider.contract),
            observed,
            rows,
            (calls, provider.inner.calls.load(Ordering::SeqCst)),
            attempt,
            &format!("real_history number={number}"),
        )
        .await?;
        assert_eq!(provider.inner.calls.load(Ordering::SeqCst), number as usize);
        assert_eq!(
            subject_counts(f, request).await,
            EntryAccounting {
                entries: i64::from(number),
                consumptions: 0,
                requests: i64::from(number)
            }
        );
        Box::pin(retire_retry(f, request, number)).await;
        if number < 128 {
            Box::pin(new_claim(f, request)).await;
            assert_eq!(request.fence.attempt.0, number + 1);
        }
    }
    Some(())
}

async fn final_proof(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
) -> Uuid {
    barrier(f, request).await;
    assert!(!delayed_apply(f, request, &good()).await);
    let command = proof_command(f, request, &verifier, "high-final-128").await;
    let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
    let ReconciliationPreparation::Snapshot(snapshot) = reader.snapshot(&command).await.unwrap()
    else {
        panic!("128 genuine entries fit the snapshot");
    };
    assert_eq!(snapshot.entries.len(), 128);
    let result = Box::pin(run_command(f, &command, verifier)).await.unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::NotAppliedRecorded);
    let proof = result.evidence.unwrap().as_uuid();
    let valid: bool = sqlx::query_scalar("SELECT grant_eligible AND (SELECT count(*) FROM workflow_action_evidence_coverage WHERE evidence_id=evidence.id)=128 AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions WHERE evidence_id=evidence.id) AND NOT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry WHERE entry.company_id=evidence.company_id AND entry.invocation_id=evidence.invocation_id AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=evidence.company_id AND coverage.evidence_id=evidence.id AND coverage.remote_entry_id=entry.id)) FROM workflow_action_evidence AS evidence WHERE company_id=$1 AND id=$2")
        .bind(request.scope().company.as_uuid()).bind(proof).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(valid);
    assert_eq!(available(f, request, None).await, Some(proof));
    assert_subject(f, request, 128, "pending").await;
    eprintln!("high_bounds source proof=128 actual_entries=128 calls=128 consumptions=0");
    proof
}

async fn foreign_entry(f: &AdmissionFixture, attempt: usize) -> Option<Uuid> {
    let fixture = Fixture::in_database(
        f.binding.fixture._db.clone(),
        &format!("-high-company-{}", Uuid::new_v4().simple()),
    )
    .await;
    let source = serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    let foreign = Box::pin(AdmissionFixture::with_binding(
        BindingFixture::from_fixture(fixture, source).await,
    ))
    .await;
    let command = foreign
        .prepare(foreign.request("high-foreign", foreign.manual()))
        .await;
    foreign.persistence().admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(foreign.persistence().pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    let claim = foreign
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let (foreign, request) = Box::pin(initialize_action_on_claim(
        foreign,
        claim,
        ActionRecovery::Reconcile,
        json!({"value":2}),
    ))
    .await;
    assert_ne!(request.scope().company, f.binding.target.company);
    // The provider fixture owns its operation row through the same registration
    // used by the accepted same-database company constructor.
    let _verifier = register_ledger(&foreign, &request).await;
    let provider = LedgerProvider::new(&foreign, Delivery::Pending);
    let rows = invocation_rows(&foreign, &request).await;
    let observed = Box::pin(invoke(&foreign, &request, &provider)).await;
    provider_timeout(
        &foreign,
        &request,
        None,
        observed,
        rows,
        (0, provider.calls.load(Ordering::SeqCst)),
        attempt,
        "foreign_entry",
    )
    .await?;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let entry = sqlx::query_scalar(
        "SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    park(&foreign, &request).await;
    Some(entry)
}

/// The committed 128-entry source. Each attempt must prove every dispatch's provider
/// Timeout; a fail-closed reserve_remote Unknown or a lapsed-window Interrupted voids it
/// (backstop asserted, logged) and the scenario is rebuilt on fresh state, at most three
/// times.
pub(super) async fn high_source() -> HighSource {
    for attempt in 1..=3 {
        if let Some(source) = Box::pin(try_high_source(attempt)).await {
            return source;
        }
    }
    panic!(
        "high_source: every attempt was void (fail-closed reserve_remote Unknown or lapsed-window Interrupted)"
    );
}

async fn try_high_source(attempt: usize) -> Option<HighSource> {
    let (f, mut request) = Box::pin(supported_fixture()).await;
    let verifier = ledger(&f, &request).await;
    let provider = RegisteredLedger::new(&f, &request, Delivery::Pending).await;
    if Box::pin(real_history(&f, &mut request, &provider, attempt))
        .await
        .is_none()
    {
        f.persistence().pool().close().await;
        return None;
    }
    let proof = Box::pin(final_proof(&f, &request, verifier)).await;
    // Bootstrap the other tenant before starting the short subject fence.
    let Some(other_company) = Box::pin(foreign_entry(&f, attempt)).await else {
        f.persistence().pool().close().await;
        return None;
    };
    Box::pin(new_claim(&f, &mut request)).await;
    assert_eq!(request.fence.attempt.0, 129);
    assert_eq!(available(&f, &request, None).await, Some(proof));
    let historical = Box::pin(native::historical_entry(&f, &request)).await;
    assert_eq!(
        subject_counts(&f, &request).await,
        EntryAccounting {
            entries: 129,
            consumptions: 0,
            requests: 128
        }
    );
    assert_eq!(available(&f, &request, None).await, None);
    let sibling = Box::pin(sibling(&f, &request, 2)).await;
    let sibling_provider = RegisteredLedger::new(&f, &sibling, Delivery::Pending).await;
    let rows = invocation_rows(&f, &sibling).await;
    let observed = Box::pin(registered_invoke(&f, &sibling, &sibling_provider)).await;
    if provider_timeout(
        &f,
        &sibling,
        Some(&sibling_provider.contract),
        observed,
        rows,
        (0, sibling_provider.inner.calls.load(Ordering::SeqCst)),
        attempt,
        "sibling",
    )
    .await
    .is_none()
    {
        f.persistence().pool().close().await;
        return None;
    }
    let other_invocation = sqlx::query_scalar(
        "SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(sibling.scope().company.as_uuid())
    .bind(sibling.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_ne!(sibling.subject.invocation, request.subject.invocation);
    Box::pin(retire_retry(&f, &request, 129)).await;
    Box::pin(new_claim(&f, &mut request)).await;
    let claimed = std::time::Instant::now();
    assert_subject(&f, &request, 130, "processing").await;
    eprintln!(
        "high_bounds committed_source attempts=129 retired_entries=129 current_attempt=130 max_retries=132"
    );
    Some(HighSource {
        fixture: f,
        request,
        provider,
        proof,
        historical,
        other_invocation,
        other_company,
        claimed,
    })
}
