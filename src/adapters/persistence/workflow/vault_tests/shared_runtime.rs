use super::*;
use crate::adapters::persistence::workflow::tests::{
    http_tests::vault_tests::renewal as renewal_support,
    mcp_tests::support::{self as mcp_support, McpSetup, renewal::configured_setup},
};
use crate::application::workflow::completion::WorkflowCompletion;
use crate::{
    adapters::{
        mcp::{
            pinned_fixture::PinnedFixture, preparation_test_server::Server,
            preparation_tests::client,
        },
        persistence::integrations::renewal_tests::business_peer,
        workflow_mcp::McpRuntime,
    },
    application::{
        integrations::destinations::AppTransportPolicy, services::mcp_client::McpClient,
    },
};

struct Shared {
    setup: Setup,
    vault: PostgresVault,
    account: ConnectedAccount,
    token: TokenServer,
    mcp: McpSetup,
}
impl Shared {
    async fn new() -> Self {
        let setup = Box::pin(native_setup(Case::Google)).await;
        sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
            .execute(setup.fixture.persistence.pool())
            .await
            .unwrap();
        let token = TokenServer::new().await;
        let mut registry = static_registry();
        registry.register(token.google_entry()).unwrap();
        let mut fixture = token.google_entry();
        fixture.app.id = AppId::parse("gmail-mcp-fixture").unwrap();
        fixture.app.capabilities = vec![Capability::parse("gmail.read").unwrap()];
        fixture.app.transport = AppTransportPolicy::McpEndpoint;
        fixture.app.destinations =
            vec![ApprovedDestination::parse("https://mcp.example.test/rpc").unwrap()];
        registry.register(fixture).unwrap();
        let vault =
            PostgresVault::with_registry(setup.fixture.persistence.clone(), Arc::new(registry));
        let account = Box::pin(account_for(&setup, &vault, Case::Google)).await;
        Box::pin(attach_source(&setup, &vault, &account, Case::Google.app())).await;
        let mcp = Box::pin(configured_setup(
            &setup.fixture,
            &vault,
            &account,
            AppId::parse("gmail-mcp-fixture").unwrap(),
        ))
        .await;
        Self {
            setup,
            vault,
            account,
            token,
            mcp,
        }
    }
}
async fn native_mcp(f: &Shared) -> (Server, Arc<PinnedFixture>) {
    let current = f.mcp.current().await;
    let server=Server::business(SecretString::from(Case::Google.access()),current.discovered_tools,
        json!({"content":[],"structuredContent":{"name":Case::Google.access(),"tier":1},"isError":false})).await;
    let native = client(&server);
    let client = Arc::new(PinnedFixture {
        native,
        metadata: current.endpoint.as_str().to_owned().try_into().unwrap(),
        actual: server.url.clone().try_into().unwrap(),
    });
    (server, client)
}
fn gateway(
    vault: &PostgresVault,
    http: &BusinessPeer,
    mcp: Arc<PinnedFixture>,
) -> Arc<dyn ExecutionGateway> {
    Arc::new(AuthenticatedExecution {
        registry: vault.registry.clone(),
        credentials: Arc::new(vault.clone()),
        transports: business_peer::with_mcp(http.transports(Case::Google.destination()), mcp),
    })
}

