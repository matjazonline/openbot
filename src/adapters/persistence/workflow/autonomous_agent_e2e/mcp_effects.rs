use super::*;
use crate::application::agent::{
    AgentAuthoringReads, AgentDraftPersistence, AgentToolAssignment, AgentToolChoice,
    PublishedAgentRevision, SaveCompanyAgent,
};
use crate::application::workflow::run_views::{RunReadScope, RunViews};
use crate::entities::agent_capability::AgentId;

async fn retarget(f: &Fixture, run: &AgentRun, binding: &McpBinding) -> PublishedAgentRevision {
    let scope = AgentPublicationScope {
        company: f.target.company,
        actor: f.target.actor,
        agent: AgentId::new(run.agent),
    };
    let state = f.persistence.authoring_state(scope).await.unwrap();
    f.persistence
        .save_company_agent(SaveCompanyAgent {
            scope,
            expected_revision: state.agent.draft_revision,
            write: AgentWrite {
                name: state.agent.name,
                slug: state.agent.slug,
                provider: state.agent.provider,
                model: state.agent.model,
                system_prompt: state.agent.system_prompt,
                ..Default::default()
            },
            tools: Some(vec![AgentToolAssignment {
                binding: None,
                alias: AgentToolAlias::parse(ALIAS).unwrap(),
                choice: AgentToolChoice::Mcp {
                    connection: binding.connection,
                    operation: TypeName::parse(TOOL).unwrap(),
                    reviewed_version: binding.reviewed,
                    remote_name: TOOL.to_owned().try_into().unwrap(),
                },
            }]),
        })
        .await
        .unwrap();
    f.publish_agent(run.agent).await
}

