//! 06-F019(h): an agent's approved MCP tool end to end. The agent names the tool; the
//! workflow's bound MCP slot supplies the connection; ActionService dispatches it once and
//! replay neither calls the model nor the MCP server again.
use super::*;
use crate::adapters::{
    harness::rig::{WorkflowAgentRuntime, providers::ProviderRegistry},
    persistence::workflow::{
        agent_publication::AgentPublicationDirectory,
        tests::approval_test_support::{composer, notices},
    },
    workflow_agent::AgentActionTools,
    workflow_http::{HttpDestinationPolicy, HttpRuntime},
    workflow_message::MessageRuntime,
};
use crate::application::{
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        company::{CompanyModelConnectionWrite, CompanyPersistence},
    },
    workflow::worker::WorkflowHandler,
};
use crate::services::test_support::{
    LlmTurn, SCRIPTED_MODEL, SCRIPTED_PROVIDER, register_scripted_agent_base_url, scripted_llm,
};

#[tokio::test]
async fn workflow_agent_mcp_tool_dispatches_once_through_the_bound_slot() {
    let server = mcp_server(Call::Json).await;
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let (connection, _) = Box::pin(configured(&fixture, &server.url)).await;
    let p = fixture.persistence.clone();
    let company = fixture.target.company;
    let agent = AgentPersistence::create(
        &p,
        company.as_uuid(),
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("Look the customer up, then answer.".into()),
            workflow_tools: Some(vec![TypeName::parse(TOOL).unwrap()]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    CompanyPersistence::replace_model_connections_for_user(
        &p,
        fixture.target.actor.user_id(),
        company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: mcp::McpPublicationDirectory {
                persistence: p.clone(),
                selections: vec![mcp::McpPublicationSelection {
                    connection: connection.id,
                    slot: ResourceName::parse("service").unwrap(),
                    tool: TypeName::parse(TOOL).unwrap(),
                }],
            },
        },
    );
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["resources"] = json!([{"slot":"service","kind":"mcp"}]);
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await.unwrap();
    let target = Box::pin(bound(&fixture, connection.id, publication.version)).await;
    let admission = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            company_id: company,
            actor: fixture.target.actor,
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: target.binding,
            idempotency_key: IdempotencyKey::parse("agent-mcp-run").unwrap(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await
        .unwrap();
    let AdmissionResult::Created(run) = admission else {
        panic!("{admission:?}")
    };
    let claim = claim(&p, job_scope(&p, company, run, "start").await).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(TOOL, json!({"customer_id":"c-42"})),
        LlmTurn::text("\"Ada\""),
    ])
    .await;
    register_scripted_agent_base_url(agent.id, &llm.base_url);
    let runtime = WorkflowAgentRuntime::new(
        ProviderRegistry::standard().unwrap(),
        p.clone(),
        AgentActionTools::new(
            Arc::new(HttpRuntime::new(
                p.clone(),
                HttpDestinationPolicy::default(),
                lease(),
            )),
            Arc::new(McpRuntime::new(
                p.clone(),
                guarded_client(&server.url),
                lease(),
            )),
            Arc::new(MessageRuntime::new(
                p.clone(),
                composer(&p),
                "agents.test".into(),
                lease(),
            )),
        ),
        Some(notices(&p)),
        lease(),
    );
    let kind = WorkflowStepKind("agent.run".into());
    let outcome = Box::pin(runtime.execute(&kind, &claim)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("Ada"));
    assert_eq!(server.calls(), 1);
    assert_eq!(server.arguments(), vec![json!({"customer_id":"c-42"})]);
    assert!(
        server
            .auth()
            .iter()
            .all(|auth| auth == &format!("Bearer {TOKEN}")),
        "the slot's connection credential authenticates the call"
    );
    let requests = llm.observed();
    assert!(requests[1]["messages"].to_string().contains("Ada"));
    assert!(
        !requests[1]["messages"].to_string().contains(TOKEN),
        "no credential reaches the model"
    );
    let replay = Box::pin(runtime.execute(&kind, &claim)).await;
    assert_eq!(replay.completed_result().unwrap(), json!("Ada"));
    assert_eq!(server.calls(), 1);
    assert_eq!(llm.finish().await.unwrap(), 2);
    assert_eq!(action_counts(&p).await, counts(1, 1, 1));
}
