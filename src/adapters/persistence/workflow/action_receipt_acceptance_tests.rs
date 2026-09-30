use super::*;
use crate::application::workflow::completion::WorkflowCompletion;

struct LostReceiptAck(Dispatch);
#[async_trait]
impl ActionDispatch for LostReceiptAck {
    async fn local(&self, request: &ActionDispatchRequest) -> AppResult<LocalDispatchResult> {
        self.0.local(request).await
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
impl RemoteDispatch for LostReceiptAck {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        self.0.reserve_remote(request, contract).await
    }
    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry> {
        self.0.enter_remote(reservation).await
    }
    async fn finish_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation> {
        let observation = self.0.finish_remote(entry, result).await?;
        assert!(matches!(
            observation,
            RemoteDispatchObservation::Committed(_)
        ));
        Err(AppError::Timeout(
            "lost receipt commit acknowledgement".into(),
        ))
    }
    async fn owns_remote(&self, request: &ActionDispatchRequest) -> AppResult<bool> {
        self.0.owns_remote(request).await
    }
}

#[tokio::test]
async fn workflow_action_receipt_committed_ambiguous_ack_restart_zero_io() {
    let (f, request, contract) = receipt_fixture(ActionRecovery::ProviderIdempotency, false).await;
    let provider = dedup(&f, contract, false);
    assert!(matches!(
        ActionService::new(LostReceiptAck(adapter(&f, 0)))
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await,
        Err(AppError::Timeout(_))
    ));
    assert_eq!(facts(&f).await, (1, 1, 0));
    let restart = dedup(&f, None, false);
    let RemoteDispatchObservation::Committed(receipt) = ActionService::new(adapter(&f, 0))
        .dispatch_remote(&request, &restart, &CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("restart needs committed effect truth")
    };
    assert_eq!(receipt.result, json!({"written":true}));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(restart.calls.load(Ordering::SeqCst), 0);
    assert_eq!(effect_count(&f).await, 1);
}

struct ReconcileRecoveryProbe<'a>(DeduplicatingProvider<'a>);

#[async_trait]
impl RemoteAction for ReconcileRecoveryProbe<'_> {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(
        &self,
        action: &FrozenAction,
        invocation: &ProviderInvocation,
    ) -> AppResult<Value> {
        let unresolved: Option<bool> =
            sqlx::query_scalar("SELECT workflow_action_retry_safe($1,$2)")
                .bind(action.scope().company.as_uuid())
                .bind(action.scope().execution.as_uuid())
                .fetch_one(self.0.f.persistence().pool())
                .await?;
        assert_eq!(unresolved, Some(false), "unresolved Reconcile cannot retry");
        self.0.invoke(action, invocation).await
    }
}

#[tokio::test]
async fn workflow_action_receipt_resolved_reconcile_retires_safe_and_reclaims_zero_io() {
    let (f, mut request, contract) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    assert!(contract.is_none());
    let provider = ReconcileRecoveryProbe(dedup(&f, None, false));
    let RemoteDispatchObservation::Committed(receipt) = ActionService::new(adapter(&f, 0))
        .dispatch_remote(&request, &provider, &CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("real committed receipt resolves Reconcile")
    };
    reclaim(&f, &mut request).await;
    let safety: String = sqlx::query_scalar(
        "SELECT workflow_retry_safety FROM task_attempts WHERE task_id=$1 AND attempt_number=1",
    )
    .bind(request.fence.scope.job.0)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(safety, "safe");
    let no_registration = dedup(&f, None, false);
    let RemoteDispatchObservation::Committed(replayed) = ActionService::new(adapter(&f, 0))
        .dispatch_remote(&request, &no_registration, &CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("reclaimed current fence returns saved receipt")
    };
    assert_eq!(replayed.result, receipt.result);
    assert_eq!(provider.0.calls.load(Ordering::SeqCst), 1);
    assert_eq!(no_registration.calls.load(Ordering::SeqCst), 0);
    assert_eq!(effect_count(&f).await, 1);
    assert_eq!(facts(&f).await, (1, 1, 0));
}

