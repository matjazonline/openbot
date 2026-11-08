use super::*;
use crate::adapters::{
    mcp::{
        pinned_fixture::PinnedFixture, preparation_test_server::Server, preparation_tests::client,
    },
    persistence::workflow::tests::mcp_tests::support::{
        self as support, renewal::configured_setup,
    },
    workflow_mcp::McpRuntime,
};
use crate::application::{
    integrations::destinations::AppTransportPolicy, services::mcp_client::McpClient,
};

#[tokio::test]
async fn integration_portability_bearer_google_and_strict_oauth_use_real_native_mcp_without_workflow_branches()
 {
    for case in [Case::Bearer, Case::Google, Case::Workspace] {
        Box::pin(native_mcp(case)).await;
    }
}
async fn native_mcp(case: Case) {
    let setup = Box::pin(native_setup(case)).await;
    let token = TokenServer::new().await;
    let vault =
        PostgresVault::with_registry(setup.fixture.persistence.clone(), case.registry(&token));
    let account = Box::pin(account_for(&setup, &vault, case)).await;
    let base = vault.registry.get(&case.app()).unwrap();
    let mut app = base.app.clone();
    app.id = AppId::parse("native-mcp-extension-fixture").unwrap();
    app.transport = AppTransportPolicy::McpEndpoint;
    app.destinations = vec![ApprovedDestination::parse("https://mcp.example.test/rpc").unwrap()];
    let mut registry = CodeOwnedRegistry::default();
    registry
        .register(Registration {
            provider: base.provider.clone(),
            config: base.config.clone(),
            app: app.clone(),
            adapter: base.adapter.clone(),
            authorization: base.authorization.clone(),
            renewal: base.renewal.clone(),
        })
        .unwrap();
    let vault = PostgresVault::with_registry(vault.persistence.clone(), Arc::new(registry));
    let mcp = Box::pin(configured_setup(&setup.fixture, &vault, &account, app.id)).await;
    let current = mcp.current().await;
    let server = Server::business(
        SecretString::from(case.access()),
        current.discovered_tools,
        json!({"content":[],"structuredContent":{"name":case.access(),"tier":1},"isError":false}),
    )
    .await;
    let native = Arc::new(PinnedFixture {
        native: client(&server),
        metadata: current.endpoint.as_str().to_owned().try_into().unwrap(),
        actual: server.url.clone().try_into().unwrap(),
    });
    let transports = Arc::new(
        crate::adapters::integrations::execution::IntegrationTransports {
            http: HttpDestinationPolicy::default(),
            mcp: native.clone(),
        },
    );
    let gateway = Arc::new(AuthenticatedExecution {
        registry: vault.registry.clone(),
        credentials: Arc::new(vault.clone()),
        transports,
    });
    let runtime = McpRuntime::with_integrations(
        vault.persistence.clone(),
        native.clone(),
        lease(),
        None,
        gateway,
    );
    let claim = Box::pin(support::integration_claim(&mcp)).await;
    let kind = WorkflowStepKind("mcp.call".into());
    for _ in 0..2 {
        let result = Box::pin(runtime.run(&kind, &claim))
            .await
            .completed_result()
            .unwrap();
        assert!(!result.to_string().contains(case.access()));
    }
    assert_eq!(server.counts().calls, 1);
    assert_eq!(server.counts().initialize, 1);
    assert_eq!(token.calls(), usize::from(!matches!(case, Case::Bearer)));
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 1
        }
    );
    native.shutdown().await.unwrap();
    server.close().await;
}
#[tokio::test]
async fn integration_portability_http_api_key_policy_denies_mcp_consent_before_any_remote_session()
{
    let case = Case::ApiKey;
    let setup = Box::pin(native_setup(case)).await;
    let token = TokenServer::new().await;
    let vault =
        PostgresVault::with_registry(setup.fixture.persistence.clone(), case.registry(&token));
    let (connection, _) = Box::pin(support::configured(
        &setup.fixture,
        "https://mcp.example.test/rpc",
    ))
    .await;
    let account = Box::pin(account_for(&setup, &vault, case)).await;
    let bindings = PostgresIntegrationBindings {
        persistence: setup.fixture.persistence.clone(),
        vault,
    };
    let proposal = bindings
        .propose(
            setup.fixture.target.actor.user_id(),
            ProposeBinding {
                company: setup.fixture.target.company.as_uuid(),
                connection: connection.id,
                kind: BindingKind::Mcp,
                app: case.app(),
                expected_revision: connection.revision,
            },
        )
        .await
        .unwrap();
    assert!(
        bindings
            .confirm(
                VerifiedOwner::from_verified_client(account.owner.clone()),
                proposal.id,
                account.id,
                account.authorization_revision
            )
            .await
            .is_err()
    );
    assert_eq!(token.calls(), 0);
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}
