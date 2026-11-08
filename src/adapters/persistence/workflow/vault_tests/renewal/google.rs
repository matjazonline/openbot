use super::*;
use sha2::{Digest, Sha256};
#[tokio::test]
async fn google_refresh_valid_cache_reuses_without_endpoint_and_keeps_private_boundary() {
    let f = Box::pin(RenewalSetup::new(Utc::now() + ChronoDuration::hours(1))).await;
    let result = f.resolve().await.unwrap();
    assert_eq!(
        result.injection.value.expose_secret(),
        "old-access-sentinel"
    );
    assert_eq!(result.generation, CredentialGeneration::INITIAL);
    assert_eq!(result.authorization.account, f.account.id);
    assert_eq!(result.session_policy, SessionPolicy::RebuildOnGeneration);
    assert!(result.expires_at.unwrap() > Utc::now() + ChronoDuration::seconds(60));
    assert_eq!(f.server.calls(), 0);
    assert!(!format!("{result:?}").contains("old-access-sentinel"));
    assert_eq!(
        f.vault
            .registry
            .get(&AppId::parse("gmail").unwrap())
            .unwrap()
            .renewal
            .revocation_scope(),
        RevocationScope::CombinedProject
    );
}
#[tokio::test]
async fn google_refresh_omitted_refresh_is_preserved_and_cache_updates_do_not_change_consent() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let result = f.resolve().await.unwrap();
    assert_eq!(
        result.injection.value.expose_secret(),
        "renewed-access-sentinel"
    );
    assert_eq!(result.generation.get(), 2);
    let stored = f.credential().await;
    assert_eq!(stored.account.revision.get(), 2);
    assert_eq!(
        stored.account.authorization_revision,
        f.account.authorization_revision
    );
    let value: Value = serde_json::from_str(stored.payload.expose_secret()).unwrap();
    assert_eq!(value["refresh_token"], "refresh-sentinel");
    assert_eq!(value["generation"], 2);
    assert_eq!(f.server.calls(), 1);
    {
        let observation = f.server.state.observation.lock().unwrap();
        let observation = observation.as_ref().unwrap();
        assert_eq!(observation.grant, "refresh_token");
        assert!(observation.client_valid);
        assert_eq!(
            observation.refresh_digest,
            <[u8; 32]>::from(Sha256::digest("refresh-sentinel"))
        );
    }
    assert_eq!(f.resolve().await.unwrap().generation, result.generation);
    assert_eq!(f.server.calls(), 1);
}
#[tokio::test]
async fn google_refresh_race_http_requests_with_distinct_dispatches_share_one_refresh() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let first = prepared(&f.setup).await;
    let second = prepared(&f.setup).await;
    assert_ne!(first.0.dispatch, second.0.dispatch);
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = f.separate_service().await;
    let vault = service.vault;
    let a = tokio::spawn(async move { vault.resolve(&first.0, &first.1).await });
    f.server.wait_calls(1).await;
    let service = f.separate_service().await;
    let vault = service.vault;
    let b = tokio::spawn(async move { vault.resolve(&second.0, &second.1).await });
    f.wait_blocked(&service.application).await;
    drop(held);
    let a = a.await.unwrap().unwrap();
    let b = b.await.unwrap().unwrap();
    assert_eq!(
        a.injection.value.expose_secret(),
        b.injection.value.expose_secret()
    );
    assert_eq!(a.generation, b.generation);
    assert_eq!(f.server.calls(), 1);
}
#[tokio::test]
async fn google_refresh_race_http_and_mcp_apps_share_the_same_account_lock() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let first = prepared(&f.setup).await;
    let second = Box::pin(
        crate::adapters::persistence::workflow::tests::mcp_tests::support::renewal::prepare(
            &f.setup.fixture,
            &f.vault,
            &f.account,
            AppId::parse("gmail-read").unwrap(),
        ),
    )
    .await;
    assert_ne!(
        first.0.authorization.grant,
        second.invocation.authorization.grant
    );
    assert_ne!(
        first.0.authorization.app,
        second.invocation.authorization.app
    );
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = f.separate_service().await;
    let vault = service.vault;
    let a = tokio::spawn(async move { vault.resolve(&first.0, &first.1).await });
    f.server.wait_calls(1).await;
    let service = f.separate_service().await;
    let vault = service.vault;
    let b = tokio::spawn(async move { vault.resolve(&second.invocation, &second.context).await });
    f.wait_blocked(&service.application).await;
    drop(held);
    let a = a.await.unwrap().unwrap();
    let b = b.await.unwrap().unwrap();
    assert_eq!(a.generation, b.generation);
    assert_eq!(f.server.calls(), 1);
}
#[tokio::test]
async fn integration_renewal_static_accounts_have_no_renewal_protocol() {
    let s = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(
        &s,
        s.fixture.target.actor.user_id(),
        "static-resolution",
        "static-private",
    )
    .await;
    attach(&s, &vault, &account).await;
    let (i, c) = prepared(&s).await;
    let result = vault.resolve(&i, &c).await.unwrap();
    assert!(matches!(result.injection.method, InjectionMethod::Bearer));
    assert_eq!(result.injection.value.expose_secret(), "static-private");
    assert!(result.expires_at.is_none());
    let strategy = &vault
        .registry
        .get(&AppId::parse("static-bearer").unwrap())
        .unwrap()
        .renewal;
    assert_eq!(strategy.revocation_scope(), RevocationScope::LocalOnly);
    assert_eq!(strategy.session_policy(), SessionPolicy::Stateless);
}