#[tokio::test]
async fn workflow_action_receipt_actual_result_precedes_shared_completion_writer() {
    let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, false).await;
    let mut provider = dedup(&f, contract, false);
    provider.result = json!({"items":[],"token_count":0});
    let RemoteDispatchObservation::Committed(receipt) = ActionService::new(adapter(&f, 0))
        .dispatch_remote(&request, &provider, &CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("receipt before completion")
    };
    let unadvanced: bool = sqlx::query_scalar("SELECT completed_at IS NULL AND committed_output IS NULL FROM workflow_executions WHERE id=$1")
        .bind(request.scope().execution.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(unadvanced);
    let committed = f
        .persistence()
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: request.fence,
            output: receipt.result.clone(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(committed.output, receipt.result);
    let ordered: bool = sqlx::query_scalar("SELECT receipt.created_at<=execution.completed_at AND receipt.result=execution.committed_output FROM workflow_action_receipts AS receipt JOIN workflow_executions AS execution ON execution.company_id=receipt.company_id AND execution.id=receipt.execution_id WHERE receipt.invocation_id=$1")
        .bind(request.subject.invocation.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(ordered);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_action_receipt_reclaimed_attempt_rechecks_current_registration_and_access() {
    let (f, mut request, contract) =
        receipt_fixture(ActionRecovery::ProviderIdempotency, true).await;
    let first = dedup(&f, contract.clone(), true);
    assert!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &first, &CancellationToken::new())
            .await
            .is_err()
    );
    reclaim(&f, &mut request).await;
    let missing = dedup(&f, None, false);
    assert!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &missing, &CancellationToken::new())
            .await
            .is_err()
    );
    let action = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    let changed = ProviderReplayContract::approve(
        TypeName::parse("different.registration").unwrap(),
        &action.request().contract,
        &action.request().target,
        ProviderReplayMode::ProviderIdempotency {
            retention: Duration::from_secs(31536000),
        },
    )
    .unwrap();
    let changed = dedup(&f, Some(changed), false);
    assert!(matches!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &changed, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    sqlx::query("UPDATE fixture_action_resources SET enabled=false")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let current = dedup(&f, contract, false);
    assert!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &current, &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(missing.calls.load(Ordering::SeqCst), 0);
    assert_eq!(changed.calls.load(Ordering::SeqCst), 0);
    assert_eq!(current.calls.load(Ordering::SeqCst), 0);
    let entries: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_remote_entries")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(entries, 1, "denied redispatch never creates a new entry");
    sqlx::query("UPDATE fixture_action_resources SET enabled=true")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(matches!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &current, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(effect_count(&f).await, 1);
}

#[derive(Clone, Copy)]
enum AuthorityFailure {
    Database,
    Policy,
    Timeout,
}
struct FailingReturnAuthority {
    calls: AtomicUsize,
    failure: AuthorityFailure,
}
#[async_trait]
impl SqlActionAuthority for FailingReturnAuthority {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        saved: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority> {
        if self.calls.fetch_add(1, Ordering::SeqCst) >= 2 {
            return match self.failure {
                AuthorityFailure::Database => {
                    Err(AppError::Database("return authority outage".into()))
                }
                AuthorityFailure::Policy => {
                    Err(AppError::BadRequest("return policy malformed".into()))
                }
                AuthorityFailure::Timeout => std::future::pending().await,
            };
        }
        Authority.lock_current(tx, saved).await
    }
}
#[tokio::test]
async fn workflow_action_receipt_return_authority_errors_propagate_after_commit() {
    for failure in [
        AuthorityFailure::Database,
        AuthorityFailure::Policy,
        AuthorityFailure::Timeout,
    ] {
        let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, false).await;
        let provider = dedup(&f, contract, false);
        let writer = PostgresActionDispatch::new(
            f.persistence().clone(),
            FailingReturnAuthority {
                calls: AtomicUsize::new(0),
                failure,
            },
            Effect(0),
        );
        let result = ActionService::new(writer)
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await;
        match (failure, result) {
            (AuthorityFailure::Database, Err(AppError::Database(message))) => {
                assert_eq!(message, "return authority outage")
            }
            (AuthorityFailure::Policy, Err(AppError::BadRequest(message))) => {
                assert_eq!(message, "return policy malformed")
            }
            (AuthorityFailure::Timeout, Err(AppError::Conflict(message))) => {
                assert_eq!(message, "Workflow lease transaction timed out")
            }
            _ => panic!("operational failure must remain visible"),
        }
        assert_eq!(facts(&f).await, (1, 1, 0));
        let restart = dedup(&f, None, false);
        assert!(matches!(
            ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &restart, &CancellationToken::new())
                .await
                .unwrap(),
            RemoteDispatchObservation::Committed(_)
        ));
        assert_eq!(restart.calls.load(Ordering::SeqCst), 0);
    }
}
