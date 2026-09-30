use super::*;
use crate::application::workflow::publication::ActionRecovery;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Copy)]
enum Response {
    Success,
    Lost,
    Invalid,
    ToolError,
    Pending,
    Metadata,
}
struct RegisteredProvider<'a> {
    f: &'a AdmissionFixture,
    contract: Option<ProviderReplayContract>,
    calls: AtomicUsize,
    keys: Mutex<Vec<ActionIdempotencyKey>>,
    response: Response,
}
#[async_trait]
impl RemoteAction for RegisteredProvider<'_> {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        self.contract.as_ref()
    }
    async fn invoke(
        &self,
        action: &FrozenAction,
        invocation: &ProviderInvocation,
    ) -> AppResult<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let saved: Value = sqlx::query_scalar("SELECT current_policy->'provider_replay' FROM workflow_action_dispatches WHERE company_id=$1")
            .bind(action.scope().company.as_uuid()).fetch_one(self.f.persistence().pool()).await?;
        assert_eq!(saved, invocation.proof()?);
        if let Some(key) = invocation.idempotency_key() {
            assert_eq!(key, &action.idempotency_key());
            self.keys.lock().unwrap().push(key.clone());
        }
        match self.response {
            Response::Success => Ok(json!({"written":true})),
            Response::Lost => Err(AppError::Timeout("lost provider response".into())),
            Response::Invalid => Ok(json!("invalid output after possible effect")),
            Response::ToolError => Ok(json!({"isError":true})),
            Response::Pending => std::future::pending().await,
            Response::Metadata => Ok(
                json!({"method":"PUT","headers":{"Idempotency-Key":"client-key"},"annotations":{"idempotentHint":true,"readOnlyHint":true},"sessionId":"session","requestId":7}),
            ),
        }
    }
}
async fn saved(f: &AdmissionFixture, request: &ActionDispatchRequest) -> FrozenAction {
    f.persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action
}
fn approved(action: &FrozenAction, mode: ProviderReplayMode) -> ProviderReplayContract {
    ProviderReplayContract::approve(
        TypeName::parse("fixture.provider.v1").unwrap(),
        &action.request().contract,
        &action.request().target,
        mode,
    )
    .unwrap()
}
fn idempotent() -> ProviderReplayMode {
    ProviderReplayMode::ProviderIdempotency {
        retention: Duration::from_secs(365 * 24 * 60 * 60),
    }
}
fn provider<'a>(
    f: &'a AdmissionFixture,
    contract: Option<ProviderReplayContract>,
    response: Response,
) -> RegisteredProvider<'a> {
    RegisteredProvider {
        f,
        contract,
        calls: AtomicUsize::new(0),
        keys: Mutex::new(vec![]),
        response,
    }
}

#[tokio::test]
async fn workflow_action_replay_approved_first_dispatch_stable_key_competing_tool_and_restart() {
    for (recovery, mode) in [
        (ActionRecovery::SafeRepeat, ProviderReplayMode::SafeRepeat),
        (ActionRecovery::ProviderIdempotency, idempotent()),
    ] {
        let (f, request) = setup_action(recovery, json!({"value":1})).await;
        let action = saved(&f, &request).await;
        let intent = ActionService::new(f.persistence().clone())
            .prepare_tool(
                action.request().clone(),
                ModelToolCallId::parse("registered-call").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(intent.approval_subject(), request.subject);
        let provider = provider(&f, Some(approved(&action, mode)), Response::Success);
        let service = ActionService::new(adapter(&f, 0));
        let mut tool = request.clone();
        tool.subject = intent.approval_subject();
        let cancel = CancellationToken::new();
        let (step, tool) = tokio::join!(
            service.dispatch_remote(&request, &provider, &cancel),
            service.dispatch_remote(&tool, &provider, &cancel),
        );
        let results = [step.unwrap(), tool.unwrap()];
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, RemoteDispatchObservation::Committed(_)))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, RemoteDispatchObservation::PossibleDispatchExists))
                .count(),
            1
        );
        let keys = provider.keys.lock().unwrap().clone();
        if recovery == ActionRecovery::ProviderIdempotency {
            assert_eq!(keys, vec![intent.idempotency_key]);
        } else {
            assert!(keys.is_empty());
        }
        assert!(matches!(
            ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &provider, &cancel)
                .await
                .unwrap(),
            RemoteDispatchObservation::Committed(_)
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(facts(&f).await, (1, 1, 0));
    }
}

