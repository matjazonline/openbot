use super::*;
use crate::adapters::persistence::integrations::{
    renewal_tests::workspace, resolve::testsupport as protocol,
};
struct StrictSetup {
    setup: Setup,
    vault: PostgresVault,
    account: ConnectedAccount,
    server: TokenServer,
}
impl StrictSetup {
    async fn new() -> Self {
        let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
        let server = TokenServer::new().await;
        *server.state.reply.lock().unwrap() = Reply::Json(workspace::response());
        let entry = workspace::registration(&server);
        let payload = workspace::payload();
        let write = AccountWrite {
            key: AccountKey::parse("strict-private").unwrap(),
            provider: entry.provider.id.clone(),
            config: entry.config.id.clone(),
            method: entry.provider.method,
            schema: entry.provider.schema.clone(),
            identity: entry.adapter.validate(&payload).unwrap(),
            metadata: AccountMetadata::new("Workspace without email".into(), None).unwrap(),
            payload,
        };
        let mut registry = CodeOwnedRegistry::default();
        registry.register(entry).unwrap();
        let vault =
            PostgresVault::with_registry(setup.fixture.persistence.clone(), Arc::new(registry));
        let account = vault
            .create(&owner_for_user(setup.fixture.target.actor.user_id()), write)
            .await
            .unwrap();
        attach_app(
            &setup,
            &vault,
            &account,
            AppId::parse("workspace-strict").unwrap(),
        )
        .await;
        Self {
            setup,
            vault,
            account,
            server,
        }
    }
}
#[tokio::test]
async fn integration_renewal_strict_workspace_rotates_and_reuses_without_an_email() {
    let f = Box::pin(StrictSetup::new()).await;
    let (i, c) = prepared(&f.setup).await;
    let result = f.vault.resolve(&i, &c).await.unwrap();
    assert_eq!(result.generation.get(), 2);
    assert_eq!(
        result.injection.value.expose_secret(),
        "workspace-renewed-token"
    );
    let stored = f
        .vault
        .read_usable(&f.account.owner, f.account.id)
        .await
        .unwrap();
    let payload: Value = serde_json::from_str(stored.payload.expose_secret()).unwrap();
    assert_eq!(payload["refresh"], "replacement-refresh");
    assert!(!payload.as_object().unwrap().contains_key("email"));
    assert_eq!(
        stored.versions.authorization,
        f.account.authorization_revision
    );
    assert!(!protocol::pending(&f.vault, &f.account).await);
    assert_eq!(
        f.vault.resolve(&i, &c).await.unwrap().generation,
        result.generation
    );
    assert_eq!(f.server.calls(), 1);
    assert_eq!(
        f.vault
            .registry
            .get(&AppId::parse("workspace-strict").unwrap())
            .unwrap()
            .renewal
            .revocation_scope(),
        RevocationScope::Installation
    );
}
#[tokio::test]
async fn integration_renewal_strict_active_pending_is_not_an_interrupted_claimant() {
    let f = Box::pin(StrictSetup::new()).await;
    let (i, c) = prepared(&f.setup).await;
    let claim = protocol::reserve(&f.vault, &i, &c).await.unwrap();
    assert!(protocol::pending(&f.vault, &f.account).await);
    let competing = f.vault.resolve(&i, &c).await;
    assert!(matches!(
        competing,
        Err(ResolutionError::Retry(RetryReason::Pending, Some(_)))
    ));
    assert_eq!(f.server.calls(), 0);
    assert_eq!(
        f.vault
            .get(&f.account.owner, f.account.id)
            .await
            .unwrap()
            .status,
        AccountStatus::Ready
    );
    let result = protocol::complete(&f.vault, &claim, &i, &c).await.unwrap();
    assert_eq!(result.generation.get(), 2);
    assert!(protocol::complete(&f.vault, &claim, &i, &c).await.is_err());
    assert_eq!(f.server.calls(), 1);
}
#[tokio::test]
async fn integration_renewal_strict_expired_pending_requires_reconnect_without_an_endpoint_retry() {
    let f = Box::pin(StrictSetup::new()).await;
    let (i, c) = prepared(&f.setup).await;
    let claim = protocol::reserve(&f.vault, &i, &c).await.unwrap();
    protocol::expire(&f.vault, &f.account, &claim).await;
    assert_eq!(
        f.vault.resolve(&i, &c).await.unwrap_err(),
        ResolutionError::ReconnectRequired
    );
    assert!(!protocol::pending(&f.vault, &f.account).await);
    assert_eq!(
        f.vault
            .get(&f.account.owner, f.account.id)
            .await
            .unwrap()
            .status,
        AccountStatus::ReconnectRequired
    );
    assert!(protocol::complete(&f.vault, &claim, &i, &c).await.is_err());
    assert_eq!(f.server.calls(), 0);
}
#[tokio::test]
async fn integration_renewal_strict_cancellation_keeps_durable_ambiguity_for_recovery() {
    let f = Box::pin(StrictSetup::new()).await;
    let (i, c) = prepared(&f.setup).await;
    let claim = protocol::reserve(&f.vault, &i, &c).await.unwrap();
    let copied = claim.clone();
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let cancel = c.cancellation.clone();
    let vault = f.vault.clone();
    let worker = tokio::spawn(async move { protocol::complete(&vault, &copied, &i, &c).await });
    f.server.wait_calls(1).await;
    cancel.cancel();
    assert_eq!(
        worker.await.unwrap().unwrap_err(),
        ResolutionError::Cancelled
    );
    assert!(protocol::pending(&f.vault, &f.account).await);
    drop(held);
    let (i, c) = prepared(&f.setup).await;
    assert!(matches!(
        f.vault.resolve(&i, &c).await,
        Err(ResolutionError::Retry(RetryReason::Pending, _))
    ));
    protocol::expire(&f.vault, &f.account, &claim).await;
    assert_eq!(
        f.vault.resolve(&i, &c).await.unwrap_err(),
        ResolutionError::ReconnectRequired
    );
    assert_eq!(f.server.calls(), 1);
}
#[derive(Clone, Copy)]
enum Removal {
    Disconnect,
    Delete,
}
async fn delayed_claim(removal: Removal) {
    let f = Box::pin(StrictSetup::new()).await;
    let (i, c) = prepared(&f.setup).await;
    let claim = protocol::reserve(&f.vault, &i, &c).await.unwrap();
    match removal {
        Removal::Disconnect => {
            f.vault
                .disconnect(
                    &f.account.owner,
                    f.account.id,
                    f.account.authorization_revision,
                )
                .await
                .unwrap();
        }
        Removal::Delete => {
            f.vault
                .delete(
                    &f.account.owner,
                    f.account.id,
                    f.account.authorization_revision,
                )
                .await
                .unwrap();
        }
    }
    assert!(protocol::complete(&f.vault, &claim, &i, &c).await.is_err());
    assert_eq!(f.server.calls(), 0);
}
#[tokio::test]
async fn integration_renewal_strict_delayed_claim_after_disconnect_cannot_contact_provider() {
    Box::pin(delayed_claim(Removal::Disconnect)).await;
}
#[tokio::test]
async fn integration_renewal_strict_delayed_claim_after_delete_cannot_contact_provider() {
    Box::pin(delayed_claim(Removal::Delete)).await;
}
#[tokio::test]
async fn integration_renewal_strict_omitted_replacement_does_not_inherit_google_reuse() {
    let f = Box::pin(StrictSetup::new()).await;
    let (i, c) = prepared(&f.setup).await;
    let mut reply = workspace::response();
    reply.as_object_mut().unwrap().remove("refresh_token");
    *f.server.state.reply.lock().unwrap() = Reply::Json(reply);
    assert_eq!(
        f.vault.resolve(&i, &c).await.unwrap_err(),
        ResolutionError::Integration(IntegrationError::Credential)
    );
    assert!(protocol::pending(&f.vault, &f.account).await);
    assert!(matches!(
        f.vault.resolve(&i, &c).await,
        Err(ResolutionError::Retry(RetryReason::Pending, _))
    ));
    assert_eq!(f.server.calls(), 1);
}
#[tokio::test]
async fn integration_renewal_strict_lost_response_preserves_pending_instead_of_retrying_refresh() {
    let f = Box::pin(StrictSetup::new()).await;
    let (i, c) = prepared(&f.setup).await;
    *f.server.state.reply.lock().unwrap() = Reply::Lost;
    assert!(matches!(
        f.vault.resolve(&i, &c).await,
        Err(ResolutionError::Retry(RetryReason::Pending, _))
    ));
    assert!(protocol::pending(&f.vault, &f.account).await);
    assert!(matches!(
        f.vault.resolve(&i, &c).await,
        Err(ResolutionError::Retry(RetryReason::Pending, _))
    ));
    assert_eq!(f.server.calls(), 1);
}

