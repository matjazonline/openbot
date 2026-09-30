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
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    due(f, request.fence.scope).await;
    let (a, b) = tokio::join!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy()),
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
    );
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
    let receipt = ActionService::new(adapter(&f, 0))
        .dispatch_remote(&request, &provider, &CancellationToken::new())
        .await
        .unwrap();
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
    let RemoteDispatchObservation::Committed(replayed) = ActionService::new(adapter(&f, 0))
        .dispatch_remote(&request, &no_registration, &CancellationToken::new())
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

#[path = "action_history_tests.rs"]
mod history_tests;

#[path = "action_receipt_acceptance_tests.rs"]
mod acceptance_tests;

#[path = "action_uncertainty_tests.rs"]
mod uncertainty_tests;