#[tokio::test]
async fn workflow_action_replay_unsupported_or_mismatched_contract_never_marks_or_polls() {
    let (f, request) = setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
    let action = saved(&f, &request).await;
    let mut variants = vec![None];
    for variant in 0..8 {
        let mut changed = action.request().clone();
        match variant {
            0 => changed.contract.company_id = CompanyId::new(Uuid::new_v4()),
            1 => {
                changed.target = ActionTarget::Connection {
                    resource: Uuid::new_v4(),
                    resource_kind: TypeName::parse("mcp").unwrap(),
                }
            }
            2 => changed.contract.contract.name = TypeName::parse("other").unwrap(),
            3 => changed.contract.contract.input_schema = json!(true),
            4 => changed.contract.contract.output_schema = json!(true),
            5 => changed.contract.policy.capability = TypeName::parse("other").unwrap(),
            6 => changed.contract.policy.policy_revision += 1,
            _ => changed.contract.policy.effect = publication::ActionEffect::Read,
        }
        variants.push(Some(
            ProviderReplayContract::approve(
                TypeName::parse("wrong").unwrap(),
                &changed.contract,
                &changed.target,
                idempotent(),
            )
            .unwrap(),
        ));
    }
    variants.push(Some(approved(
        &action,
        ProviderReplayMode::ProviderIdempotency {
            retention: Duration::from_millis(1),
        },
    )));
    for contract in variants {
        let provider = provider(&f, contract, Response::Success);
        assert!(
            ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(facts(&f).await, (0, 0, 0));
    }
    for mode in [
        ProviderReplayMode::SafeRepeat,
        ProviderReplayMode::ProviderIdempotency {
            retention: Duration::ZERO,
        },
        ProviderReplayMode::ProviderIdempotency {
            retention: Duration::from_secs(366 * 24 * 60 * 60),
        },
    ] {
        assert!(
            ProviderReplayContract::approve(
                TypeName::parse("unsupported").unwrap(),
                &action.request().contract,
                &action.request().target,
                mode
            )
            .is_err()
        );
    }
}

#[tokio::test]
async fn workflow_action_replay_frozen_policy_alone_does_not_register_provider_safety() {
    for recovery in [
        ActionRecovery::SafeRepeat,
        ActionRecovery::ProviderIdempotency,
    ] {
        let (f, request) = setup_action(recovery, json!({"value":1})).await;
        let provider = provider(&f, None, Response::Success);
        assert!(
            ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(facts(&f).await, (0, 0, 0));
    }
}

#[tokio::test]
async fn workflow_action_replay_ambiguous_observations_never_issue_second_permit() {
    for recovery in [
        ActionRecovery::Reconcile,
        ActionRecovery::ProviderIdempotency,
    ] {
        for response in [
            Response::Lost,
            Response::Invalid,
            Response::ToolError,
            Response::Pending,
        ] {
            let (f, request) = setup_action(recovery, json!({"value":1})).await;
            let action = saved(&f, &request).await;
            let contract = (recovery == ActionRecovery::ProviderIdempotency)
                .then(|| approved(&action, idempotent()));
            let provider = provider(&f, contract, response);
            let service = ActionService::new(adapter(&f, 0));
            let cancel = CancellationToken::new();
            let dispatch = service.dispatch_remote(&request, &provider, &cancel);
            if matches!(response, Response::Pending) {
                let stop = async {
                    tokio::time::timeout(Duration::from_secs(5), async {
                        while provider.calls.load(Ordering::SeqCst) == 0 {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                    cancel.cancel();
                };
                let (result, ()) = tokio::join!(dispatch, stop);
                assert!(matches!(
                    result.unwrap(),
                    RemoteDispatchObservation::Interrupted
                ));
            } else if matches!(response, Response::ToolError) {
                assert!(matches!(
                    dispatch.await.unwrap(),
                    RemoteDispatchObservation::Committed(_)
                ));
            } else {
                assert!(dispatch.await.is_err());
            }
            let replay = ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .unwrap();
            let received = matches!(response, Response::ToolError);
            assert!(if received {
                matches!(replay, RemoteDispatchObservation::Committed(_))
            } else {
                matches!(replay, RemoteDispatchObservation::PossibleDispatchExists)
            });
            assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
            assert_eq!(facts(&f).await, (1, i64::from(received), 0));
        }
    }
}

#[tokio::test]
async fn workflow_action_replay_metadata_headers_and_changed_arguments_cannot_grant_retry() {
    let (f, request) = setup_action(ActionRecovery::Reconcile, json!({"value":1})).await;
    let action = saved(&f, &request).await;
    // Schema-valid output is receipt truth; metadata still cannot register replay safety.
    let provider = provider(&f, None, Response::Metadata);
    let service = ActionService::new(adapter(&f, 0));
    assert!(matches!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert!(matches!(
        service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    let mut changed = action.request().clone();
    changed.arguments = json!({"value":2});
    let changed = ActionService::new(f.persistence().clone())
        .prepare_step(changed)
        .await
        .unwrap();
    assert_ne!(changed.approval_subject(), request.subject);
    assert_ne!(changed.idempotency_key, action.idempotency_key());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_action_replay_first_proof_crash_and_tampered_entry_stay_conservative() {
    for tamper in [false, true] {
        let (f, request) =
            setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
        let action = saved(&f, &request).await;
        let contract = approved(&action, idempotent());
        let writer = adapter(&f, 0);
        let RemoteReservationResult::Reserved(mut reservation) = writer
            .reserve_remote(&request, Some(&contract))
            .await
            .unwrap()
        else {
            panic!("first approved marker")
        };
        assert!(
            sqlx::query(
                "UPDATE workflow_action_dispatches SET current_policy='{}' WHERE company_id=$1"
            )
            .bind(action.scope().company.as_uuid())
            .execute(f.persistence().pool())
            .await
            .is_err()
        );
        if tamper {
            let other = ProviderReplayContract::approve(
                TypeName::parse("fixture.provider.v2").unwrap(),
                &action.request().contract,
                &action.request().target,
                idempotent(),
            )
            .unwrap();
            reservation.provider =
                prepare_provider(&action, Some(&other), Duration::from_secs(1)).unwrap();
            assert!(writer.enter_remote(*reservation).await.is_err());
        } else {
            drop(reservation);
        }
        let provider = provider(&f, Some(contract), Response::Success);
        assert!(matches!(
            ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .unwrap(),
            RemoteDispatchObservation::PossibleDispatchExists
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(facts(&f).await, (1, 0, 0));
    }
}

#[tokio::test]
async fn workflow_action_replay_delayed_lock_cannot_shorten_retention_horizon() {
    let (f, request) = setup_action(ActionRecovery::ProviderIdempotency, json!({"value":1})).await;
    let action = saved(&f, &request).await;
    let provider = provider(
        &f,
        Some(approved(
            &action,
            ProviderReplayMode::ProviderIdempotency {
                retention: Duration::from_millis(900),
            },
        )),
        Response::Success,
    );
    sqlx::query(
        "UPDATE workflow_runs SET deadline=clock_timestamp()+interval '2 seconds' WHERE id=$1",
    )
    .bind(request.scope().run.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let mut held = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(request.scope().run.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    let release = async {
        crate::adapters::persistence::test_support::wait_until_backends_are_blocked(
            f.persistence().pool(),
            1,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(700)).await;
        let remaining: i64 = sqlx::query_scalar("SELECT ceil(extract(epoch FROM (deadline-clock_timestamp()))*1000000)::bigint FROM workflow_runs WHERE id=$1")
            .bind(request.scope().run.as_uuid()).fetch_one(&mut *held).await.unwrap();
        assert!(
            remaining > 900_000 && remaining < 1_400_000,
            "fixture must retain a horizon longer than approved retention after the real lock wait: {remaining}"
        );
        held.commit().await.unwrap();
    };
    let service = ActionService::new(adapter(&f, 0));
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(
        service.dispatch_remote(&request, &provider, &cancel),
        release
    );
    assert!(result.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(facts(&f).await, (0, 0, 0));
}