#[path = "strict_commit.rs"]
mod commit;

#[tokio::test]
async fn integration_renewal_strict_competing_live_services_share_one_rotation() {
    let f = Box::pin(StrictSetup::new()).await;
    let first = prepared(&f.setup).await;
    let second = prepared(&f.setup).await;
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = independent_service(&f.vault).await;
    let a = tokio::spawn(async move { service.vault.resolve(&first.0, &first.1).await });
    f.server.wait_calls(1).await;
    let service = independent_service(&f.vault).await;
    let application = service.application.clone();
    let b = tokio::spawn(async move { service.vault.resolve(&second.0, &second.1).await });
    wait_blocked(&f.vault, &application).await;
    drop(held);
    assert_eq!(a.await.unwrap().unwrap().generation.get(), 2);
    assert_eq!(b.await.unwrap().unwrap().generation.get(), 2);
    assert_eq!(f.server.calls(), 1);
}
#[tokio::test]
async fn integration_renewal_strict_backend_crash_keeps_the_committed_pending_marker() {
    let f = Box::pin(StrictSetup::new()).await;
    let (i, c) = prepared(&f.setup).await;
    let service = independent_service(&f.vault).await;
    let claim = protocol::reserve(&service.vault, &i, &c).await.unwrap();
    let application = service.application.clone();
    let copied = claim.clone();
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let worker =
        tokio::spawn(async move { protocol::complete(&service.vault, &copied, &i, &c).await });
    f.server.wait_calls(1).await;
    let pids:Vec<i32>=sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND state='idle in transaction'").bind(application).fetch_all(f.vault.persistence.pool()).await.unwrap();
    assert_eq!(pids.len(), 1);
    let done: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
        .bind(pids[0])
        .fetch_one(f.vault.persistence.pool())
        .await
        .unwrap();
    assert!(done);
    drop(held);
    assert!(worker.await.unwrap().is_err());
    assert!(protocol::pending(&f.vault, &f.account).await);
    let (i, c) = prepared(&f.setup).await;
    assert!(matches!(
        f.vault.resolve(&i, &c).await,
        Err(ResolutionError::Retry(RetryReason::Pending, _))
    ));
    protocol::expire(&f.vault, &f.account, &claim).await;
    assert_eq!(
        f.vault.resolve(&i, &c).await.unwrap_err(),
        ResolutionError::ReconnectRequired
    );
    assert_eq!(f.server.calls(), 1);
}
