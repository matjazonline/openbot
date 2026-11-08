use super::*;
use crate::{
    adapters::{
        integrations::{
            execution::IntegrationTransports, owner::owner_for_user,
            workflow_dispatch::IntegrationActionDispatch,
        },
        persistence::{
            integrations::{PostgresVault, renewal_tests::server::TokenServer},
            workflow::{action_dispatch::PostgresActionDispatch, mcp::McpActionAuthority},
        },
        workflow_http::{HttpDestinationPolicy, NoLocal},
    },
    application::{
        integrations::{
            execute::AuthenticatedExecution,
            management::{AccountRepository, AccountWrite},
            registry::CodeOwnedRegistry,
            *,
        },
        services::mcp_client::{McpCallResult, McpClient, McpSession},
    },
    domain::integrations::*,
};
use async_trait::async_trait;
use secrecy::ExposeSecret;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Notify, Semaphore};

pub(super) const SECRET: &str = "gateway-mcp-private-sentinel";
pub(super) struct Stage {
    entered: AtomicUsize,
    started: Notify,
    release: Semaphore,
}
impl Stage {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: AtomicUsize::new(0),
            started: Notify::new(),
            release: Semaphore::new(0),
        })
    }
    async fn wait(&self) {
        self.entered.fetch_add(1, Ordering::SeqCst);
        self.started.notify_waiters();
        self.release.acquire().await.unwrap().forget();
    }
    pub(super) async fn observed(&self, count: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let ready = self.started.notified();
                if self.entered.load(Ordering::SeqCst) >= count {
                    break;
                }
                ready.await;
            }
        })
        .await
        .expect("scripted transport reached the selected stage");
    }
    pub(super) fn release(&self, count: usize) {
        self.release.add_permits(count);
    }
}
#[derive(Clone)]
pub(super) enum Delivery {
    Accepted,
    Uncertain,
    FirstDiscoveryWait(Arc<Stage>),
    EveryDiscoveryWait(Arc<Stage>),
    CallWait(Arc<Stage>),
    CredentialInCatalog,
}
#[derive(Default)]
struct Counters {
    opens: AtomicUsize,
    calls: AtomicUsize,
    closes: AtomicUsize,
    active: AtomicUsize,
    active_calls: AtomicUsize,
}
pub(super) struct ScriptedClient {
    counters: Arc<Counters>,
    expected: Vec<SecretString>,
    delivery: Delivery,
}
impl ScriptedClient {
    fn new(expected: &[&str], delivery: Delivery) -> Arc<Self> {
        Arc::new(Self {
            counters: Arc::new(Counters::default()),
            expected: expected
                .iter()
                .map(|s| SecretString::from((*s).to_owned()))
                .collect(),
            delivery,
        })
    }
    pub(super) fn counts(&self) -> (usize, usize, usize) {
        (
            self.counters.opens.load(Ordering::SeqCst),
            self.counters.calls.load(Ordering::SeqCst),
            self.counters.closes.load(Ordering::SeqCst),
        )
    }
}
#[async_trait]
impl McpClient for ScriptedClient {
    async fn connect_prepared(
        &self,
        endpoint: &crate::entities::mcp::McpEndpoint,
        token: SecretString,
        work: crate::application::services::mcp_client::McpPreparation,
    ) -> crate::application::services::mcp_client::McpPreparationResult<Box<dyn McpSession>> {
        use crate::application::services::mcp_client::McpPreparationFailure;
        work.check().await?;
        let session = self
            .connect(endpoint, Some(token))
            .await
            .map_err(|_| McpPreparationFailure::Unavailable)?;
        if let Err(error) = work.check().await {
            let _ = session.close().await;
            return Err(error);
        }
        Ok(session)
    }
    async fn shutdown(&self) -> AppResult<()> {
        assert_eq!(self.counters.active.load(Ordering::SeqCst), 0);
        Ok(())
    }
    fn validate_endpoint(&self, endpoint: &crate::entities::mcp::McpEndpoint) -> AppResult<()> {
        if endpoint.as_str() != "https://mcp.example.test/rpc" {
            return Err(AppError::Forbidden("Unregistered scripted endpoint".into()));
        }
        Ok(())
    }
    async fn connect(
        &self,
        endpoint: &crate::entities::mcp::McpEndpoint,
        token: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>> {
        self.validate_endpoint(endpoint)?;
        let index = self.counters.opens.fetch_add(1, Ordering::SeqCst);
        let token = token.expect("private module supplied a bearer");
        assert!(
            self.expected
                .get(index)
                .is_some_and(|expected| expected.expose_secret() == token.expose_secret())
        );
        self.counters.active.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Session {
            counters: self.counters.clone(),
            token,
            delivery: self.delivery.clone(),
            index,
            closed: false,
        }))
    }
}
struct Session {
    counters: Arc<Counters>,
    token: SecretString,
    delivery: Delivery,
    index: usize,
    closed: bool,
}
struct CallGuard(Arc<Counters>);
impl Drop for CallGuard {
    fn drop(&mut self) {
        self.0.active_calls.fetch_sub(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl McpSession for Session {
    async fn discover(&self) -> AppResult<Vec<McpDiscoveredTool>> {
        match &self.delivery {
            Delivery::FirstDiscoveryWait(stage) if self.index == 0 => stage.wait().await,
            Delivery::EveryDiscoveryWait(stage) => stage.wait().await,
            _ => {}
        }
        Ok(vec![McpDiscoveredTool {
            name: TOOL.to_string().try_into().unwrap(),
            description: if matches!(self.delivery, Delivery::CredentialInCatalog) {
                self.token.expose_secret().to_owned()
            } else {
                "Bounded scripted catalog".into()
            },
            input_schema: input_schema(),
            output_schema: Some(output_schema()),
        }])
    }
    async fn call(
        &self,
        name: &crate::entities::mcp::McpToolName,
        arguments: Value,
    ) -> AppResult<McpCallResult> {
        assert_eq!(name.as_str(), TOOL);
        assert!(arguments.get("customer_id").is_some());
        self.counters.calls.fetch_add(1, Ordering::SeqCst);
        self.counters.active_calls.fetch_add(1, Ordering::SeqCst);
        let _guard = CallGuard(self.counters.clone());
        if let Delivery::CallWait(stage) = &self.delivery {
            stage.wait().await;
        }
        if matches!(self.delivery, Delivery::Uncertain) {
            return Err(AppError::ServiceUnavailable(
                "Scripted uncertain MCP call".into(),
            ));
        }
        Ok(McpCallResult {
            content: vec![json!({"type":"text","text":self.token.expose_secret()})],
            structured_content: Some(json!({"name":self.token.expose_secret(),"tier":1})),
            is_error: false,
        })
    }
    async fn close(mut self: Box<Self>) -> AppResult<()> {
        assert_eq!(
            self.counters.active_calls.load(Ordering::SeqCst),
            0,
            "actual call future was cancelled before close"
        );
        self.counters.closes.fetch_add(1, Ordering::SeqCst);
        self.counters.active.fetch_sub(1, Ordering::SeqCst);
        self.closed = true;
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if !self.closed {
            self.counters.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

type Dispatcher = IntegrationActionDispatch<McpActionAuthority, NoLocal>;
pub(super) struct Harness {
    pub setup: McpSetup,
    pub vault: PostgresVault,
    pub account: ConnectedAccount,
    pub request: ActionDispatchRequest,
    pub client: Arc<ScriptedClient>,
    pub dispatch: Dispatcher,
}
pub(super) fn gateway(
    vault: &PostgresVault,
    client: Arc<dyn McpClient>,
) -> Arc<dyn ExecutionGateway> {
    Arc::new(AuthenticatedExecution {
        registry: vault.registry.clone(),
        credentials: Arc::new(vault.clone()),
        transports: Arc::new(IntegrationTransports {
            http: HttpDestinationPolicy::default(),
            mcp: client,
        }),
    })
}
async fn finish(
    original: &Fixture,
    vault: PostgresVault,
    account: ConnectedAccount,
    app: AppId,
    client: Arc<ScriptedClient>,
) -> Harness {
    let setup = Box::pin(configured_setup(original, &vault, &account, app)).await;
    let claim = claimed(&setup).await;
    let p = &setup.fixture.persistence;
    let request = p.mcp_step_request(&claim, &kind()).await.unwrap();
    let intent = Box::pin(ActionService::new(p.clone()).prepare_step(request))
        .await
        .unwrap();
    let request = ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: claim.fence,
        lease: lease(),
    };
    let native = PostgresActionDispatch::new(p.clone(), McpActionAuthority, NoLocal);
    let dispatch =
        IntegrationActionDispatch::new(p.clone(), native, gateway(&vault, client.clone()));
    Harness {
        setup,
        vault,
        account,
        request,
        client,
        dispatch,
    }
}
async fn original() -> Fixture {
    let mut f = Fixture::new().await;
    f.persistence = PostgresPersistence::with_credential_cipher(
        f.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    f
}
pub(super) async fn fixture(delivery: Delivery) -> Harness {
    let original = Box::pin(original()).await;
    let vault = PostgresVault::new(original.persistence.clone());
    let account = vault
        .create(
            &owner_for_user(original.target.actor.user_id()),
            AccountWrite {
                key: AccountKey::parse("mcp-execution").unwrap(),
                provider: ProviderId::parse("static-bearer").unwrap(),
                config: AuthConfigId::parse("static-bearer").unwrap(),
                method: AuthMethod::StaticSecret,
                schema: CredentialSchema::parse("static-v1").unwrap(),
                identity: None,
                metadata: AccountMetadata::new("MCP execution".into(), None).unwrap(),
                payload: SecretString::from(SECRET),
            },
        )
        .await
        .unwrap();
    Box::pin(finish(
        &original,
        vault,
        account,
        AppId::parse("static-bearer").unwrap(),
        ScriptedClient::new(&[SECRET, SECRET], delivery),
    ))
    .await
}
pub(super) fn google_payload(generation: i64, access: &str) -> SecretString {
    SecretString::from(json!({"subject":"mcp-generation-fixture","client":"google-client-fixture","mailbox":"fixture@gmail.com",
        "scopes":["openid","email","https://www.googleapis.com/auth/gmail.readonly","https://www.googleapis.com/auth/gmail.send"],
        "refresh_token":"refresh-sentinel","access_token":access,"expires_at":chrono::Utc::now()+chrono::Duration::hours(1),"generation":generation,"refresh_retry_at":null}).to_string())
}
pub(super) async fn managed_fixture(delivery: Delivery) -> (Harness, TokenServer) {
    let original = Box::pin(original()).await;
    let server = TokenServer::new().await;
    let mut entry = server.google_entry();
    // Synthetic Google-shaped credentials and a code-owned scripted endpoint; production
    // registers no Google MCP server and never inherits this test-only policy.
    entry.app.id = AppId::parse("google-session-fixture").unwrap();
    entry.app.destinations = vec![
        crate::domain::integrations::authority::ApprovedDestination::parse(
            "https://mcp.example.test/rpc",
        )
        .unwrap(),
    ];
    entry.app.transport =
        crate::application::integrations::destinations::AppTransportPolicy::McpEndpoint;
    let payload = google_payload(1, "old-mcp-access-sentinel");
    let input = AccountWrite {
        key: AccountKey::parse("managed-mcp").unwrap(),
        provider: entry.provider.id.clone(),
        config: entry.config.id.clone(),
        method: entry.provider.method,
        schema: entry.provider.schema.clone(),
        identity: entry.adapter.validate(&payload).unwrap(),
        metadata: AccountMetadata::new("Managed MCP fixture".into(), None).unwrap(),
        payload,
    };
    let app = entry.app.id.clone();
    let mut registry = CodeOwnedRegistry::default();
    registry.register(entry).unwrap();
    let vault = PostgresVault::with_registry(original.persistence.clone(), Arc::new(registry));
    let account = vault
        .create(&owner_for_user(original.target.actor.user_id()), input)
        .await
        .unwrap();
    (
        Box::pin(finish(
            &original,
            vault,
            account,
            app,
            ScriptedClient::new(
                &["old-mcp-access-sentinel", "new-mcp-access-sentinel"],
                delivery,
            ),
        ))
        .await,
        server,
    )
}
pub(super) struct UnreachableProvider;
#[async_trait]
impl RemoteAction for UnreachableProvider {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, _: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        panic!("integration bytes never enter the host provider")
    }
}
