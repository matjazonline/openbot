use super::*;
use crate::application::{agent::AgentToolOrigin, workflow::agent::PreparedTool};
use crate::entities::agent_capability::{
    AgentBindingId, AgentBindingSource, AgentToolAlias, AgentToolBinding,
};

const ALIAS: &str = "shared.lookup";
const DECLARED: &str = "shared__lookup";

#[path = "mcp_agent_authority_tests.rs"]
mod authority;

struct McpBinding {
    connection: Uuid,
    reviewed: Uuid,
}
struct AgentRun {
    agent: Uuid,
    claim: ClaimedWorkflow,
}
struct SavedCall {
    origin: AgentToolOrigin,
    call: ModelToolCallId,
    arguments: Value,
}

#[tokio::test]
async fn autonomous_agent_dispatch_same_alias_keeps_two_connections_origins_and_receipts_isolated()
{
    let first = mcp_server(Call::Json).await;
    let second = mcp_server(Call::Json).await;
    let fixture = fixture().await;
    let left_binding = registered(&fixture, &first, "first").await;
    let right_binding = registered(&fixture, &second, "second").await;
    let left = agent_run(&fixture, "first", &left_binding).await;
    let right = agent_run(&fixture, "second", &right_binding).await;
    let mut left_model = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED, json!({"customer_id":"first"})),
        LlmTurn::text("\"first\""),
    ])
    .await;
    let mut right_model = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED, json!({"customer_id":"second"})),
        LlmTurn::text("\"second\""),
    ])
    .await;
    register_scripted_agent_base_url(left.agent, &left_model.base_url);
    register_scripted_agent_base_url(right.agent, &right_model.base_url);
    let p = &fixture.persistence;
    let left_mcp = Arc::new(McpRuntime::new(
        p.clone(),
        guarded_client(&first.url),
        lease(),
    ));
    let right_mcp = Arc::new(McpRuntime::new(
        p.clone(),
        guarded_client(&second.url),
        lease(),
    ));
    let left_runtime = mcp_agent_runtime(p, left_mcp.clone());
    let right_runtime = mcp_agent_runtime(p, right_mcp);
    let kind = WorkflowStepKind("agent.run".into());
    // Competing real roots share an alias and company but hold boxed provider seams.
    let (one, two) = tokio::join!(
        Box::pin(left_runtime.execute(&kind, &left.claim)),
        Box::pin(right_runtime.execute(&kind, &right.claim))
    );
    assert_eq!(one.completed_result().unwrap(), json!("first"));
    assert_eq!(two.completed_result().unwrap(), json!("second"));
    assert_peer(&first, "first");
    assert_peer(&second, "second");
    assert_eq!(
        left_model.observed()[0]["tools"][0]["function"]["name"],
        DECLARED
    );
    assert_eq!(
        right_model.observed()[0]["tools"][0]["function"]["name"],
        DECLARED
    );
    reject_origin_swap(p, &left, &right, &right_binding, &left_mcp, &first, &second).await;
    let (one, two) = tokio::join!(
        Box::pin(left_runtime.execute(&kind, &left.claim)),
        Box::pin(right_runtime.execute(&kind, &right.claim))
    );
    assert_eq!(one.completed_result().unwrap(), json!("first"));
    assert_eq!(two.completed_result().unwrap(), json!("second"));
    assert_eq!(first.calls(), 1);
    assert_eq!(second.calls(), 1);
    assert_eq!(left_model.finish().await.unwrap(), 2);
    assert_eq!(right_model.finish().await.unwrap(), 2);
    assert_eq!(action_counts(p).await, counts(2, 2, 2));
}

fn assert_peer(server: &McpServer, customer: &str) {
    assert_eq!(server.calls(), 1);
    assert_eq!(server.arguments(), vec![json!({"customer_id":customer})]);
    assert_eq!(
        server.call_names(),
        vec![TOOL.to_owned()],
        "the alias must never become the remote name"
    );
    assert!(
        !server.auth().is_empty(),
        "the actual connection credential is sent"
    );
    assert!(
        server
            .auth()
            .iter()
            .all(|header| header == &format!("Bearer {TOKEN}"))
    );
    assert!(server.count("DELETE") >= 1, "the actual session is closed");
}

