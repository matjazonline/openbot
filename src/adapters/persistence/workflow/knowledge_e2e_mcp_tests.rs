//! A real bound MCP service observes no public call after private search; a CRM cleared to9 works.
use super::*;
use crate::adapters::persistence::workflow::mcp::McpPublicationSelection;
use crate::adapters::persistence::{
    credentials::CredentialCipher, workflow::tests::mcp_tests::support as mcp_fixture,
};
use crate::application::use_cases::mcp::McpPersistence;
use crate::application::workflow::connection_level::ConnectionLevelCommand;
use crate::domain::workflow::{ResourceName, RuntimeResourceId, TypeName};
use crate::infra::workflow::WorkflowEngineSetup;

fn install_client(j: &mut SalesJourney, server: &mcp_fixture::McpServer) {
    let p = PostgresPersistence::with_credential_cipher(
        j.d.p().pool().clone(),
        CredentialCipher::for_test(),
    );
    j.d.fixture.persistence = p.clone();
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(Arc::new(EmailRenderer::new(APP_DOMAIN)))
            .unwrap(),
    );
    let config = AppConfig {
        app_domain_name: APP_DOMAIN.into(),
        ..AppConfig::for_test()
    };
    let engine = WorkflowEngine::new(
        p.clone(),
        WorkflowEngineSetup {
            config: &config,
            composer: DeliveryComposer::new(renderers, Arc::new(p)),
            mcp_client: mcp_fixture::guarded_client(&server.url),
            monitoring: Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
            slots: 1,
            knowledge: j.d.knowledge.clone(),
        },
    )
    .unwrap();
    // No workers have started. Reuse the ONE existing knowledge Arc and retarget its wakeup.
    j.d.threads = composition::threads(
        j.d.p(),
        &config,
        Arc::new(
            TransportRenderers::new()
                .register(Arc::new(EmailRenderer::new(APP_DOMAIN)))
                .unwrap(),
        ),
        engine.intent_wakeups.clone(),
    );
    j.d.engine = engine;
}

async fn configure_connection(
    j: &mut SalesJourney,
    server: &mcp_fixture::McpServer,
    level: AccessLevel,
) {
    install_client(j, server);
    let (connection, _) = Box::pin(mcp_fixture::configured(&j.d.fixture, &server.url)).await;
    j.d.p()
        .set_mcp_level(ConnectionLevelCommand {
            company: j.d.fixture.target.company,
            actor: j.d.fixture.target.actor,
            connection: connection.id,
            expected_revision: connection.revision,
            access_level: level,
        })
        .await
        .unwrap();
    sqlx::query("UPDATE agents SET workflow_tools=ARRAY['search_knowledge',$2] WHERE company_id=$1 AND slug='assistant'")
        .bind(j.d.fixture.target.company.as_uuid()).bind(mcp_fixture::TOOL).execute(j.d.p().pool()).await.unwrap();
    j.d.mcp_selections = vec![McpPublicationSelection {
        connection: connection.id,
        slot: ResourceName::parse("service").unwrap(),
        tool: TypeName::parse(mcp_fixture::TOOL).unwrap(),
    }];
    j.d.resources = BTreeMap::from([(
        ResourceName::parse("service").unwrap(),
        RuntimeResourceId::new(connection.id),
    )]);
}

#[tokio::test]
async fn knowledge_e2e_private_search_refuses_public_mcp_to_model_and_allows_level_nine_crm() {
    for level in [AccessLevel::PUBLIC, AccessLevel::L9] {
        let server = mcp_fixture::mcp_server(mcp_fixture::Call::Json).await;
        let scripted = vec![
            LlmTurn::tool_call(
                "search_knowledge",
                json!({"query":"What is the discount floor for AX-4410?"}),
            ),
            LlmTurn::tool_call(mcp_fixture::TOOL, json!({"customer_id":"c-42"})),
            LlmTurn::text("\"sales.md: PRIVATE FLOOR 60\""),
        ];
        let mut j = Box::pin(SalesJourney::new(scripted, Audience::Team)).await;
        configure_connection(&mut j, &server, level).await;
        let member = j.member(AccessLevel::L5).await;
        let mut source = answering_source();
        source["resources"] = json!([{"slot":"service","kind":"mcp"}]);
        j.d.bind(source, Some(MessageTrigger::Direct)).await;
        j.d.email(email(&j.d, member.email.as_str(), "mcp-sales"))
            .await;
        j.drive(1).await;
        let requests = j.d.llm.observed();
        assert_eq!(requests.len(), 3);
        assert!(requests[1]["messages"].to_string().contains(PRIVATE));
        let last = requests[2]["messages"].to_string();
        if level == AccessLevel::PUBLIC {
            assert_eq!(server.calls(), 0);
            assert!(last.contains(crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT));
        } else {
            assert_eq!(server.calls(), 1);
            assert!(last.contains("Ada"));
            assert!(!last.contains(crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT));
        }
        assert!(!last.contains(mcp_fixture::TOKEN));
        assert_eq!(j.transport.calls.load(Ordering::SeqCst), 1);
        let facts = j.facts().await;
        assert_eq!(facts["content"], 5);
        assert_eq!(facts["reply_levels"], json!([5]));
        j.d.knowledge.shutdown().await.unwrap();
        assert_eq!(j.d.llm.finish().await.unwrap(), 3);
    }
}
