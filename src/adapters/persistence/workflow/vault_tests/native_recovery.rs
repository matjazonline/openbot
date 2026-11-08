use super::*;
use crate::{
    adapters::{
        integrations::workflow_dispatch::IntegrationActionDispatch,
        mcp::preparation_test_server::Stage,
        persistence::{
            integrations::resolve::testsupport as protocol,
            workflow::{action_dispatch::PostgresActionDispatch, http::HttpActionAuthority},
        },
        workflow_http::NoLocal,
    },
    app_error::AppResult,
    application::workflow::http::HttpActionPreparation,
};

struct NoLegacy;
#[async_trait]
impl RemoteAction for NoLegacy {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, _: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        panic!("private gateway required");
    }
}
type Dispatch = IntegrationActionDispatch<HttpActionAuthority, NoLocal>;
fn native_dispatch(vault: &PostgresVault, peer: &BusinessPeer, case: Case) -> Dispatch {
    let p = vault.persistence.clone();
    IntegrationActionDispatch::new(
        p.clone(),
        PostgresActionDispatch::new(p, HttpActionAuthority, NoLocal),
        Arc::new(AuthenticatedExecution {
            registry: vault.registry.clone(),
            credentials: Arc::new(vault.clone()),
            transports: peer.transports(case.destination()),
        }),
    )
}
async fn request(f: &Journey) -> ActionDispatchRequest {
    let p = &f.setup.fixture.persistence;
    let request = p
        .http_step_request(&f.setup.claim, &f.setup.kind)
        .await
        .unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: f.setup.claim.fence,
        lease: lease(),
    }
}
#[derive(Clone, Copy, Debug)]
enum CancelAt {
    Lock,
    Exchange,
    Business,
}
#[tokio::test]
async fn vault_google_e2e_native_cancel_releases_lock_and_never_retries_an_uncertain_business_send()
{
    for case in [Case::Google, Case::Workspace] {
        for stage in [CancelAt::Lock, CancelAt::Exchange, CancelAt::Business] {
            Box::pin(cancel_at(case, stage)).await;
        }
    }
}
async fn cancel_at(case: Case, stage: CancelAt) {
    let mut f = Box::pin(Journey::new(case)).await;
    let hold = Stage::new();
    if matches!(stage, CancelAt::Business) {
        let peer = BusinessPeer::new(
            CredentialExpectation::Bearer(SecretString::from(case.access())),
            json!({"accepted":true}),
            Some(hold.clone()),
        )
        .await;
        std::mem::replace(&mut f.peer, peer).close().await;
    }
    let service = Box::pin(renewal_support::independent_service(&f.vault)).await;
    let dispatch = native_dispatch(&service.vault, &f.peer, case);
    let request = Box::pin(request(&f)).await;
    let cancel = CancellationToken::new();
    let mut lock = vault_lock(&f, stage).await;
    let exchange = if matches!(stage, CancelAt::Exchange) {
        Some(f.token.state.gate.clone().acquire_owned().await.unwrap())
    } else {
        None
    };
    let operation = async {
        tokio::join!(
            Box::pin(dispatch.remote(&request, &NoLegacy, &cancel)),
            async {
                cancel_after(&f, &service, stage, &hold, &cancel).await;
            }
        )
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(12), operation)
        .await
        .expect("native cancellation must settle within the existing preparation bounds");
    drop(exchange);
    if let Some(tx) = lock.take() {
        tx.rollback().await.unwrap();
    }
    Box::pin(assert_recovery(
        &f,
        &dispatch,
        &request,
        case,
        stage,
        result.unwrap(),
    ))
    .await;
    assert!(
        f.vault
            .read_usable(&f.account.owner, f.account.id)
            .await
            .is_ok()
    );
    service.vault.persistence.pool().close().await;
    f.peer.close().await;
}

