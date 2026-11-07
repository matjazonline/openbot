use super::*;
use crate::application::integrations::registry::Registration;
use async_trait::async_trait;
use std::sync::atomic::AtomicBool;
struct Hung {
    inner: Arc<dyn RenewalProvider>,
    dropped: Arc<AtomicBool>,
    entered: Arc<AtomicUsize>,
}
struct DropWitness(Arc<AtomicBool>);
impl Drop for DropWitness {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
#[async_trait]
impl RenewalProvider for Hung {
    fn session_policy(&self) -> SessionPolicy {
        self.inner.session_policy()
    }
    fn revocation_scope(&self) -> RevocationScope {
        self.inner.revocation_scope()
    }
    fn assess(
        &self,
        p: &SecretString,
        g: CredentialGeneration,
        n: chrono::DateTime<Utc>,
    ) -> ResolutionResult<RenewalDecision> {
        self.inner.assess(p, g, n)
    }
    fn cooldown(
        &self,
        p: &SecretString,
        n: chrono::DateTime<Utc>,
    ) -> IntegrationResult<SecretString> {
        self.inner.cooldown(p, n)
    }
    async fn renew(
        &self,
        _: &SecretString,
        _: CredentialGeneration,
        _: &RenewalWork,
    ) -> Result<SecretString, RenewalFailure> {
        let _witness = DropWitness(self.dropped.clone());
        self.entered.fetch_add(1, Ordering::SeqCst);
        std::future::pending().await
    }
}
fn hung_registry(
    f: &RenewalSetup,
    dropped: Arc<AtomicBool>,
    entered: Arc<AtomicUsize>,
) -> Arc<CodeOwnedRegistry> {
    let mut registry = CodeOwnedRegistry::default();
    for app in ["gmail", "gmail-read"] {
        let entry = f.vault.registry.get(&AppId::parse(app).unwrap()).unwrap();
        let RenewalStrategy::ReusableRefresh(inner) = &entry.renewal else {
            panic!("Reusable provider")
        };
        registry
            .register(Registration {
                provider: entry.provider.clone(),
                config: entry.config.clone(),
                app: entry.app.clone(),
                adapter: entry.adapter.clone(),
                authorization: entry.authorization.clone(),
                renewal: RenewalStrategy::ReusableRefresh(Arc::new(Hung {
                    inner: inner.clone(),
                    dropped: dropped.clone(),
                    entered: entered.clone(),
                })),
            })
            .unwrap();
    }
    Arc::new(registry)
}
#[tokio::test]
async fn integration_renewal_core_ten_second_provider_budget_drops_an_uncooperative_future() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let dropped = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicUsize::new(0));
    let vault = PostgresVault::with_registry(
        f.vault.persistence.clone(),
        hung_registry(&f, dropped.clone(), entered.clone()),
    );
    let (i, c) = prepared(&f.setup).await;
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(std::time::Duration::from_secs(13), vault.resolve(&i, &c))
        .await
        .expect("Core provider budget must fire before13s");
    assert!(matches!(
        result,
        Err(ResolutionError::Retry(RetryReason::Cooldown, _))
    ));
    assert!(start.elapsed() >= std::time::Duration::from_secs(9));
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(entered.load(Ordering::SeqCst), 1);
    assert!(matches!(
        vault.resolve(&i, &c).await,
        Err(ResolutionError::Retry(RetryReason::Cooldown, _))
    ));
    assert_eq!(entered.load(Ordering::SeqCst), 1);
    f.vault
        .disconnect(
            &f.account.owner,
            f.account.id,
            f.account.authorization_revision,
        )
        .await
        .unwrap();
    assert_eq!(f.server.calls(), 0);
}
#[tokio::test]
async fn google_refresh_five_second_lock_wait_is_retryable_and_cancel_releases_the_account() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let first = prepared(&f.setup).await;
    let second = prepared(&f.setup).await;
    let cancel = first.1.cancellation.clone();
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = f.separate_service().await;
    let a = tokio::spawn(async move { service.vault.resolve(&first.0, &first.1).await });
    f.server.wait_calls(1).await;
    let service = f.separate_service().await;
    let application = service.application.clone();
    let start = std::time::Instant::now();
    let b = tokio::spawn(async move { service.vault.resolve(&second.0, &second.1).await });
    f.wait_blocked(&application).await;
    let blocked = tokio::time::timeout(std::time::Duration::from_secs(7), b)
        .await
        .expect("Entry lock wait bounded")
        .unwrap();
    assert!(matches!(blocked, Err(ResolutionError::Retry(_, _))));
    assert!(start.elapsed() >= std::time::Duration::from_secs(4));
    cancel.cancel();
    assert_eq!(a.await.unwrap().unwrap_err(), ResolutionError::Cancelled);
    drop(held);
    assert_eq!(
        f.credential().await.versions.generation,
        CredentialGeneration::INITIAL
    );
    assert_eq!(f.server.calls(), 1);
    f.vault
        .disconnect(
            &f.account.owner,
            f.account.id,
            f.account.authorization_revision,
        )
        .await
        .unwrap();
}
#[tokio::test]
async fn integration_renewal_transaction_settings_are_local_and_match_the_request_budget() {
    use crate::adapters::persistence::integrations::resolve::testsupport;
    let f = Box::pin(RenewalSetup::new(Utc::now() + ChronoDuration::hours(1))).await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            f.vault
                .persistence
                .pool()
                .connect_options()
                .as_ref()
                .clone(),
        )
        .await
        .unwrap();
    let vault = PostgresVault::with_registry(
        PostgresPersistence::with_credential_cipher(pool, CredentialCipher::for_test()),
        f.vault.registry.clone(),
    );
    let before = testsupport::outside_settings(&vault).await;
    let bounded = testsupport::resolver_settings(&vault).await;
    let after = testsupport::outside_settings(&vault).await;
    assert_eq!(before.pid, bounded.pid);
    assert_eq!(bounded.pid, after.pid);
    assert_eq!(bounded.lock_timeout, "5s");
    assert_eq!(bounded.statement_timeout, "5s");
    assert_eq!(bounded.idle_timeout, "12s");
    assert_eq!(before.lock_timeout, after.lock_timeout);
    assert_eq!(before.statement_timeout, after.statement_timeout);
    assert_eq!(before.idle_timeout, after.idle_timeout);
}

async fn caller_proof(
    setup: &Setup,
    deadline: chrono::DateTime<Utc>,
) -> (AuthorizedInvocation, ExecutionContext) {
    let p = &setup.fixture.persistence;
    let request = p
        .http_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    p.prepare_integration_invocation(
        ActionDispatchRequest {
            subject: intent.approval_subject(),
            fence: setup.claim.fence,
            lease: lease(),
        },
        DispatchId::new(Uuid::new_v4()),
        deadline,
        CancellationToken::new(),
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn google_refresh_caller_deadline_caps_the_actual_request_and_never_returns_a_token() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let (i, c) = caller_proof(&f.setup, Utc::now() + ChronoDuration::seconds(2)).await;
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = f.separate_service().await;
    let worker = tokio::spawn(async move { service.vault.resolve(&i, &c).await });
    f.server.wait_calls(1).await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(4), worker)
            .await
            .expect("Caller deadline caps refresh")
            .unwrap()
            .is_err()
    );
    drop(held);
    let saved = f.credential().await;
    assert_eq!(
        saved.versions.authorization,
        f.account.authorization_revision
    );
    assert_eq!(saved.versions.generation, CredentialGeneration::INITIAL);
    assert_eq!(f.server.calls(), 1);
}