#[tokio::test]
async fn vault_google_e2e_two_pools_http_mcp_share_one_refresh_and_cache_without_sharing_a_session()
{
    use tracing::instrument::WithSubscriber;
    let capture =
        crate::adapters::persistence::integrations::observation_test_support::Capture::default();
    Box::pin(shared_case())
        .with_subscriber(capture.subscriber())
        .await;
    let log = capture.text();
    for event in ["cache_hit", "refresh_attempt", "refresh_outcome"] {
        assert!(log.contains(event));
    }
    for secret in [
        Case::Google.access(),
        "refresh-sentinel",
        "old-access-sentinel",
    ] {
        assert!(!log.contains(secret));
    }
}
async fn shared_case() {
    let f = Box::pin(Shared::new()).await;
    let independent = Box::pin(renewal_support::independent_service(&f.vault)).await;
    let http = BusinessPeer::new(
        CredentialExpectation::Bearer(SecretString::from(Case::Google.access())),
        json!({"accepted":true}),
        None,
    )
    .await;
    let (mcp, client) = Box::pin(native_mcp(&f)).await;
    let http_runtime = HttpRuntime::with_integrations(
        f.setup.fixture.persistence.clone(),
        HttpDestinationPolicy::default(),
        lease(),
        None,
        gateway(&f.vault, &http, client.clone()),
    );
    let mcp_runtime = McpRuntime::with_integrations(
        independent.vault.persistence.clone(),
        client.clone(),
        lease(),
        None,
        gateway(&independent.vault, &http, client.clone()),
    );
    let claim = Box::pin(mcp_support::integration_claim(&f.mcp)).await;
    let held = f.token.state.gate.clone().acquire_owned().await.unwrap();
    let mcp_kind = WorkflowStepKind("mcp.call".into());
    let (a, b) = tokio::join!(
        Box::pin(http_runtime.run(&f.setup.kind, &f.setup.claim)),
        async {
            // Establish HTTP as the real lock holder before starting the independent claimant.
            f.token.wait_calls(1).await;
            let (result, ()) = tokio::join!(Box::pin(mcp_runtime.run(&mcp_kind, &claim)), async {
                renewal_support::wait_blocked(&f.vault, &independent.application).await;
                assert_eq!(http.calls(), 0);
                assert_eq!(mcp.counts().initialize, 0);
                drop(held);
            });
            result
        }
    );
    let http_result = a.completed_result().unwrap();
    let result = b.completed_result().unwrap();
    assert!(!result.to_string().contains(Case::Google.access()));
    assert_eq!(f.token.calls(), 1);
    assert_eq!(http.calls(), 1);
    assert_eq!(mcp.counts().calls, 1);
    assert_eq!(
        action_counts(&f.setup.fixture).await,
        ActionCounts {
            dispatches: 2,
            entries: 2,
            receipts: 2
        }
    );
    settle(&f.setup.fixture.persistence, &f.setup.claim, http_result).await;
    settle(&f.mcp.fixture.persistence, &claim, result).await;
    Box::pin(assert_cached_run(&f, &http_runtime)).await;
    assert_eq!(f.token.calls(), 1);
    assert_eq!(http.calls(), 2);
    assert_eq!(mcp.counts().initialize, 1);
    let current = f
        .vault
        .read_usable(&f.account.owner, f.account.id)
        .await
        .unwrap();
    assert_eq!(
        current.versions.authorization,
        f.account.authorization_revision
    );
    assert_eq!(current.versions.generation.get(), 2);
    client.shutdown().await.unwrap();
    mcp.close().await;
    http.close().await;
    independent.vault.persistence.pool().close().await;
}
async fn settle(p: &PostgresPersistence, claim: &ClaimedWorkflow, output: Value) {
    assert!(
        p.complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: claim.fence,
            policy: lease(),
            output,
        })
        .await
        .unwrap()
        .is_some()
    );
}
async fn assert_cached_run(f: &Shared, runtime: &HttpRuntime) {
    let target = Box::pin(bound(
        &f.setup.fixture,
        &f.setup.connection,
        f.setup.version,
        Entrance::Http,
        RelatedAssociation::Company,
    ))
    .await;
    let claim = Box::pin(
        crate::adapters::persistence::workflow::tests::http_tests::support::claimed_with_key(
            &f.setup.fixture,
            target,
            RelatedAssociation::Company,
            "cache-native-run",
        ),
    )
    .await;
    let result = Box::pin(runtime.run(&f.setup.kind, &claim))
        .await
        .completed_result()
        .unwrap();
    settle(&f.setup.fixture.persistence, &claim, result).await;
    assert_eq!(
        action_counts(&f.setup.fixture).await,
        ActionCounts {
            dispatches: 3,
            entries: 3,
            receipts: 3
        }
    );
}
