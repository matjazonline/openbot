use super::*;
use crate::adapters::persistence::integrations::renewal_tests::commit_proxy::{CommitProxy, Fault};
pub(super) async fn through(vault: &PostgresVault, proxy: &CommitProxy) -> PostgresVault {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect_with(proxy.options.clone())
        .await
        .unwrap();
    PostgresVault::with_registry(
        PostgresPersistence::with_credential_cipher(pool, CredentialCipher::for_test()),
        vault.registry.clone(),
    )
}
#[tokio::test]
async fn google_refresh_lost_commit_acknowledgement_rereads_committed_cache_before_return() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let proxy = CommitProxy::new(f.setup.fixture.persistence.pool(), Fault::CredentialWrite).await;
    let vault = through(&f.vault, &proxy).await;
    let (i, c) = prepared(&f.setup).await;
    let result = vault.resolve(&i, &c).await.unwrap();
    assert_eq!(result.generation.get(), 2);
    assert_eq!(proxy.dropped(), 1);
    assert_eq!(f.server.calls(), 1);
    assert_eq!(f.credential().await.versions.generation, result.generation);
    assert_eq!(
        vault.resolve(&i, &c).await.unwrap().generation,
        result.generation
    );
    assert_eq!(f.server.calls(), 1);
}
#[tokio::test]
async fn google_refresh_ambiguous_ack_cannot_restore_authority_after_newer_disconnect() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let proxy = CommitProxy::new(f.setup.fixture.persistence.pool(), Fault::CredentialWrite).await;
    let held = proxy.gate().acquire_owned().await.unwrap();
    let vault = through(&f.vault, &proxy).await;
    let (i, c) = prepared(&f.setup).await;
    let worker = tokio::spawn(async move { vault.resolve(&i, &c).await });
    proxy.wait_drop().await;
    let disconnected = f
        .vault
        .disconnect(
            &f.account.owner,
            f.account.id,
            f.account.authorization_revision,
        )
        .await
        .unwrap();
    assert_eq!(disconnected.status, AccountStatus::Disconnected);
    drop(held);
    assert!(worker.await.unwrap().is_err());
    assert_eq!(proxy.dropped(), 1);
    assert_eq!(f.server.calls(), 1);
    assert_eq!(
        f.vault
            .get(&f.account.owner, f.account.id)
            .await
            .unwrap()
            .authorization_revision,
        disconnected.authorization_revision
    );
}
#[tokio::test]
async fn google_refresh_lost_cooldown_commit_ack_preserves_shared_backoff() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    *f.server.state.status.lock().unwrap() = axum::http::StatusCode::SERVICE_UNAVAILABLE;
    *f.server.state.reply.lock().unwrap() = Reply::Json(json!({"error":"temporarily_unavailable"}));
    let proxy = CommitProxy::new(f.setup.fixture.persistence.pool(), Fault::CredentialWrite).await;
    let vault = through(&f.vault, &proxy).await;
    let (i, c) = prepared(&f.setup).await;
    assert!(matches!(
        vault.resolve(&i, &c).await,
        Err(ResolutionError::Retry(RetryReason::Cooldown, _))
    ));
    assert_eq!(proxy.dropped(), 1);
    assert!(matches!(
        vault.resolve(&i, &c).await,
        Err(ResolutionError::Retry(RetryReason::Cooldown, Some(_)))
    ));
    assert_eq!(f.server.calls(), 1);
}
