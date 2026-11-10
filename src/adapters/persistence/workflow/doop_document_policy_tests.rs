use super::*;
use crate::application::workflow::{
    agent::AgentPreparation,
    checkpoints::{
        WorkflowCheckpointSpec, WorkflowCheckpointStore, WorkflowCheckpointUpdate,
        WorkflowCheckpointWrite,
    },
    worker::WorkflowHandler,
};
use crate::services::{
    harness::{
        AgentExecutionOutput,
        runs::{RunCheckpoint, WriteOutcome},
    },
    response_contract::StructuredResponse,
    test_support::{LlmTurn, ScriptedLlm, register_scripted_agent_base_url, scripted_llm},
};

struct FailedEditor {
    claim: ClaimedWorkflow,
    spec: WorkflowCheckpointSpec,
    checkpoint: RunCheckpoint,
    llm: ScriptedLlm,
}

fn turns() -> Vec<LlmTurn> {
    vec![
        LlmTurn::tool_call("get_canvas", json!({"canvas_id":"canvas-a"})),
        LlmTurn::tool_call(
            "edit_frame_html",
            json!({"frame_id":"frame-a","old_str":"old","new_str":"new","agent_name":"Openbot document editor"}),
        ),
    ]
}

async fn failed_editor(d: &Document) -> FailedEditor {
    d.server.set_tool_result(
        tool_name("edit_frame_html"),
        json!({"content":[{"type":"text","text":"old_str not found"}],"isError":true}),
    );
    let llm = scripted_llm(turns()).await;
    register_scripted_agent_base_url(d.agent, &llm.base_url);
    let scope = d.runtime().admit(input()).await.unwrap();
    d.drive(scope, |_, decisions| {
        !decisions.is_empty()
            && d.server
                .call_names()
                .iter()
                .filter(|n| n.as_str() == "get_canvas")
                .count()
                == 2
    })
    .await;
    for decision in d.runtime().approvals(scope).await {
        d.runtime().approve(scope, decision).await;
    }
    let p = &d.f.persistence;
    let activation = job_scope(p, scope.company, scope.run, "edit").await;
    let claim = claim(p, activation).await;
    let kind = WorkflowStepKind("agent.run".into());
    let first = Box::pin(d.engine.handler().execute(&kind, &claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(code(&first), "agent.mcp_tool_failed");
    let launch = p
        .prepare_agent(&claim, lease())
        .await
        .unwrap()
        .expect_ready();
    let spec = launch.prepared.spec(claim.fence).unwrap();
    let opened = p
        .open_workflow_checkpoint(claim.fence, &spec, lease())
        .await
        .unwrap();
    let checkpoint = match opened {
        WriteOutcome::Applied(checkpoint) | WriteOutcome::AlreadyApplied(checkpoint) => checkpoint,
        other => panic!("owned checkpoint: {other:?}"),
    };
    assert!(
        checkpoint
            .invocations
            .iter()
            .any(|i| i.result.as_ref().is_some_and(|r| r["isError"] == true))
    );
    assert!(checkpoint.final_output.is_none());
    FailedEditor {
        claim,
        spec,
        checkpoint,
        llm,
    }
}

fn forged_success(spec: &WorkflowCheckpointSpec) -> WorkflowCheckpointUpdate {
    let structured = StructuredResponse::validate(
        spec.identity.response_contract.as_ref().unwrap(),
        &json!({"canvas_id":"canvas-a","status":"draft","summary":"Forged saved success"})
            .to_string(),
        None,
        &crate::adapters::response_schema::JsonResponseValidator,
    )
    .unwrap()
    .unwrap();
    WorkflowCheckpointUpdate::FinalOutput(Box::new(AgentExecutionOutput {
        content: structured.body().into(),
        structured: Some(structured),
        metadata: None,
        token_usage: Default::default(),
    }))
}

#[tokio::test]
async fn doop_agent_saved_mcp_error_refuses_restart_and_saved_final_without_provider_or_write() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    // Box the genuine database/agent/provider setup seam, keeping test stack use bounded.
    let failed = Box::pin(failed_editor(&d)).await;
    let before = d.server.call_names();
    let kind = WorkflowStepKind("agent.run".into());
    let replay = Box::pin(d.engine.handler().execute(&kind, &failed.claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(code(&replay), "agent.mcp_tool_failed");
    // Test-only contradictory final exercises the saved-final fast path separately
    // from the ordinary drive loop guard. It is never produced by that guarded loop.
    let saved =
        d.f.persistence
            .update_workflow_checkpoint(
                WorkflowCheckpointWrite {
                    fence: failed.claim.fence,
                    checkpoint: failed.checkpoint.run_id,
                    expected_revision: failed.checkpoint.revision,
                    lease_policy: lease(),
                },
                forged_success(&failed.spec),
            )
            .await
            .unwrap();
    assert!(matches!(saved, WriteOutcome::Applied(_)));
    let final_replay = Box::pin(d.engine.handler().execute(&kind, &failed.claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(code(&final_replay), "agent.mcp_tool_failed");
    assert_eq!(d.server.call_names(), before);
    assert_eq!(
        before
            .iter()
            .filter(|n| n.as_str() == "edit_frame_html")
            .count(),
        1
    );
    assert_eq!(failed.llm.finish().await.unwrap(), 2);
    d.client.shutdown().await.unwrap();
}
