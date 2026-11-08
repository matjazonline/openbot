use super::*;
use crate::{
    adapters::{
        integrations::owner::owner_for_user,
        persistence::{
            integration_bindings::PostgresIntegrationBindings, integrations::PostgresVault,
        },
    },
    application::{
        integrations::{
            grants::GrantWriter,
            management::{AccountRepository, AccountWrite},
            *,
        },
        use_cases::integration_binding::*,
    },
    domain::integrations as integration,
};
use chrono::{Duration as ChronoDuration, Utc};
use tokio_util::sync::CancellationToken;
struct VaultMcpSetup {
    setup: McpSetup,
    vault: PostgresVault,
    grant: integration::authority::UseGrant,
    claim: ClaimedWorkflow,
}
async fn fixture() -> VaultMcpSetup {
    let server = mcp_server(Call::Json).await;
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let (connection, operation) =
        Box::pin(configured(&fixture, "https://mcp.example.test/rpc")).await;
    let version = Box::pin(published(&fixture, connection.id)).await;
    let target = Box::pin(bound(&fixture, connection.id, version)).await;
    let client = guarded_client(&server.url);
    let setup = McpSetup {
        fixture,
        server,
        connection,
        operation,
        version,
        target,
        client,
    };
    let p = &setup.fixture.persistence;
    let vault = PostgresVault::new(p.clone());
    let account = vault
        .create(
            &owner_for_user(setup.actor().user_id()),
            AccountWrite {
                key: integration::AccountKey::parse("mcp-private").unwrap(),
                provider: integration::ProviderId::parse("static-bearer").unwrap(),
                config: integration::AuthConfigId::parse("static-bearer").unwrap(),
                method: integration::AuthMethod::StaticSecret,
                schema: integration::CredentialSchema::parse("static-v1").unwrap(),
                identity: None,
                metadata: integration::AccountMetadata::new("MCP private".into(), None).unwrap(),
                payload: SecretString::from("private-mcp-key"),
            },
        )
        .await
        .unwrap();
    let host = PostgresIntegrationBindings {
        persistence: p.clone(),
        vault: vault.clone(),
    };
    let proposal = host
        .propose(
            setup.actor().user_id(),
            ProposeBinding {
                company: setup.company().as_uuid(),
                connection: setup.connection.id,
                kind: BindingKind::Mcp,
                app: integration::AppId::parse("static-bearer").unwrap(),
                expected_revision: setup.connection.revision,
            },
        )
        .await
        .unwrap();
    let grant = host
        .confirm(
            VerifiedOwner::from_verified_client(account.owner.clone()),
            proposal.id,
            account.id,
            account.authorization_revision,
        )
        .await
        .unwrap();
    let claim = claimed(&setup).await;
    VaultMcpSetup {
        setup,
        vault,
        grant,
        claim,
    }
}
#[tokio::test]
async fn vault_dispatch_authority_mcp_uses_the_real_step_and_fence_without_exposing_a_token() {
    let f = Box::pin(fixture()).await;
    let p = &f.setup.fixture.persistence;
    let request = p.mcp_step_request(&f.claim, &kind()).await.unwrap();
    let usage = p.mcp_use(&request).await.unwrap();
    assert!(matches!(
        usage.authentication,
        crate::application::workflow::http::PreparedAuthentication::Integration(_)
    ));
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    let (invocation, context) = p
        .prepare_integration_invocation(
            ActionDispatchRequest {
                subject: intent.approval_subject(),
                fence: f.claim.fence,
                lease: lease(),
            },
            integration::DispatchId::new(Uuid::new_v4()),
            Utc::now() + ChronoDuration::seconds(20),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    f.vault
        .final_dispatch_guard(&invocation, &context)
        .await
        .unwrap();
    f.vault
        .revoke(&f.grant.owner, f.grant.id, f.grant.revision)
        .await
        .unwrap();
    assert!(
        f.vault
            .final_dispatch_guard(&invocation, &context)
            .await
            .is_err()
    );
    assert!(f.setup.server.log().is_empty());
}
#[tokio::test]
async fn vault_dispatch_authority_generic_mcp_tool_cannot_borrow_a_workflow_run() {
    let f = Box::pin(fixture()).await;
    let p = &f.setup.fixture.persistence;
    let scope = ActionScope {
        company: f.claim.fence.scope.company,
        run: f.claim.fence.scope.run,
        execution: f.claim.fence.scope.execution,
    };
    assert!(
        p.mcp_tool_request(scope, &tool_call("direct-model-tool"))
            .await
            .is_err()
    );
    assert!(f.setup.server.log().is_empty());
}

#[tokio::test]
async fn vault_binding_mcp_definition_edits_preserve_the_confirmed_source_and_refuse_substitution()
{
    let f = Box::pin(fixture()).await;
    let p = &f.setup.fixture.persistence;
    let current = f.setup.current().await;
    let definition = McpConnectionWrite {
        slug: "renamed-vault".into(),
        endpoint: current.endpoint.clone(),
        enabled: current.enabled,
        auth: current.auth.clone(),
        discovered_tools: current.discovered_tools.clone(),
        tool_grants: current.tool_grants.clone(),
    };
    let saved = p
        .update_mcp_connection(
            current.company_id,
            current.id,
            current.revision,
            definition.clone(),
        )
        .await
        .unwrap();
    assert_eq!(saved.auth, current.auth);
    assert!(!saved.secret_set);
    let mut forged = definition;
    forged.auth = match saved.auth.clone() {
        McpAuth::Integration(mut reference) => {
            reference.grant = integration::UseGrantId::new(Uuid::new_v4());
            McpAuth::Integration(reference)
        }
        _ => panic!("integration source"),
    };
    assert!(
        p.update_mcp_connection(saved.company_id, saved.id, saved.revision, forged)
            .await
            .is_err()
    );
    assert!(f.setup.server.log().is_empty());
}
