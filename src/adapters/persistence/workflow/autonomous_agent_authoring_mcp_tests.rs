use super::*;
use crate::application::agent::*;
use crate::application::workflow::completion::WorkflowCompletion;
use crate::application::workflow::run_views::{RunReadScope, RunViews};
use crate::entities::agent_capability::AgentId;

enum McpDraftAssignment {
    Fresh,
    Retain,
}

async fn publish_typed_mcp(
    f: &Fixture,
    agent: AgentId,
    instructions: &str,
    assignment: McpDraftAssignment,
) -> PublishedAgentRevision {
    let scope = AgentPublicationScope {
        company: f.target.company,
        actor: f.target.actor,
        agent,
    };
    let mut state = f.persistence.authoring_state(scope).await.unwrap();
    assert_eq!(state.tools.len(), 1);
    assert!(matches!(
        state.tools[0].assignment.choice,
        AgentToolChoice::Mcp { .. }
    ));
    if let McpDraftAssignment::Fresh = assignment {
        state.tools[0].assignment.binding = None;
    }
    let saved = f
        .persistence
        .save_company_agent(SaveCompanyAgent {
            scope,
            expected_revision: state.agent.draft_revision,
            write: AgentWrite {
                name: state.agent.name,
                slug: state.agent.slug,
                provider: state.agent.provider,
                model: state.agent.model,
                system_prompt: Some(instructions.into()),
                description: state.agent.description,
                skill_ids: state.skills.iter().map(|skill| skill.id).collect(),
                ..Default::default()
            },
            tools: Some(
                state
                    .tools
                    .into_iter()
                    .map(|tool| tool.assignment)
                    .collect(),
            ),
        })
        .await
        .unwrap();
    AgentPublications::publish_agent(
        &f.persistence,
        PublishAgent {
            scope,
            expected_draft_revision: saved.draft_revision,
            expected_active_revision: state.active.map(|active| active.id),
            key: AgentPublicationKey::parse(Uuid::new_v4().to_string()).unwrap(),
        },
    )
    .await
    .unwrap()
}

async fn admit_again(f: &Fixture, first: &ClaimedWorkflow) -> ActivationRequest {
    let scope = first.fence.scope;
    let binding: Uuid =
        sqlx::query_scalar("SELECT binding_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    let p = &f.persistence;
    let AdmissionResult::Created(run) =
        WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
            .admit(AdmitWorkflowRequest {
                company_id: scope.company,
                actor: f.target.actor,
                association: RelatedAssociation::Company,
                trigger: TriggerRef::new(
                    scope.company,
                    TriggerId::new(Uuid::new_v4()),
                    TriggerSource::Manual,
                )
                .unwrap(),
                correlation_id: CorrelationId::new(),
                binding_id: WorkflowBindingId::new(binding),
                idempotency_key: IdempotencyKey::parse(Uuid::new_v4().to_string()).unwrap(),
                input: json!({}),
                rerun_of: None,
                delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
                expected_binding_revision: None,
                admission_claim: None,
            })
            .await
            .unwrap()
    else {
        panic!("new root")
    };
    job_scope(p, scope.company, run, "start").await
}

#[tokio::test]
async fn autonomous_agent_authoring_one_typed_mcp_agent_runs_in_two_workflows_and_views_retain_revisions()
 {
    let server = mcp_server(Call::Json).await;
    let (f, agent) = published_mcp_agent(&server).await;
    let old = publish_typed_mcp(
        &f,
        agent,
        "authoring MCP revision one",
        McpDraftAssignment::Fresh,
    )
    .await;
    let first = claim_without_slots(&f).await;
    let second_fixture = Fixture {
        _db: f._db.clone(),
        persistence: f.persistence.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..f.target
        },
    };
    let second = claim_without_slots(&second_fixture).await;
    let new = publish_typed_mcp(
        &f,
        agent,
        "authoring MCP revision two",
        McpDraftAssignment::Retain,
    )
    .await;
    let third_scope = admit_again(&f, &first).await;
    assert_ne!(old.id, new.id);
    let mut llm = scripted_llm(
        (0..3)
            .flat_map(|_| {
                [
                    LlmTurn::tool_call(TOOL, json!({"customer_id":"c-42"})),
                    LlmTurn::text("\"Ada\""),
                ]
            })
            .collect(),
    )
    .await;
    register_scripted_agent_base_url(agent.as_uuid(), &llm.base_url);
    let runtime = mcp_agent_runtime(
        &f.persistence,
        Arc::new(McpRuntime::new(
            f.persistence.clone(),
            guarded_client(&server.url),
            lease(),
        )),
    );
    let mut versions = Vec::new();
    for selected_claim in [&first, &second] {
        versions.push(Box::pin(execute_and_show(&f, &runtime, selected_claim, old.id)).await);
    }
    // The third root was already admitted on v2. Finish the old leases before claiming it,
    // honoring the company's existing two-worker capacity rather than raising the limit.
    let third = claim(&f.persistence, third_scope).await;
    versions.push(Box::pin(execute_and_show(&f, &runtime, &third, new.id)).await);
    assert_ne!(versions[0], versions[1]);
    assert_eq!(
        versions[0], versions[2],
        "agent publication does not republish the first workflow"
    );
    assert_eq!(server.calls(), 3);
    let observed = llm.observed();
    assert!(
        observed[0]
            .to_string()
            .contains("authoring MCP revision one")
    );
    assert!(
        observed[2]
            .to_string()
            .contains("authoring MCP revision one")
    );
    assert!(
        observed[4]
            .to_string()
            .contains("authoring MCP revision two")
    );
    assert_eq!(llm.finish().await.unwrap(), 6);
}

async fn assert_mcp_view(
    f: &Fixture,
    claim: &ClaimedWorkflow,
    selected: crate::entities::agent_capability::AgentRevisionId,
) -> VersionId {
    let scope = RunReadScope {
        company: f.target.company,
        run: claim.fence.scope.run,
        viewer: f.target.actor,
    };
    let view = f.persistence.run_view(scope).await.unwrap().unwrap();
    let dependencies = view.dependencies.as_ref().unwrap();
    assert_eq!(dependencies.agents[0].revision_id, selected);
    assert_eq!(view.calls.items[0].origin.revision, selected);
    assert_eq!(
        view.calls.items[0].tool.target,
        dependencies.agents[0].bindings[0].target
    );
    let bundle = f
        .persistence
        .run_bundle(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap();
    assert!(bundle.compiled().graph().definition().resources.is_empty());
    let safe = format!("{:?}", dependencies);
    assert!(
        !safe.contains(TOKEN)
            && !safe.contains("scripted-key")
            && !safe.contains("authoring MCP revision")
    );
    view.detail.summary.version
}

async fn execute_and_show(
    f: &Fixture,
    runtime: &WorkflowAgentRuntime<PostgresPersistence, AgentActionTools>,
    claim: &ClaimedWorkflow,
    selected: crate::entities::agent_capability::AgentRevisionId,
) -> VersionId {
    // Box provider execution under the stock 2 MiB test stack.
    let output = Box::pin(runtime.execute(&WorkflowStepKind("agent.run".into()), claim))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(output, json!("Ada"));
    f.persistence
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap()
        .expect("the native completion owns the claim");
    Box::pin(assert_mcp_view(f, claim, selected)).await
}
