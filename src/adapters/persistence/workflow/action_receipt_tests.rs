use super::*;
use crate::application::workflow::publication::ActionRecovery;
use std::sync::atomic::AtomicBool;

struct DeduplicatingProvider<'a> {
    f: &'a AdmissionFixture,
    contract: Option<ProviderReplayContract>,
    lose: AtomicBool,
    calls: AtomicUsize,
    result: Value,
}
#[async_trait]
impl RemoteAction for DeduplicatingProvider<'_> {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        self.contract.as_ref()
    }
    async fn invoke(
        &self,
        action: &FrozenAction,
        invocation: &ProviderInvocation,
    ) -> AppResult<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let marker: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workflow_action_remote_entries WHERE company_id=$1)",
        )
        .bind(action.scope().company.as_uuid())
        .fetch_one(self.f.persistence().pool())
        .await?;
        assert!(
            marker,
            "entry and FIRST marker must commit before actual provider I/O"
        );
        let key =
            if action.request().contract.policy.recovery == ActionRecovery::ProviderIdempotency {
                invocation
                    .idempotency_key()
                    .expect("actual transmitted provider key")
                    .clone()
            } else {
                action.idempotency_key()
            };
        sqlx::query("INSERT INTO fixture_remote_effects VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(key.as_str())
            .execute(self.f.persistence().pool())
            .await?;
        if self.lose.swap(false, Ordering::SeqCst) {
            return Err(AppError::Timeout("lost after applied remote effect".into()));
        }
        Ok(self.result.clone())
    }
}
async fn receipt_fixture(
    recovery: ActionRecovery,
    _lost: bool,
) -> (
    AdmissionFixture,
    ActionDispatchRequest,
    Option<ProviderReplayContract>,
) {
    let (f, request) = setup_action(recovery, json!({"value":1})).await;
    sqlx::raw_sql("CREATE TABLE fixture_remote_effects (provider_key text PRIMARY KEY)")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let action = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    let mode = match recovery {
        ActionRecovery::SafeRepeat => Some(ProviderReplayMode::SafeRepeat),
        ActionRecovery::ProviderIdempotency => Some(ProviderReplayMode::ProviderIdempotency {
            retention: Duration::from_secs(31536000),
        }),
        ActionRecovery::Reconcile => None,
    };
    let contract = mode.map(|mode| {
        ProviderReplayContract::approve(
            TypeName::parse("fixture.dedup").unwrap(),
            &action.request().contract,
            &action.request().target,
            mode,
        )
        .unwrap()
    });
    (f, request, contract)
}
fn dedup<'a>(
    f: &'a AdmissionFixture,
    contract: Option<ProviderReplayContract>,
    lost: bool,
) -> DeduplicatingProvider<'a> {
    DeduplicatingProvider {
        f,
        contract,
        lose: AtomicBool::new(lost),
        calls: AtomicUsize::new(0),
        result: json!({"written":true}),
    }
}
/// reserve_remote fails closed only on an ambiguous COMMIT. A definite NotApplied
/// (`DatabaseTimeout`) reported before the provider was called wrote nothing, so the caller
/// retries it as the worker does; every other outcome is returned as is.
async fn dispatch_settled(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &DeduplicatingProvider<'_>,
) -> AppResult<RemoteDispatchObservation> {
    let started = std::time::Instant::now();
    loop {
        let calls = provider.calls.load(Ordering::SeqCst);
        match ActionService::new(adapter(f, 0))
            .dispatch_remote(request, provider, &CancellationToken::new())
            .await
        {
            Err(AppError::DatabaseTimeout(error))
                if provider.calls.load(Ordering::SeqCst) == calls =>
            {
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "dispatch never settled before the provider: {error}"
                );
            }
            result => return result,
        }
    }
}
async fn effect_count(f: &AdmissionFixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM fixture_remote_effects")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}
async fn reclaim(f: &AdmissionFixture, request: &mut ActionDispatchRequest) {
    let safe: Option<bool> = sqlx::query_scalar("SELECT workflow_action_retry_safe($1,$2)")
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(safe, Some(true));
    expire(f, request.fence.scope).await;
    assert!(
        f.persistence()
            .settled()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    due(f, request.fence.scope).await;
    let [a, b] = f
        .persistence()
        .settled()
        .competing(
            |p, _| p.claim_io(request.fence.scope, worker(), policy()),
            Option::is_some,
        )
        .await;
    let claims = [a.unwrap(), b.unwrap()];
    assert_eq!(claims.iter().filter(|x| x.is_some()).count(), 1);
    request.fence = claims.into_iter().flatten().next().unwrap().fence;
    assert_eq!(request.fence.attempt, WorkflowAttempt(2));
}

#[tokio::test]
async fn workflow_action_receipt_lost_actual_effect_supported_reclaim_and_competing_replay() {
    for recovery in [
        ActionRecovery::SafeRepeat,
        ActionRecovery::ProviderIdempotency,
    ] {
        let (f, mut request, contract) = receipt_fixture(recovery, true).await;
        let provider = dedup(&f, contract, true);
        let service = ActionService::new(adapter(&f, 0));
        assert!(
            service
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(effect_count(&f).await, 1);
        assert!(matches!(
            service
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .unwrap(),
            RemoteDispatchObservation::PossibleDispatchExists
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        reclaim(&f, &mut request).await;
        let cancel = CancellationToken::new();
        let (a, b) = tokio::join!(
            service.dispatch_remote(&request, &provider, &cancel),
            service.dispatch_remote(&request, &provider, &cancel)
        );
        let outcomes = [a.unwrap(), b.unwrap()];
        assert!(
            outcomes
                .iter()
                .any(|x| matches!(x, RemoteDispatchObservation::Committed(_)))
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            effect_count(&f).await,
            1,
            "actual provider key deduplicates applied lost response"
        );
        assert_eq!(facts(&f).await, (1, 1, 0));
        let entries: i64 =
            sqlx::query_scalar("SELECT count(*) FROM workflow_action_remote_entries")
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        assert_eq!(entries, 2);
        assert!(matches!(
            service
                .dispatch_remote(&request, &provider, &cancel)
                .await
                .unwrap(),
            RemoteDispatchObservation::Committed(_)
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn workflow_action_receipt_restart_model_link_no_registration_and_current_access() {
    let (f, request, contract) = receipt_fixture(ActionRecovery::ProviderIdempotency, false).await;
    let provider = dedup(&f, contract, false);
    let receipt = dispatch_settled(&f, &request, &provider).await.unwrap();
    let RemoteDispatchObservation::Committed(receipt) = receipt else {
        panic!("receipt before output")
    };
    assert_eq!(receipt.result, json!({"written":true}));
    let action = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    let call = ModelToolCallId::parse("durable-receipt-model-call").unwrap();
    let intent = ActionService::new(f.persistence().clone())
        .prepare_tool(action.request().clone(), call.clone())
        .await
        .unwrap();
    let restart = ActionService::new(f.persistence().clone())
        .prepare_tool(action.request().clone(), call)
        .await
        .unwrap();
    assert!(restart.replayed);
    assert_eq!(intent.approval_subject(), request.subject);
    let no_registration = dedup(&f, None, false);
    let RemoteDispatchObservation::Committed(replayed) =
        dispatch_settled(&f, &request, &no_registration)
            .await
            .unwrap()
    else {
        panic!("durable receipt replay")
    };
    assert_eq!(replayed.result, receipt.result);
    assert_eq!(no_registration.calls.load(Ordering::SeqCst), 0);
    sqlx::query("UPDATE fixture_action_resources SET enabled=false")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &no_registration, &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(facts(&f).await, (1, 1, 0));
}

#[tokio::test]
async fn workflow_action_receipt_late_cancel_and_stale_truth_without_advancement() {
    for cancel in [false, true] {
        let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, false).await;
        let provider = dedup(&f, contract, false);
        let writer = adapter(&f, 0);
        let RemoteReservationResult::Reserved(reservation) =
            writer.reserve_remote(&request, None).await.unwrap()
        else {
            panic!("reserved")
        };
        let entry = writer.enter_remote(*reservation).await.unwrap();
        let output = provider
            .invoke(&entry.action, &entry.provider)
            .await
            .unwrap();
        if cancel {
            sqlx::query("UPDATE workflow_runs SET state='cancelled' WHERE id=$1")
                .bind(request.scope().run.as_uuid())
                .execute(f.persistence().pool())
                .await
                .unwrap();
        } else {
            expire(&f, request.fence.scope).await;
        }
        assert!(matches!(
            writer.finish_remote(entry, output).await.unwrap(),
            RemoteDispatchObservation::Interrupted
        ));
        assert_eq!(facts(&f).await, (1, 1, 0));
        let unadvanced:bool=sqlx::query_scalar("SELECT completed_at IS NULL AND committed_output IS NULL AND successor_execution_id IS NULL FROM workflow_executions WHERE id=$1")
            .bind(request.scope().execution.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
        assert!(unadvanced);
    }
}

#[tokio::test]
async fn workflow_action_receipt_deferred_rollback_returns_no_success() {
    let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    sqlx::raw_sql("CREATE FUNCTION receipt_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'receipt commit rollback'; END $$; CREATE CONSTRAINT TRIGGER receipt_fault AFTER INSERT ON workflow_action_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION receipt_fault();")
        .execute(f.persistence().pool()).await.unwrap();
    let provider = dedup(&f, contract, false);
    let service = ActionService::new(adapter(&f, 0));
    assert!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(facts(&f).await, (1, 0, 0));
    assert_eq!(effect_count(&f).await, 1);
    assert!(matches!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

/// Whether the exact run is lockable right now, observed from another OS thread while this
/// runtime's only thread waits for the answer. No task of this runtime can run in between,
/// including sqlx's return-to-pool flush of a dropped transaction's queued ROLLBACK, so only
/// a ROLLBACK the operation awaited before returning can have released the run.
fn run_lockable_now(f: &AdmissionFixture, request: &ActionDispatchRequest) -> bool {
    use sqlx::Connection as _;
    let options = (*f.persistence().pool().connect_options()).clone();
    let company = request.scope().company.as_uuid().to_owned();
    let run = request.scope().run.as_uuid().to_owned();
    std::thread::spawn(move || {
        let observe = async {
            let mut observer = sqlx::PgConnection::connect_with(&options).await.unwrap();
            let mut tx = observer.begin().await.unwrap();
            // SKIP LOCKED never waits on the row; a relation-level wait fails on the server.
            sqlx::query("SELECT set_config('lock_timeout','100ms',true)")
                .execute(&mut *tx)
                .await
                .unwrap();
            let (visible, lockable): (Uuid, Option<Uuid>) = sqlx::query_as(
                "SELECT visible.id, (SELECT locked.id FROM workflow_runs AS locked \
                 WHERE locked.company_id=visible.company_id AND locked.id=visible.id \
                 FOR UPDATE OF locked SKIP LOCKED) \
                 FROM workflow_runs AS visible WHERE visible.company_id=$1 AND visible.id=$2",
            )
            .bind(company)
            .bind(run)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            tx.rollback().await.unwrap();
            observer.close().await.unwrap();
            assert_eq!(visible, run);
            lockable == Some(run)
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(10), observe)
                    .await
                    .expect("exact run observation hung past its 10 s guard")
            })
    })
    .join()
    .unwrap()
}

/// A refused return (the fence still names us but its window lapsed, so lock_fence took
/// the run, execution and job locks) ends in ROLLBACK before finish_remote returns.
#[tokio::test]
async fn workflow_action_receipt_refused_return_releases_run_before_returning() {
    let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let provider = dedup(&f, contract, false);
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reservation) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("reserved")
    };
    let entry = writer.enter_remote(*reservation).await.unwrap();
    let output = provider
        .invoke(&entry.action, &entry.provider)
        .await
        .unwrap();
    expire(&f, request.fence.scope).await;
    assert!(matches!(
        writer.finish_remote(entry, output).await.unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    assert!(
        run_lockable_now(&f, &request),
        "a refused return left the exact run locked after finish_remote returned"
    );
}

#[path = "action_history_tests.rs"]
mod history_tests;

#[path = "action_receipt_acceptance_tests.rs"]
mod acceptance_tests;

#[path = "action_receipt_resolution_tests.rs"]
mod resolution_tests;

#[path = "action_uncertainty_tests.rs"]
mod uncertainty_tests;
