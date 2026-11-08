//! Genuine MCP execution authority sharing a private account with HTTP fixtures.
use super::*;
use crate::{
    adapters::persistence::{
        integration_bindings::PostgresIntegrationBindings, integrations::PostgresVault,
    },
    application::{
        integrations::{AuthorizedInvocation, ExecutionContext, VerifiedOwner},
        use_cases::integration_binding::*,
    },
    domain::integrations::{AppId, ConnectedAccount, DispatchId},
};
pub(in crate::adapters::persistence::workflow) struct Proof {
    pub invocation: AuthorizedInvocation,
    pub context: ExecutionContext,
}
async fn attach_account(
    setup: &McpSetup,
    vault: &PostgresVault,
    account: &ConnectedAccount,
    app: AppId,
) {
    let host = PostgresIntegrationBindings {
        persistence: setup.fixture.persistence.clone(),
        vault: vault.clone(),
    };
    let proposal = host
        .propose(
            setup.actor().user_id(),
            ProposeBinding {
                company: setup.company().as_uuid(),
                connection: setup.connection.id,
                kind: BindingKind::Mcp,
                app,
                expected_revision: setup.connection.revision,
            },
        )
        .await
        .unwrap();
    host.confirm(
        VerifiedOwner::from_verified_client(account.owner.clone()),
        proposal.id,
        account.id,
        account.authorization_revision,
    )
    .await
    .unwrap();
}
pub(in crate::adapters::persistence::workflow::tests) async fn configured_setup(
    original: &Fixture,
    vault: &PostgresVault,
    account: &ConnectedAccount,
    app: AppId,
) -> McpSetup {
    let fixture = Fixture {
        _db: original._db.clone(),
        persistence: original.persistence.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..original.target
        },
    };
    let server = mcp_server(Call::Json).await;
    let (connection, operation) =
        Box::pin(configured(&fixture, "https://mcp.example.test/rpc")).await;
    let mut definition = source();
    definition["workflow_id"] = json!(fixture.target.workflow.as_uuid());
    let version = Box::pin(publish_unique(&fixture, connection.id, definition)).await;
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
    attach_account(&setup, vault, account, app).await;
    setup
}

pub(in crate::adapters::persistence::workflow) async fn prepare(
    original: &Fixture,
    vault: &PostgresVault,
    account: &ConnectedAccount,
    app: AppId,
) -> Proof {
    let setup = Box::pin(configured_setup(original, vault, account, app)).await;
    let claim = claimed(&setup).await;
    let p = &setup.fixture.persistence;
    let request = p.mcp_step_request(&claim, &kind()).await.unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    let (invocation, context) = p
        .prepare_integration_invocation(
            ActionDispatchRequest {
                subject: intent.approval_subject(),
                fence: claim.fence,
                lease: lease(),
            },
            DispatchId::new(Uuid::new_v4()),
            chrono::Utc::now() + chrono::Duration::seconds(20),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(setup.server.calls(), 0);
    Proof {
        invocation,
        context,
    }
}

async fn publish_unique(fixture: &Fixture, connection: Uuid, definition: Value) -> VersionId {
    let p = &fixture.persistence;
    let directory = mcp::McpPublicationDirectory {
        persistence: p.clone(),
        selections: vec![mcp::McpPublicationSelection {
            connection,
            slot: ResourceName::parse("service").unwrap(),
            tool: TypeName::parse(TOOL).unwrap(),
        }],
    };
    let service = DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let mut draft = fixture.save(None);
    draft.content.source = definition.to_string();
    service.save(draft).await.unwrap();
    let mut command = fixture.publish();
    command.key = IdempotencyKey::parse(format!(
        "renewal-mcp-{}",
        fixture.target.workflow.as_uuid().simple()
    ))
    .unwrap();
    service.publish(command.clone()).await.unwrap();
    command.version
}

#[path = "mcp_gateway_tests.rs"]
mod gateway_tests;