async fn next_admission(f: &Fixture, previous: &AgentRun) -> AgentRun {
    let p = &f.persistence;
    let company = f.target.company;
    let binding: Uuid =
        sqlx::query_scalar("SELECT binding_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid())
            .bind(previous.claim.fence.scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let result = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            company_id: company,
            actor: f.target.actor,
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                company,
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
        .unwrap();
    let AdmissionResult::Created(root) = result else {
        panic!("new root")
    };
    AgentRun {
        agent: previous.agent,
        claim: claim(p, job_scope(p, company, root, "start").await).await,
    }
}

async fn competing_dispatch_and_receipt_replay(
    f: &Fixture,
    run: &AgentRun,
    call: &McpToolCall,
    client: Arc<HttpMcpClient>,
) {
    let left = McpRuntime::new(f.persistence.clone(), client.clone(), lease());
    let right = McpRuntime::new(f.persistence.clone(), client.clone(), lease());
    let cancel = CancellationToken::new();
    let (a, b) = tokio::join!(
        left.call_tool(run.claim.fence, copy_agent_call(call), &cancel),
        right.call_tool(run.claim.fence, copy_agent_call(call), &cancel)
    );
    assert!(
        a.as_ref()
            .is_ok_and(|r| matches!(r, RemoteDispatchObservation::Committed(_)))
            || b.as_ref()
                .is_ok_and(|r| matches!(r, RemoteDispatchObservation::Committed(_)))
    );
    let restarted = McpRuntime::new(f.persistence.clone(), client, lease());
    assert!(matches!(
        restarted
            .call_tool(run.claim.fence, copy_agent_call(call), &cancel)
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(action_counts(&f.persistence).await, counts(1, 1, 1));
}

#[tokio::test]
async fn autonomous_agent_e2e_retargeted_alias_cannot_redirect_saved_call_and_dispatch_races_once()
{
    let old_server = mcp_server(Call::Json).await;
    let new_server = mcp_server(Call::Json).await;
    let f = fixture().await;
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let old = registered(&f, &old_server, "original").await;
    let run = agent_run(&f, "caller", &old).await;
    let saved = prepare_call(&f, &run, &old).await;
    let McpToolTarget::Agent {
        origin: original, ..
    } = &saved.target
    else {
        panic!("agent")
    };
    let original = *original;
    let replacement = registered(&f, &new_server, "replacement").await;
    let published = retarget(&f, &run, &replacement).await;
    assert_ne!(published.id, original.revision);
    let scope = AgentPublicationScope {
        company: f.target.company,
        actor: f.target.actor,
        agent: original.agent,
    };
    f.persistence
        .revoke_agent_grant(scope, AgentGrantIdentity::Binding(original.binding))
        .await
        .unwrap();
    let client = Arc::new(HttpMcpClient::new(
        EndpointPolicy::new([old_server.url.clone(), new_server.url.clone()]).unwrap(),
    ));
    let runtime = McpRuntime::new(f.persistence.clone(), client.clone(), lease());
    assert!(
        runtime
            .call_tool(run.claim.fence, saved, &CancellationToken::new())
            .await
            .is_err()
    );
    assert!(old_server.log().is_empty() && new_server.log().is_empty());
    let next = next_admission(&f, &run).await;
    let call = prepare_call(&f, &next, &replacement).await;
    let McpToolTarget::Agent { origin: fresh, .. } = &call.target else {
        panic!("agent")
    };
    assert_eq!(fresh.revision, published.id);
    assert_ne!(fresh.binding, original.binding);
    competing_dispatch_and_receipt_replay(&f, &next, &call, client).await;
    assert_eq!(old_server.calls(), 0);
    assert_eq!(new_server.calls(), 1);
    assert_eq!(new_server.call_names(), vec![TOOL.to_owned()]);
    let view = f
        .persistence
        .run_view(RunReadScope {
            company: f.target.company,
            run: run.claim.fence.scope.run,
            viewer: f.target.actor,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        view.dependencies.unwrap().agents[0].revision_id,
        original.revision
    );
}

#[derive(Clone, Copy, Debug)]
enum Refusal {
    ConnectionGrant,
    ReviewedPolicy,
    PeerSchema,
}

async fn change_authority(f: &Fixture, server: &McpServer, binding: &McpBinding, refusal: Refusal) {
    let p = &f.persistence;
    let company = f.target.company;
    let current = connection_on(p, company.as_uuid(), binding.connection).await;
    match refusal {
        Refusal::ConnectionGrant => {
            let mut write = connection_write(&server.url);
            write.tool_grants.clear();
            p.update_mcp_connection(company.as_uuid(), current.id, current.revision, write)
                .await
                .unwrap();
        }
        Refusal::ReviewedPolicy => {
            let mut reviewed = operation(company);
            reviewed.approval_required = true;
            let next = p
                .register_mcp_operation(
                    company,
                    f.target.actor,
                    McpOperationWrite {
                        connection: current.id,
                        catalog_revision: current.revision,
                        expected_version: Some(binding.reviewed),
                        operation: reviewed,
                    },
                )
                .await
                .unwrap();
            assert_ne!(next, binding.reviewed);
        }
        Refusal::PeerSchema => {
            let mut tools = listed_tools();
            tools[0]["inputSchema"]["properties"]["customer_id"]["type"] = json!("integer");
            server.set_tools(tools);
        }
    }
}

#[tokio::test]
async fn autonomous_agent_e2e_saved_calls_recheck_connection_grant_review_and_peer_schema() {
    for refusal in [
        Refusal::ConnectionGrant,
        Refusal::ReviewedPolicy,
        Refusal::PeerSchema,
    ] {
        let server = mcp_server(Call::Json).await;
        let f = fixture().await;
        let binding = registered(&f, &server, "reviewed").await;
        let run = agent_run(&f, "caller", &binding).await;
        let call = prepare_call(&f, &run, &binding).await;
        change_authority(&f, &server, &binding, refusal).await;
        let runtime = McpRuntime::new(f.persistence.clone(), guarded_client(&server.url), lease());
        assert!(
            runtime
                .call_tool(run.claim.fence, call, &CancellationToken::new())
                .await
                .is_err(),
            "{refusal:?}"
        );
        assert_eq!(server.calls(), 0, "{refusal:?}");
        assert_eq!(action_counts(&f.persistence).await, counts(0, 0, 0));
        if !matches!(refusal, Refusal::PeerSchema) {
            assert!(server.log().is_empty());
        }
    }
}
