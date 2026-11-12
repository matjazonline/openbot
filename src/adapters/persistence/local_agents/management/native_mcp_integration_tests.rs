//! Native broker + actual consent/credential gateway, with a counted loopback MCP peer.
use super::*;
use crate::adapters::persistence::local_agents::test_support::mcp_workflow::{approve, eventually};
use crate::adapters::persistence::workflow::native_mcp_remote_fixture as remote;
use crate::adapters::{
    integrations::{execution::IntegrationTransports, owner::owner_for_user},
    mcp::pinned_fixture::PinnedFixture,
    persistence::{
        credentials::CredentialCipher, integration_bindings::PostgresIntegrationBindings,
        integrations::PostgresVault,
    },
    workflow_http::HttpDestinationPolicy,
};
use crate::application::{
    integrations::{
        VerifiedOwner,
        execute::AuthenticatedExecution,
        management::{AccountRepository, AccountWrite},
    },
    services::mcp_client::McpClient,
    use_cases::integration_binding::*,
    workflow::{lease::*, mcp::McpActionPreparation, polling::*, worker::WorkflowWorker},
};
use crate::domain::integrations::*;
use secrecy::SecretString;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

const SECRET: &str = "native-integration-owned-credential";
const DESTINATION: &str = "https://mcp.example.test/rpc";