async fn vault_lock(
    f: &Journey,
    stage: CancelAt,
) -> Option<sqlx::Transaction<'static, sqlx::Postgres>> {
    if !matches!(stage, CancelAt::Lock) {
        return None;
    }
    let mut tx = f.vault.persistence.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM integration_accounts WHERE id=$1 FOR UPDATE")
        .bind(f.account.id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    Some(tx)
}
async fn cancel_after(
    f: &Journey,
    service: &renewal_support::ServiceInstance,
    stage: CancelAt,
    hold: &Stage,
    cancel: &CancellationToken,
) {
    match stage {
        CancelAt::Lock => renewal_support::wait_blocked(&f.vault, &service.application).await,
        CancelAt::Exchange => f.token.wait_calls(1).await,
        CancelAt::Business => hold.observed().await,
    }
    cancel.cancel();
}
async fn assert_recovery(
    f: &Journey,
    dispatch: &Dispatch,
    request: &ActionDispatchRequest,
    case: Case,
    stage: CancelAt,
    outcome: RemoteDispatchObservation,
) {
    assert!(
        !matches!(outcome, RemoteDispatchObservation::Committed(_)),
        "{stage:?}"
    );
    if !matches!(stage, CancelAt::Business) {
        assert!(
            matches!(
                outcome,
                RemoteDispatchObservation::PreparationFailed(PreDispatchFailure::Cancelled)
                    | RemoteDispatchObservation::Interrupted
            ),
            "{stage:?} must observe the real cancellation"
        );
    }
    let retry_cancel = CancellationToken::new();
    let after = Box::pin(dispatch.remote(request, &NoLegacy, &retry_cancel));
    match stage {
        CancelAt::Business => {
            assert!(matches!(
                after.await.unwrap(),
                RemoteDispatchObservation::PossibleDispatchExists
            ));
            assert_eq!(f.peer.calls(), 1);
            assert_eq!(
                action_counts(&f.setup.fixture).await,
                ActionCounts {
                    dispatches: 1,
                    entries: 1,
                    receipts: 0
                }
            );
        }
        _ => {
            drop(after);
            assert_eq!(f.peer.calls(), 0);
            assert_eq!(
                action_counts(&f.setup.fixture).await,
                ActionCounts {
                    dispatches: 0,
                    entries: 0,
                    receipts: 0
                }
            );
        }
    }
    if matches!(case, Case::Workspace) && matches!(stage, CancelAt::Exchange) {
        assert!(protocol::pending(&f.vault, &f.account).await);
        let restarted = native_dispatch(&f.vault, &f.peer, case);
        assert!(matches!(
            restarted
                .remote(request, &NoLegacy, &CancellationToken::new())
                .await
                .unwrap(),
            RemoteDispatchObservation::PreparationFailed(_)
        ));
        assert_eq!(
            f.token.calls(),
            1,
            "committed pending prevents reuse after cancellation"
        );
    }
}

#[tokio::test]
async fn vault_google_e2e_strict_native_backend_crash_after_response_preserves_pending_on_restart()
{
    let f = Box::pin(Journey::new(Case::Workspace)).await;
    let service = Box::pin(renewal_support::independent_service(&f.vault)).await;
    let mut gate = f.vault.persistence.pool().acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(773071)")
        .execute(&mut *gate)
        .await
        .unwrap();
    sqlx::query("CREATE FUNCTION vault_e2e_commit_gate() RETURNS trigger LANGUAGE plpgsql AS $$BEGIN IF NEW.credential_generation>OLD.credential_generation AND NEW.envelope IS DISTINCT FROM OLD.envelope THEN PERFORM pg_advisory_xact_lock(773071); END IF; RETURN NEW; END$$").execute(f.vault.persistence.pool()).await.unwrap();
    sqlx::query("CREATE TRIGGER vault_e2e_commit_gate BEFORE UPDATE ON integration_accounts FOR EACH ROW EXECUTE FUNCTION vault_e2e_commit_gate()").execute(f.vault.persistence.pool()).await.unwrap();
    let dispatch = native_dispatch(&service.vault, &f.peer, Case::Workspace);
    let request = Box::pin(request(&f)).await;
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(
        Box::pin(dispatch.remote(&request, &NoLegacy, &cancel)),
        async {
            f.token.wait_calls(1).await;
            renewal_support::wait_blocked(&f.vault, &service.application).await;
            let pids:Vec<i32>=sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND cardinality(pg_blocking_pids(pid))>0")
            .bind(&service.application).fetch_all(f.vault.persistence.pool()).await.unwrap();
            assert_eq!(pids.len(), 1);
            assert!(
                sqlx::query_scalar::<_, bool>("SELECT pg_terminate_backend($1)")
                    .bind(pids[0])
                    .fetch_one(f.vault.persistence.pool())
                    .await
                    .unwrap()
            );
        }
    );
    assert!(!matches!(
        result.unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    sqlx::query("SELECT pg_advisory_unlock(773071)")
        .execute(&mut *gate)
        .await
        .unwrap();
    sqlx::query("DROP TRIGGER vault_e2e_commit_gate ON integration_accounts")
        .execute(f.vault.persistence.pool())
        .await
        .unwrap();
    assert!(protocol::pending(&f.vault, &f.account).await);
    let restart = native_dispatch(&f.vault, &f.peer, Case::Workspace);
    assert!(matches!(
        restart
            .remote(&request, &NoLegacy, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PreparationFailed(_)
    ));
    assert_eq!(f.token.calls(), 1);
    assert_eq!(f.peer.calls(), 0);
    assert_eq!(
        action_counts(&f.setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
    service.vault.persistence.pool().close().await;
    f.peer.close().await;
}