async fn reject_origin_swap(
    p: &PostgresPersistence,
    left: &AgentRun,
    right: &AgentRun,
    target: &McpBinding,
    runtime: &McpRuntime,
    first: &McpServer,
    second: &McpServer,
) {
    let one = saved_call(p, &left.claim).await;
    let two = saved_call(p, &right.claim).await;
    assert_ne!(one.origin.agent, two.origin.agent);
    assert_ne!(one.origin.manifest, two.origin.manifest);
    let before_first = first.log();
    let before_second = second.log();
    let forged = McpToolCall {
        target: McpToolTarget::Agent {
            connection: target.connection,
            origin: two.origin,
        },
        tool: TypeName::parse(TOOL).unwrap(),
        arguments: one.arguments,
        call: one.call,
    };
    assert!(
        runtime
            .call_tool(left.claim.fence, forged, &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(first.log(), before_first);
    assert_eq!(
        second.log(),
        before_second,
        "a valid origin from another root cannot open its connection"
    );
}

async fn fixture() -> Fixture {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    CompanyPersistence::replace_model_connections_for_user(
        &fixture.persistence,
        fixture.target.actor.user_id(),
        fixture.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    fixture
}

async fn registered(fixture: &Fixture, server: &McpServer, slug: &str) -> McpBinding {
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let mut write = connection_write(&server.url);
    write.slug = slug.into();
    let connection = p
        .create_mcp_connection(company.as_uuid(), write)
        .await
        .unwrap();
    p.replace_mcp_token(
        company.as_uuid(),
        connection.id,
        connection.revision,
        Some(SecretString::from(TOKEN)),
    )
    .await
    .unwrap();
    let current = connection_on(p, company.as_uuid(), connection.id).await;
    let reviewed = p
        .register_mcp_operation(
            company,
            fixture.target.actor,
            McpOperationWrite {
                connection: connection.id,
                catalog_revision: current.revision,
                expected_version: None,
                operation: operation(company),
            },
        )
        .await
        .unwrap();
    McpBinding {
        connection: connection.id,
        reviewed,
    }
}

async fn agent_run(fixture: &Fixture, key: &str, binding: &McpBinding) -> AgentRun {
    let source = AgentBindingSource::Mcp {
        connection: binding.connection,
        operation: TypeName::parse(TOOL).unwrap(),
        reviewed_version: binding.reviewed,
        remote_name: TOOL.to_owned().try_into().unwrap(),
    };
    let agent = AgentPersistence::create(
        &fixture.persistence,
        fixture.target.company.as_uuid(),
        AgentWrite {
            name: key.into(),
            slug: key.into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            tool_bindings: Some(vec![AgentToolBinding {
                id: AgentBindingId::new(Uuid::new_v4()),
                alias: AgentToolAlias::parse(ALIAS).unwrap(),
                source,
            }]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    fixture.publish_agent(agent.id).await;
    let target = publish_reference(fixture, key).await;
    let claim = admit(fixture, target, key).await;
    AgentRun {
        agent: agent.id,
        claim,
    }
}

async fn publish_reference(fixture: &Fixture, key: &str) -> BindingTarget {
    let p = &fixture.persistence;
    let service = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: Preflight,
        },
    );
    let target = DefinitionTarget {
        workflow: WorkflowId::new(Uuid::new_v4()),
        ..fixture.target
    };
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["workflow_id"] = json!(target.workflow.to_string());
    source["resources"] = json!([]);
    source["steps"]["start"]["with"]["agent"] = json!({"literal":key});
    let mut draft = fixture.save(None);
    draft.target = target;
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
    let mut publication = fixture.publish();
    publication.target = target;
    publication.version = VersionId::new(Uuid::new_v4());
    publication.key = IdempotencyKey::parse(format!("mcp-reference:{}", target.workflow)).unwrap();
    let version = publication.version;
    service.publish(publication).await.unwrap();
    bind_without_slots(fixture, version).await
}

async fn admit(fixture: &Fixture, target: BindingTarget, key: &str) -> ClaimedWorkflow {
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let result = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            history_session: None,
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
            idempotency_key: IdempotencyKey::parse(key).unwrap(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await
        .unwrap();
    let AdmissionResult::Created(run) = result else {
        panic!("new admission")
    };
    claim(p, job_scope(p, company, run, "start").await).await
}

async fn saved_call(p: &PostgresPersistence, claim: &ClaimedWorkflow) -> SavedCall {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        checkpoint: Value,
        workflow_prepared: Value,
    }
    let scope = claim.fence.scope;
    let row:Row=sqlx::query_as("SELECT id,checkpoint,workflow_prepared FROM task_harness_runs WHERE company_id=$1 AND workflow_run_id=$2 AND workflow_execution_id=$3 AND task_id=$4")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0)
        .fetch_one(p.pool()).await.unwrap();
    let checkpoint =
        crate::adapters::persistence::harness_checkpoint::decode(row.id, row.checkpoint).unwrap();
    let call = &checkpoint.invocations[0].call;
    let tools: Vec<PreparedTool> =
        serde_json::from_value(row.workflow_prepared["tools"].clone()).unwrap();
    let tool = tools
        .iter()
        .find(|tool| tool.declared.as_str() == call.tool_id.as_str())
        .unwrap();
    SavedCall {
        origin: tool.origin,
        call: ModelToolCallId::parse(call.invocation_id.0.to_string()).unwrap(),
        arguments: call.arguments.clone(),
    }
}
