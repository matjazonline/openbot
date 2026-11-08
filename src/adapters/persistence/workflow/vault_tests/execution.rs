use super::*;
use crate::application::integrations::{
    execute::{AuthenticatedExecution, AuthenticatedTransports},
    private::{InjectionMethod, TypedInjection},
    renewal::ResolutionResult,
};
use async_trait::async_trait;
use tokio::sync::Notify;
#[path = "native_portability.rs"]
mod native_portability;
#[path = "projection.rs"]
mod projection;
#[path = "wire.rs"]
mod wire;

#[derive(Clone)]
pub(super) enum Delivery {
    Accepted,
    Uncertain,
    PreparationUnavailable,
    Wait {
        started: Arc<Notify>,
        release: Arc<Notify>,
    },
}
pub(super) struct ScriptedTransport {
    expected: SecretString,
    method: &'static str,
    pub(super) preparations: Arc<AtomicUsize>,
    pub(super) calls: Arc<AtomicUsize>,
    closes: Arc<AtomicUsize>,
    delivery: Delivery,
}
impl ScriptedTransport {
    pub(super) fn new(secret: &str, method: &'static str, delivery: Delivery) -> Arc<Self> {
        Arc::new(Self {
            expected: SecretString::from(secret.to_owned()),
            method,
            preparations: Arc::new(AtomicUsize::new(0)),
            calls: Arc::new(AtomicUsize::new(0)),
            closes: Arc::new(AtomicUsize::new(0)),
            delivery,
        })
    }
}
#[async_trait]
impl AuthenticatedTransports for ScriptedTransport {
    fn mcp_destination(&self, _: &ApprovedDestination) -> integration::IntegrationResult<()> {
        Err(integration::IntegrationError::Denied)
    }
    async fn discover_mcp(
        &self,
        _: &OwnerDiscoveryContext,
        _: TypedInjection,
        _: crate::application::services::mcp_client::McpPreparation,
    ) -> ResolutionResult<Box<dyn discovery::PreparedCatalog>> {
        Err(integration::IntegrationError::Denied.into())
    }
    fn http_url(&self, i: &AuthorizedInvocation) -> integration::IntegrationResult<url::Url> {
        let origin = HttpOrigin::parse(i.authorization.destination.as_str())
            .map_err(|_| integration::IntegrationError::Invalid)?;
        let request = HttpRequest::parse(i.arguments.value())
            .map_err(|_| integration::IntegrationError::Invalid)?;
        crate::adapters::guarded_http::request_url(&origin, &request)
            .map_err(|_| integration::IntegrationError::Invalid)
    }
    async fn prepare_http(
        &self,
        _: &AuthorizedInvocation,
        _: &ExecutionContext,
        injection: TypedInjection,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
        self.preparations.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            injection.value.expose_secret(),
            self.expected.expose_secret()
        );
        match injection.method {
            InjectionMethod::Bearer => assert_eq!(self.method, "bearer"),
            InjectionMethod::ApiKeyHeader(header) => assert_eq!(self.method, header),
        }
        match &self.delivery {
            Delivery::PreparationUnavailable => {
                return Err(integration::IntegrationError::Unavailable.into());
            }
            Delivery::Wait { started, release } => {
                started.notify_one();
                release.notified().await;
            }
            _ => {}
        }
        Ok(Box::new(ScriptedCall {
            calls: self.calls.clone(),
            closes: self.closes.clone(),
            delivery: self.delivery.clone(),
        }))
    }
    async fn prepare_mcp(
        &self,
        _: &AuthorizedInvocation,
        _: &ExecutionContext,
        _: TypedInjection,
        _: crate::application::services::mcp_client::McpPreparation,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
        Err(integration::IntegrationError::Invalid.into())
    }
}
struct ScriptedCall {
    calls: Arc<AtomicUsize>,
    closes: Arc<AtomicUsize>,
    delivery: Delivery,
}
#[async_trait]
impl PreparedBusinessCall for ScriptedCall {
    async fn call(&self) -> integration::IntegrationResult<ProviderData> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if matches!(self.delivery, Delivery::Uncertain) {
            return Err(integration::IntegrationError::Unavailable);
        }
        ProviderData::from_redacted(json!({"status":200,"headers":{},"body":{"accepted":true}}))
    }
    async fn close(&self) -> integration::IntegrationResult<()> {
        self.closes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
pub(super) fn gateway(
    vault: &PostgresVault,
    transport: Arc<ScriptedTransport>,
) -> Arc<dyn ExecutionGateway> {
    Arc::new(AuthenticatedExecution {
        registry: vault.registry.clone(),
        credentials: Arc::new(vault.clone()),
        transports: transport,
    })
}
pub(super) fn runtime(
    setup: &Setup,
    vault: &PostgresVault,
    transport: Arc<ScriptedTransport>,
) -> HttpRuntime {
    HttpRuntime::with_integrations(
        setup.fixture.persistence.clone(),
        HttpDestinationPolicy::default(),
        lease(),
        None,
        gateway(vault, transport),
    )
}
async fn fixture(app: &str) -> (Setup, PostgresVault, integration::ConnectedAccount) {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let vault = PostgresVault::new(setup.fixture.persistence.clone());
    let mut input = write("execution", "execution-private-sentinel");
    input.provider = integration::ProviderId::parse(app).unwrap();
    input.config = integration::AuthConfigId::parse(app).unwrap();
    let account = vault
        .create(&owner_for_user(setup.fixture.target.actor.user_id()), input)
        .await
        .unwrap();
    Box::pin(attach_source(
        &setup,
        &vault,
        &account,
        integration::AppId::parse(app).unwrap(),
    ))
    .await;
    (setup, vault, account)
}

#[tokio::test]
async fn integration_gateway_real_host_static_bearer_and_api_key_share_native_receipts() {
    for (app, method) in [("static-bearer", "bearer"), ("static-api-key", "X-API-Key")] {
        let (setup, vault, _) = Box::pin(fixture(app)).await;
        let transport =
            ScriptedTransport::new("execution-private-sentinel", method, Delivery::Accepted);
        let runtime = runtime(&setup, &vault, transport.clone());
        let result = Box::pin(runtime.run(&setup.kind, &setup.claim))
            .await
            .completed_result()
            .unwrap();
        assert_eq!(result["body"]["accepted"], true);
        let counts = action_counts(&setup.fixture).await;
        assert_eq!(
            (counts.dispatches, counts.entries, counts.receipts),
            (1, 1, 1)
        );
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert_eq!(transport.closes.load(Ordering::SeqCst), 1);
        let receipt: Value =
            sqlx::query_scalar("SELECT result FROM workflow_action_receipts WHERE company_id=$1")
                .bind(setup.fixture.target.company.as_uuid())
                .fetch_one(setup.fixture.persistence.pool())
                .await
                .unwrap();
        assert!(!receipt.to_string().contains("execution-private-sentinel"));
        Box::pin(runtime.run(&setup.kind, &setup.claim))
            .await
            .completed_result()
            .unwrap();
        assert_eq!(
            transport.preparations.load(Ordering::SeqCst),
            1,
            "receipt replay never reads a credential"
        );
    }
}

#[tokio::test]
async fn vault_workflow_http_preparation_failure_leaves_intent_without_remote_effect() {
    let (setup, vault, _) = Box::pin(fixture("static-bearer")).await;
    let transport = ScriptedTransport::new(
        "execution-private-sentinel",
        "bearer",
        Delivery::PreparationUnavailable,
    );
    let failure =
        Box::pin(runtime(&setup, &vault, transport.clone()).run(&setup.kind, &setup.claim))
            .await
            .completed_result()
            .unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "http.integration_retry");
    let counts = action_counts(&setup.fixture).await;
    assert_eq!(
        (counts.dispatches, counts.entries, counts.receipts),
        (0, 0, 0)
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn vault_workflow_http_uncertain_send_is_not_dispatched_twice() {
    let (setup, vault, _) = Box::pin(fixture("static-bearer")).await;
    let transport =
        ScriptedTransport::new("execution-private-sentinel", "bearer", Delivery::Uncertain);
    let runtime = runtime(&setup, &vault, transport.clone());
    for _ in 0..2 {
        assert!(
            Box::pin(runtime.run(&setup.kind, &setup.claim))
                .await
                .completed_result()
                .is_err()
        );
    }
    let counts = action_counts(&setup.fixture).await;
    assert_eq!(
        (counts.dispatches, counts.entries, counts.receipts),
        (1, 1, 0)
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.preparations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn vault_workflow_http_competing_claimants_have_one_business_entry() {
    let (setup, vault, _) = Box::pin(fixture("static-bearer")).await;
    let transport =
        ScriptedTransport::new("execution-private-sentinel", "bearer", Delivery::Accepted);
    let runtime = runtime(&setup, &vault, transport.clone());
    let (a, b) = tokio::join!(
        Box::pin(runtime.run(&setup.kind, &setup.claim)),
        Box::pin(runtime.run(&setup.kind, &setup.claim))
    );
    assert!(a.completed_result().is_ok() || b.completed_result().is_ok());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let counts = action_counts(&setup.fixture).await;
    assert_eq!(
        (counts.dispatches, counts.entries, counts.receipts),
        (1, 1, 1)
    );
}

#[tokio::test]
async fn vault_workflow_http_ownership_lost_during_preparation_sends_nothing() {
    let (setup, vault, _) = Box::pin(fixture("static-bearer")).await;
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let transport = ScriptedTransport::new(
        "execution-private-sentinel",
        "bearer",
        Delivery::Wait {
            started: started.clone(),
            release: release.clone(),
        },
    );
    let runtime = runtime(&setup, &vault, transport.clone());
    let (result, ()) = tokio::join!(Box::pin(runtime.run(&setup.kind, &setup.claim)), async {
        started.notified().await;
        sqlx::query("UPDATE background_tasks SET worker_id=$2 WHERE id=$1")
            .bind(setup.claim.fence.scope.job.0)
            .bind(Uuid::new_v4())
            .execute(setup.fixture.persistence.pool())
            .await
            .unwrap();
        release.notify_one();
    });
    assert!(result.completed_result().is_err());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.closes.load(Ordering::SeqCst), 1);
    let counts = action_counts(&setup.fixture).await;
    assert_eq!(
        (counts.dispatches, counts.entries, counts.receipts),
        (0, 0, 0)
    );
}
