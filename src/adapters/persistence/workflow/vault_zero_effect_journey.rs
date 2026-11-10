use super::*;
use crate::application::use_cases::integration_binding::*;
use crate::application::workflow::agent::{AgentPreparation, AgentToolDispatch};
use crate::{
    application::integrations::{
        private::CredentialStore,
        renewal::{CredentialResolver, ExecutionCredentials, ResolutionResult, ResolvedCredential},
    },
    application::workflow::{sample::SampleStatus, sample_capture::SampleDefinitionCapture},
    domain::integrations::authority::ApprovedDestination,
};
use chrono::{Duration as ChronoDuration, Utc};
use std::sync::atomic::AtomicUsize;

struct CountedCredentials {
    vault: PostgresVault,
    resolutions: Arc<AtomicUsize>,
}
#[async_trait]
impl CredentialResolver for CountedCredentials {
    async fn resolve(
        &self,
        i: &AuthorizedInvocation,
        c: &ExecutionContext,
    ) -> ResolutionResult<ResolvedCredential> {
        self.resolutions.fetch_add(1, Ordering::SeqCst);
        self.vault.resolve(i, c).await
    }
}
#[async_trait]
impl ExecutionCredentials for CountedCredentials {
    async fn resolve_configuration(
        &self,
        c: &OwnerDiscoveryContext,
    ) -> ResolutionResult<ResolvedCredential> {
        self.resolutions.fetch_add(1, Ordering::SeqCst);
        self.vault.resolve_configuration(c).await
    }
    async fn check_configuration(
        &self,
        c: &OwnerDiscoveryContext,
        g: CredentialGeneration,
    ) -> IntegrationResult<()> {
        self.vault.check_configuration(c, g).await
    }
    async fn check_generation(
        &self,
        i: &AuthorizedInvocation,
        c: &ExecutionContext,
        g: CredentialGeneration,
    ) -> IntegrationResult<()> {
        self.vault.check_generation(i, c, g).await
    }
}
async fn private_account(
    setup: &AgentSetup,
    token: &TokenServer,
) -> (PostgresVault, ConnectedAccount) {
    let mut registry = static_registry();
    registry.register(token.google_entry()).unwrap();
    let vault = PostgresVault::with_registry(setup.fixture.persistence.clone(), Arc::new(registry));
    let entry = vault.registry.get(&AppId::parse("gmail").unwrap()).unwrap();
    let payload=SecretString::from(json!({"subject":"zero-effect-mailbox","client":"google-client-fixture","mailbox":"vault@gmail.com",
        "scopes":["openid","email","https://www.googleapis.com/auth/gmail.readonly","https://www.googleapis.com/auth/gmail.send"],
        "refresh_token":"refresh-sentinel","access_token":"old-access-sentinel","expires_at":Utc::now()-ChronoDuration::seconds(1),"generation":1,"refresh_retry_at":null}).to_string());
    let account = vault
        .create(
            &owner_for_user(setup.fixture.target.actor.user_id()),
            crate::application::integrations::management::AccountWrite {
                key: AccountKey::parse("zero-effect-private").unwrap(),
                provider: entry.provider.id.clone(),
                config: entry.config.id.clone(),
                method: entry.provider.method,
                schema: entry.provider.schema.clone(),
                identity: entry.adapter.validate(&payload).unwrap(),
                metadata: AccountMetadata::new("Private mailbox".into(), None).unwrap(),
                payload,
            },
        )
        .await
        .unwrap();
    let bindings =
        crate::adapters::persistence::integration_bindings::PostgresIntegrationBindings {
            persistence: setup.fixture.persistence.clone(),
            vault: vault.clone(),
        };
    let proposal = bindings
        .propose(
            setup.fixture.target.actor.user_id(),
            crate::application::use_cases::integration_binding::ProposeBinding {
                company: setup.fixture.target.company.as_uuid(),
                connection: setup.connection.id,
                kind: crate::application::use_cases::integration_binding::BindingKind::Http,
                app: AppId::parse("gmail").unwrap(),
                expected_revision: setup.connection.revision,
            },
        )
        .await
        .unwrap();
    bindings
        .confirm(
            VerifiedOwner::from_verified_client(account.owner.clone()),
            proposal.id,
            account.id,
            account.authorization_revision,
        )
        .await
        .unwrap();
    (vault, account)
}
fn nested_tools(
    setup: &AgentSetup,
    http: Arc<HttpRuntime>,
    gateway: Arc<dyn ExecutionGateway>,
) -> AgentActionTools {
    let p = &setup.fixture.persistence;
    let mcp = McpRuntime::with_integrations(
        p.clone(),
        Arc::new(HttpMcpClient::new(EndpointPolicy::default())),
        lease(),
        None,
        gateway,
    );
    let message = MessageRuntime::new(p.clone(), composer(p), "agents.test".into(), lease());
    AgentActionTools::new(http, Arc::new(mcp), Arc::new(message))
        .with_workflow_tools(Arc::new(p.clone()))
}
fn nested_runtime(
    setup: &AgentSetup,
    http: Arc<HttpRuntime>,
    gateway: Arc<dyn ExecutionGateway>,
) -> WorkflowAgentRuntime<PostgresPersistence, AgentActionTools> {
    WorkflowAgentRuntime::new(
        ProviderRegistry::standard().unwrap(),
        setup.fixture.persistence.clone(),
        nested_tools(setup, http, gateway),
        Some(notices(&setup.fixture.persistence)),
        lease(),
    )
}
async fn sealed_sample(setup: &AgentSetup) -> crate::application::workflow::sample::PreparedSample {
    let p = &setup.fixture.persistence;
    let draft = p
        .draft(setup.fixture.target.company, setup.fixture.target.workflow)
        .await
        .unwrap()
        .unwrap();
    let capture = crate::adapters::persistence::workflow::SampleCapture {
        persistence: p.clone(),
        http: http_directory(p, &setup.connection).selections,
        mcp: vec![],
        refused_kinds: &[],
    };
    let request=serde_json::from_value(json!({"input":{},"params":{},"agents":[],"scripts":[{
        "workflow_id":setup.fixture.target.workflow,"step":"start","responses":[{"output":"Sample draft only"}]}]})).unwrap();
    Box::pin(capture.capture(setup.fixture.target, draft.draft.revision(), request))
        .await
        .unwrap()
}
#[tokio::test]
async fn vault_google_e2e_direct_nested_model_and_sealed_sample_never_resolve_private_credentials()
{
    let setup = Box::pin(agent_setup("https://gmail.googleapis.com")).await;
    let launch = Box::pin(
        setup
            .fixture
            .persistence
            .prepare_agent(&setup.claim, lease()),
    )
    .await
    .unwrap()
    .expect_ready();
    let sample = Box::pin(sealed_sample(&setup)).await;
    let token = TokenServer::new().await;
    let (vault, account) = Box::pin(private_account(&setup, &token)).await;
    let peer = BusinessPeer::new(
        CredentialExpectation::Bearer(SecretString::from("renewed-access-sentinel")),
        json!({"accepted":true}),
        None,
    )
    .await;
    let resolutions = Arc::new(AtomicUsize::new(0));
    let gateway = Arc::new(AuthenticatedExecution {
        registry: vault.registry.clone(),
        credentials: Arc::new(CountedCredentials {
            vault: vault.clone(),
            resolutions: resolutions.clone(),
        }),
        transports: peer
            .transports(ApprovedDestination::parse("https://gmail.googleapis.com").unwrap()),
    });
    let http = Arc::new(HttpRuntime::with_integrations(
        vault.persistence.clone(),
        HttpDestinationPolicy::default(),
        lease(),
        None,
        gateway.clone(),
    ));
    let arguments = json!({"method":"GET","path":"/gmail/v1/users/me/threads/zero-effect"});
    assert_direct_tool_denied(&setup, &http, arguments.clone()).await;
    let tool = launch
        .prepared
        .tool("registered__request")
        .expect("genuine published tool");
    let tools = nested_tools(&setup, http.clone(), gateway.clone());
    assert!(
        Box::pin(tools.dispatch(
            setup.claim.fence,
            tool,
            ModelToolCallId::parse(Uuid::new_v4().to_string()).unwrap(),
            arguments
        ))
        .await
        .is_err()
    );
    let mut llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = nested_runtime(&setup, http, gateway);
    let outcome =
        Box::pin(runtime.execute(&WorkflowStepKind("agent.run".into()), &setup.claim)).await;
    let failure = outcome.completed_result().unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "agent.rejected");
    assert_eq!(
        failure.failure.class(),
        crate::domain::workflow::FailureClass::Terminal
    );
    assert!(
        llm.observed().is_empty(),
        "current vault-backed generic tools are rejected before model launch"
    );
    assert_eq!(llm.finish().await.unwrap(), 0);
    let report = crate::application::workflow::sample::execute(sample).unwrap();
    assert_eq!(report.status, SampleStatus::Completed);
    assert_eq!(report.output, Some(json!("Sample draft only")));
    assert!(report.effects.is_empty());
    assert_no_private_effects(&setup, &vault, &account, &peer, &token, &resolutions).await;
    peer.close().await;
}

async fn assert_direct_tool_denied(setup: &AgentSetup, http: &HttpRuntime, arguments: Value) {
    let direct = HttpToolCall {
        origin: crate::application::agent::ToolCallOrigin::Workflow,
        connection: setup.connection.id,
        operation: TypeName::parse("registered.request").unwrap(),
        arguments,
        call: ModelToolCallId::parse("direct-vault-refused").unwrap(),
    };
    assert!(
        http.call_tool(setup.claim.fence, direct, &CancellationToken::new())
            .await
            .is_err()
    );
}

async fn assert_no_private_effects(
    setup: &AgentSetup,
    vault: &PostgresVault,
    account: &ConnectedAccount,
    peer: &BusinessPeer,
    token: &TokenServer,
    resolutions: &AtomicUsize,
) {
    assert_eq!(resolutions.load(Ordering::SeqCst), 0);
    assert_eq!(token.calls(), 0);
    assert_eq!(peer.calls(), 0);
    assert!(peer.requests().is_empty());
    assert_eq!(
        vault
            .read_usable(&account.owner, account.id)
            .await
            .unwrap()
            .versions
            .generation
            .get(),
        1
    );
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}
