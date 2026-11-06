//! Agent-owned concrete MCP bindings drive native execution and isolated samples.
//! Native replay calls the effect once; samples never open the connection.
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
async fn autonomous_agent_dispatch_mcp_without_workflow_slot_and_replay() {
    let server = mcp_server(Call::Json).await;
    let (fixture, agent) = published_mcp_agent(&server).await;
    let p = fixture.persistence.clone();
    let claim = claim_without_slots(&fixture).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(TOOL, json!({"customer_id":"c-42"})),
        LlmTurn::text("\"Ada\""),
    ])
    .await;
    register_scripted_agent_base_url(agent.as_uuid(), &llm.base_url);
    let mcp = Arc::new(McpRuntime::new(
        p.clone(),
        guarded_client(&server.url),
        lease(),
    ));
    let runtime = mcp_agent_runtime(&p, mcp.clone());
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
        "the agent binding connection credential authenticates the call"
    );
    refused_agent_calls(&p, &fixture, &claim, &mcp, &server).await;
    assert!(
        server.count("DELETE") >= 1,
        "the successful MCP session is closed"
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

async fn published_mcp_agent(
    server: &McpServer,
) -> (Fixture, crate::entities::agent_capability::AgentId) {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let (_connection, _) = Box::pin(configured(&fixture, &server.url)).await;
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
            tool_bindings: Some(
                crate::adapters::persistence::agent_binding_selection::resolve_bindings_on(
                    &mut p.pool().acquire().await.unwrap(),
                    company.as_uuid(),
                    None,
                    &[TypeName::parse(TOOL).unwrap()],
                )
                .await
                .unwrap(),
            ),
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
    fixture.publish_agent(agent.id).await;
    (
        fixture,
        crate::entities::agent_capability::AgentId::new(agent.id),
    )
}

async fn claim_without_slots(fixture: &Fixture) -> ClaimedWorkflow {
    let p = fixture.persistence.clone();
    let company = fixture.target.company;
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: Preflight,
        },
    );
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["workflow_id"] = json!(fixture.target.workflow.to_string());
    source["resources"] = json!([]);
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let mut publication = fixture.publish();
    publication.key =
        IdempotencyKey::parse(format!("agent-mcp-publication:{}", fixture.target.workflow))
            .unwrap();
    definitions.publish(publication.clone()).await.unwrap();
    let target = bind_without_slots(fixture, publication.version).await;
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
            idempotency_key: IdempotencyKey::parse(format!(
                "agent-mcp-run:{}",
                fixture.target.workflow
            ))
            .unwrap(),
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
    claim(&p, job_scope(&p, company, run, "start").await).await
}

fn mcp_agent_runtime(
    p: &PostgresPersistence,
    mcp: Arc<McpRuntime>,
) -> WorkflowAgentRuntime<PostgresPersistence, AgentActionTools> {
    WorkflowAgentRuntime::new(
        ProviderRegistry::standard().unwrap(),
        p.clone(),
        AgentActionTools::new(
            Arc::new(HttpRuntime::new(
                p.clone(),
                HttpDestinationPolicy::default(),
                lease(),
            )),
            mcp.clone(),
            Arc::new(MessageRuntime::new(
                p.clone(),
                composer(p),
                "agents.test".into(),
                lease(),
            )),
        ),
        Some(notices(p)),
        lease(),
    )
}

#[path = "mcp_agent_isolation_tests.rs"]
mod isolation;

