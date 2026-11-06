use super::*;
use crate::application::agent::{AgentGrantIdentity, AgentPublicationScope, AgentPublications};
use crate::application::workflow::agent::{AgentToolDispatch, AgentToolObservation};
use async_trait::async_trait;

#[tokio::test]
async fn autonomous_agent_recovery_mcp_cancellation_closes_session_and_preserves_one_possible_effect()
 {
    let server = mcp_server(Call::Hang).await;
    let fixture = fixture().await;
    let binding = registered(&fixture, &server, "cancelled").await;
    let run = agent_run(&fixture, "caller", &binding).await;
    let call = prepare_call(&fixture, &run, &binding).await;
    let runtime = McpRuntime::new(
        fixture.persistence.clone(),
        guarded_client(&server.url),
        lease(),
    );
    let cancel = CancellationToken::new();
    let cancelled = async {
        server.state.arrived.notified().await;
        cancel.cancel();
    };
    let (outcome, ()) = tokio::join!(
        Box::pin(runtime.call_tool(run.claim.fence, copy_agent_call(&call), &cancel)),
        cancelled
    );
    assert!(matches!(
        outcome.unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    // rmcp sends cancellation after the in-flight POST has released its transport.
    server.state.release.notify_one();
    server.observed("POST notifications/cancelled").await;
    server.observed("DELETE").await;
    let before = server.log();
    assert!(matches!(
        runtime
            .call_tool(run.claim.fence, call, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    assert_eq!(server.log(), before);
    assert_eq!(server.calls(), 1);
    assert_eq!(action_counts(&fixture.persistence).await, counts(1, 1, 0));
}

#[tokio::test]
async fn autonomous_agent_authority_mcp_binding_revocation_during_discovery_has_no_effect() {
    let server = mcp_server(Call::Json).await;
    let fixture = fixture().await;
    let binding = registered(&fixture, &server, "revoked").await;
    let run = agent_run(&fixture, "caller", &binding).await;
    let call = prepare_call(&fixture, &run, &binding).await;
    let McpToolTarget::Agent { origin, .. } = &call.target else {
        panic!("agent origin")
    };
    let scope = AgentPublicationScope {
        company: fixture.target.company,
        actor: fixture.target.actor,
        agent: origin.agent,
    };
    let grant = AgentGrantIdentity::Binding(origin.binding);
    server.hang_list(true);
    let runtime = McpRuntime::new(
        fixture.persistence.clone(),
        guarded_client(&server.url),
        lease(),
    );
    let cancel = CancellationToken::new();
    let revoke = async {
        server.state.arrived.notified().await;
        tokio::time::timeout(
            Duration::from_secs(1),
            fixture.persistence.revoke_agent_grant(scope, grant),
        )
        .await
        .expect("revocation cannot wait on a remote discovery lock")
        .unwrap();
        server.state.release.notify_one();
    };
    let (result, ()) = tokio::join!(
        Box::pin(runtime.call_tool(run.claim.fence, call, &cancel)),
        revoke
    );
    assert!(result.is_err());
    server.observed("DELETE").await;
    assert_eq!(server.calls(), 0);
    assert_eq!(action_counts(&fixture.persistence).await, counts(0, 0, 0));
}

#[tokio::test]
async fn autonomous_agent_authority_mcp_credential_rotation_before_open_keeps_binding_and_replay() {
    let server = mcp_server(Call::Json).await;
    let fixture = fixture().await;
    let binding = registered(&fixture, &server, "rotated").await;
    let run = agent_run(&fixture, "caller", &binding).await;
    let call = prepare_call(&fixture, &run, &binding).await;
    let current = connection_on(
        &fixture.persistence,
        fixture.target.company.as_uuid(),
        binding.connection,
    )
    .await;
    fixture
        .persistence
        .replace_mcp_token(
            fixture.target.company.as_uuid(),
            binding.connection,
            current.revision,
            Some(SecretString::from("rotated-agent-secret")),
        )
        .await
        .unwrap();
    let runtime = McpRuntime::new(
        fixture.persistence.clone(),
        guarded_client(&server.url),
        lease(),
    );
    let cancel = CancellationToken::new();
    assert!(matches!(
        runtime
            .call_tool(run.claim.fence, copy_agent_call(&call), &cancel)
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert!(matches!(
        runtime
            .call_tool(run.claim.fence, call, &cancel)
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(server.calls(), 1);
    assert_eq!(server.call_names(), vec![TOOL.to_owned()]);
    assert!(!server.auth().is_empty());
    assert!(
        server
            .auth()
            .iter()
            .all(|auth| auth == "Bearer rotated-agent-secret")
    );
    assert_eq!(action_counts(&fixture.persistence).await, counts(1, 1, 1));
}

async fn prepare_call(fixture: &Fixture, run: &AgentRun, binding: &McpBinding) -> McpToolCall {
    let llm = scripted_llm(vec![LlmTurn::tool_call(
        DECLARED,
        json!({"customer_id":"pending"}),
    )])
    .await;
    register_scripted_agent_base_url(run.agent, &llm.base_url);
    let runtime = WorkflowAgentRuntime::new(
        ProviderRegistry::standard().unwrap(),
        fixture.persistence.clone(),
        PreparedOnly,
        None,
        lease(),
    );
    let kind = WorkflowStepKind("agent.run".into());
    assert!(
        Box::pin(runtime.execute(&kind, &run.claim))
            .await
            .completed_result()
            .is_err()
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
    let call = saved_call(&fixture.persistence, &run.claim).await;
    McpToolCall {
        target: McpToolTarget::Agent {
            connection: binding.connection,
            origin: call.origin,
        },
        tool: TypeName::parse(TOOL).unwrap(),
        call: call.call,
        arguments: call.arguments,
    }
}

struct PreparedOnly;

fn copy_agent_call(call: &McpToolCall) -> McpToolCall {
    let McpToolTarget::Agent { connection, origin } = &call.target else {
        panic!("an explicit agent target is required")
    };
    McpToolCall {
        target: McpToolTarget::Agent {
            connection: *connection,
            origin: *origin,
        },
        tool: call.tool.clone(),
        call: call.call.clone(),
        arguments: call.arguments.clone(),
    }
}

#[async_trait]
impl AgentToolDispatch for PreparedOnly {
    async fn dispatch(
        &self,
        _: crate::application::workflow::lease::WorkflowFence,
        _: &PreparedTool,
        _: ModelToolCallId,
        _: Value,
    ) -> AppResult<AgentToolObservation> {
        Err(AppError::ServiceUnavailable("Saved MCP invocation".into()))
    }
}

#[path = "autonomous_agent_e2e/mcp_effects.rs"]
mod e2e;