async fn setup() -> (McpSetup, PostgresVault, ConnectedAccount) {
    let mut s = Setup::new().await;
    s.f.persistence = Arc::new(PostgresPersistence::with_credential_cipher(
        s.f.database.pool.clone(),
        CredentialCipher::for_test_with_keys(&[(1, [42; 32]), (2, [23; 32])], 1),
    ));
    let vault = PostgresVault::new(s.f.persistence.as_ref().clone());
    let account = vault
        .create(
            &owner_for_user(s.f.owner),
            AccountWrite {
                key: AccountKey::parse("native-mcp-integration").unwrap(),
                provider: ProviderId::parse("static-bearer").unwrap(),
                config: AuthConfigId::parse("static-bearer").unwrap(),
                method: AuthMethod::StaticSecret,
                schema: CredentialSchema::parse("static-v1").unwrap(),
                identity: None,
                metadata: AccountMetadata::new("Native MCP credential".into(), None).unwrap(),
                payload: SecretString::from(SECRET),
            },
        )
        .await
        .unwrap();
    let source = crate::adapters::persistence::local_agents::test_support::native_mcp_binding(
        s.f.persistence.as_ref(),
        s.f.company,
        s.f.owner,
        DESTINATION,
    )
    .await;
    let AgentBindingSource::Mcp { connection, .. } = source.source else {
        unreachable!()
    };
    let current =
        s.f.persistence
            .list_mcp_connections(s.f.company)
            .await
            .unwrap()
            .remove(0);
    let bindings = PostgresIntegrationBindings {
        persistence: s.f.persistence.as_ref().clone(),
        vault: vault.clone(),
    };
    let proposal = bindings
        .propose(
            s.f.owner,
            ProposeBinding {
                company: s.f.company,
                connection,
                kind: BindingKind::Mcp,
                app: AppId::parse("static-bearer").unwrap(),
                expected_revision: current.revision,
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
    let s = prepare_agent_with_source(s, source).await;
    install_claude(&s).await;
    s.f.cases.save_profile(s.f.owner, &s.profile).await.unwrap();
    native_tests::allow_connection(&s).await;
    (Box::pin(McpSetup::from_setup(s)).await, vault, account)
}

async fn worker(
    f: Arc<PostgresPersistence>,
    engine: crate::infra::workflow::WorkflowEngine,
    run: crate::domain::workflow::RunId,
    cancel: CancellationToken,
) {
    let worker = WorkflowWorker {
        port: f.as_ref(),
        handler: engine.handler(),
        notices: None,
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: live_tests::lease(),
        poll: PollPolicy::new(Duration::from_millis(50), 16).unwrap(),
        active_jobs: None,
        monitoring: None,
    };
    while !cancel.is_cancelled() {
        let page = f.poll_work(None, 16).await.unwrap();
        for candidate in page
            .candidates
            .into_iter()
            .filter(|candidate| candidate.scope.run == run)
        {
            worker.process(candidate, &cancel).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn execute(
    f: &McpSetup,
    vault: PostgresVault,
    account: &ConnectedAccount,
    remote: &remote::McpServer,
) -> NativeMcpResponse {
    let p = &f.s.f.persistence;
    let rotating = PostgresPersistence::with_credential_cipher(
        f.s.f.database.pool.clone(),
        CredentialCipher::for_test_with_keys(&[(1, [42; 32]), (2, [23; 32])], 2),
    );
    assert!(rotating.rotate_credentials().await.unwrap().complete);
    let current = vault.get(&account.owner, account.id).await.unwrap();
    assert_eq!(
        current.authorization_revision,
        account.authorization_revision
    );
    let client = Arc::new(PinnedFixture {
        native: remote::guarded_client(&remote.url),
        metadata: DESTINATION.to_owned().try_into().unwrap(),
        actual: remote.url.clone().try_into().unwrap(),
    });
    let gateway = Arc::new(AuthenticatedExecution {
        registry: vault.registry.clone(),
        credentials: Arc::new(vault),
        transports: Arc::new(IntegrationTransports {
            http: HttpDestinationPolicy::default(),
            mcp: client.clone(),
        }),
    });
    let engine = crate::infra::workflow::WorkflowEngine::for_tests_with_clients(
        p.as_ref().clone(),
        gateway,
        client.clone(),
    );
    let admitted = p
        .native_mcp(&f.request(json!({"query":"integration owned"})))
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let _guard = cancel.clone().drop_guard();
    let job = tokio::spawn(worker(p.clone(), engine, admitted.run_id, cancel.clone()));
    approve(
        p.as_ref(),
        CompanyId::new(f.s.f.company),
        WorkflowActor::authenticated(f.s.f.owner).unwrap(),
        admitted.run_id,
        remote,
        Duration::ZERO,
    )
    .await;
    eventually(async || {
        let state: String =
            sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
                .bind(f.s.f.company)
                .bind(admitted.run_id.as_uuid())
                .fetch_one(&f.s.f.database.pool)
                .await
                .unwrap();
        assert!(
            !matches!(state.as_str(), "failed" | "cancelled"),
            "native integration run ended: {state}"
        );
        (state == "succeeded").then_some(())
    })
    .await;
    cancel.cancel();
    job.await.unwrap();
    client.shutdown().await.unwrap();
    admitted
}

async fn admit_with_competing_credential_owner(
    f: &McpSetup,
    account: &ConnectedAccount,
    request: &NativeMcpRequest,
) -> NativeMcpResponse {
    let mut owner = f.s.f.database.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM integration_accounts WHERE id=$1 FOR UPDATE")
        .bind(account.id.as_uuid())
        .execute(&mut *owner)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM integration_use_grants WHERE account_id=$1 FOR UPDATE")
        .bind(account.id.as_uuid())
        .execute(&mut *owner)
        .await
        .unwrap();
    let admitted = tokio::time::timeout(
        Duration::from_secs(2),
        f.s.f.persistence.native_mcp(request),
    )
    .await
    .expect("added native credential metadata must not wait for its owner")
    .unwrap();
    owner.rollback().await.unwrap();
    admitted
}

#[tokio::test]
async fn native_mcp_integration_owned_consent_approval_credential_and_revocation_reach_real_gateway()
 {
    let remote = remote::mcp_server(remote::Call::Json).await;
    remote.set_tools(json!([{"name":"lookup","description":"Lookup","inputSchema":{"type":"object","required":["query"],"additionalProperties":false,"properties":{"query":{"type":"string","maxLength":128}}},"outputSchema":{"type":"object"}}]));
    let (f, vault, account) = Box::pin(setup()).await;
    let admitted = Box::pin(execute(&f, vault, &account, &remote)).await;
    let p = &f.s.f.persistence;
    assert_eq!(remote.calls(), 1);
    assert!(!remote.auth().is_empty());
    assert!(
        remote
            .auth()
            .iter()
            .all(|header| header == &format!("Bearer {SECRET}"))
    );
    let replay = p
        .native_mcp(&f.request(json!({"query":"integration owned"})))
        .await
        .unwrap();
    assert_eq!(replay.call_id, admitted.call_id);
    assert!(matches!(replay.state, NativeMcpState::Completed { .. }));
    let mut second = f.request(json!({"query":"revoked consent"}));
    second.body.invocation.model_call_id = "toolu_after_consent".to_owned().try_into().unwrap();
    let next = admit_with_competing_credential_owner(&f, &account, &second).await;
    let claim = Box::pin(live_tests::claim(&f, next.run_id)).await;
    sqlx::query(
        "UPDATE integration_use_grants SET enabled=false,revision=revision+1 WHERE account_id=$1",
    )
    .bind(account.id.as_uuid())
    .execute(&f.s.f.database.pool)
    .await
    .unwrap();
    assert!(
        !p.validate_io(claim.fence, live_tests::lease())
            .await
            .unwrap()
    );
    assert!(
        Box::pin(p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into())))
            .await
            .is_err()
    );
    second.body.invocation.model_call_id =
        "toolu_revoked_new_effect".to_owned().try_into().unwrap();
    assert!(Box::pin(p.native_mcp(&second)).await.is_err());
    assert_eq!(
        remote.calls(),
        1,
        "revoked consent cannot open another effect"
    );
}