async fn bind_without_slots(fixture: &Fixture, version: VersionId) -> BindingTarget {
    use crate::application::workflow::lifecycle::{ConfigureBinding, SetBindingActivity};
    let p = &fixture.persistence;
    let target = BindingTarget {
        company: fixture.target.company,
        actor: fixture.target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let service = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let configured = service
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: BTreeMap::new(),
            run_as: None,
            message_trigger: None,
        })
        .await
        .unwrap();
    service
        .activate(SetBindingActivity {
            target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    target
}

async fn refused_agent_calls(
    p: &PostgresPersistence,
    fixture: &Fixture,
    claim: &ClaimedWorkflow,
    runtime: &McpRuntime,
    server: &McpServer,
) {
    use crate::application::workflow::actions::ModelToolCallId;
    use crate::application::workflow::agent::AgentPreparation;
    use crate::application::workflow::mcp::{McpToolCall, McpToolTarget};
    use crate::entities::agent_capability::{
        AgentBindingId, AgentId, AgentRevisionId, DependencyManifestId,
    };
    let prepared = p.prepare_agent(claim).await.unwrap().prepared;
    let original = prepared.tools()[0].origin;
    let connection = match prepared.tools()[0].target {
        crate::application::workflow::agent::AgentToolTarget::Mcp { connection } => connection,
        _ => panic!("MCP binding"),
    };
    let fence = claim.fence.scope;
    let checkpoint: Value = sqlx::query_scalar("SELECT checkpoint FROM task_harness_runs WHERE company_id=$1 AND workflow_run_id=$2 AND workflow_execution_id=$3 AND task_id=$4")
        .bind(fixture.target.company.as_uuid()).bind(fence.run.as_uuid()).bind(fence.execution.as_uuid()).bind(fence.job.0).fetch_one(p.pool()).await.unwrap();
    let saved = &checkpoint["invocations"][0]["call"];
    for variant in 0..10 {
        let mut origin = original;
        let mut target = connection;
        let mut tool = TypeName::parse(TOOL).unwrap();
        let mut arguments = saved["arguments"].clone();
        let mut call = saved["invocation_id"].as_str().unwrap().to_owned();
        match variant {
            0 => origin.binding = AgentBindingId::new(Uuid::new_v4()),
            1 => origin.agent = AgentId::new(Uuid::new_v4()),
            2 => origin.revision = AgentRevisionId::new(Uuid::new_v4()),
            3 => origin.manifest = DependencyManifestId::new(Uuid::new_v4()),
            4 => target = Uuid::new_v4(),
            5 => tool = TypeName::parse("invented_tool").unwrap(),
            6 => arguments = json!({"unexpected":"argument"}),
            7 => call = Uuid::new_v4().to_string(),
            8 => {}
            _ => origin.binding = AgentBindingId::new(Uuid::nil()),
        }
        let target = if variant == 8 {
            McpToolTarget::Workflow {
                slot: ResourceName::parse("missing").unwrap(),
            }
        } else {
            McpToolTarget::Agent {
                connection: target,
                origin,
            }
        };
        let call = McpToolCall {
            target,
            tool,
            arguments,
            call: ModelToolCallId::parse(call).unwrap(),
        };
        assert!(
            runtime
                .call_tool(
                    claim.fence,
                    call,
                    &tokio_util::sync::CancellationToken::new()
                )
                .await
                .is_err(),
            "injection {variant}"
        );
        assert_eq!(
            server.calls(),
            1,
            "injection {variant} must make no remote effect"
        );
    }
}

#[tokio::test]
async fn autonomous_agent_samples_mcp_without_slots_never_opens_the_server() {
    use crate::application::agent::AgentPublications;
    use crate::application::workflow::sample::{SampleRequest, SampleStatus};
    use crate::application::workflow::sample_capture::SampleDefinitionCapture;
    let server = mcp_server(Call::Json).await;
    let (fixture, agent) = published_mcp_agent(&server).await;
    let p = fixture.persistence.clone();
    let service = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: Preflight,
        },
    );
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["resources"] = json!([]);
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
    let scope = crate::application::agent::AgentPublicationScope {
        company: fixture.target.company,
        actor: fixture.target.actor,
        agent,
    };
    let active = p.active_agent_revision(scope).await.unwrap().unwrap();
    let before: std::collections::BTreeMap<_, _> = ["POST", "GET", "DELETE"]
        .into_iter()
        .map(|method| (method, server.count(method)))
        .collect();
    let request=SampleRequest { agents:vec![],input:json!({}),params:json!({}),scripts:serde_json::from_value(json!([{
        "workflow_id":fixture.target.workflow,"step":"start","responses":[{
            "output":"Sample answer","tools":[{"name":TOOL,"input":{"customer_id":"c-42"},"output":{"content":[],"isError":false,"structuredContent":{"name":"Ada"}}}]
        }]
    }])).unwrap() };
    let capture = crate::adapters::persistence::workflow::SampleCapture {
        persistence: p.clone(),
        http: vec![],
        mcp: vec![],
        refused_kinds: &[],
    };
    let target = crate::application::workflow::lifecycle::DefinitionTarget {
        company: fixture.target.company,
        actor: fixture.target.actor,
        workflow: fixture.target.workflow,
    };
    let frozen = capture
        .capture(target, DraftRevision::new(1).unwrap(), request)
        .await
        .unwrap();
    let report = crate::application::workflow::sample::execute(frozen).unwrap();
    capture.authorize_release(target).await.unwrap();
    assert_eq!(
        report.status,
        SampleStatus::Completed,
        "{:?}",
        report.diagnostic
    );
    assert_eq!(report.usage.direct_tool_calls, 1);
    assert_eq!(report.agent_selections.len(), 1);
    assert_eq!(
        p.active_agent_revision(scope).await.unwrap().unwrap().id,
        active.id
    );
    for (method, count) in before {
        assert_eq!(
            server.count(method),
            count,
            "sampling never opens a transport"
        );
    }
    assert_eq!(server.calls(), 0);
}

#[path = "autonomous_agent_authoring_mcp_tests.rs"]
mod authoring_journey;
