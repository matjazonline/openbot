//! A registered workflow tool executes MCP before the parent consumes its result.
use super::*;
use crate::adapters::persistence::{
    credentials::CredentialCipher,
    workflow::{mcp, tests::mcp_tests::support as remote},
};
use crate::adapters::workflow_mcp::McpRuntime;
use crate::application::workflow::{
    activation::ActivationRequest,
    lifecycle::{ConfigureBinding, SetBindingActivity},
    polling::{PollCandidate, PollPolicy},
    worker::WorkflowWorker,
};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

fn directory(fixture: &Fixture, connection: Uuid) -> mcp::McpPublicationDirectory {
    mcp::McpPublicationDirectory {
        persistence: fixture.persistence.clone(),
        selections: vec![mcp::McpPublicationSelection {
            connection,
            slot: ResourceName::parse("service").unwrap(),
            tool: TypeName::parse(remote::TOOL).unwrap(),
        }],
    }
}

async fn setup(server: &remote::McpServer) -> ToolSetup {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let (connection, _) = Box::pin(remote::configured(&fixture, &server.url)).await;
    let mut child = echo_child();
    let workflow = WorkflowId::new(Uuid::from_u128(CHILD));
    child["workflow_id"] = json!(workflow.to_string());
    child["resources"] = json!([{"slot":"service","kind":"mcp"}]);
    child["entry"] = json!("lookup");
    child["output_schema"] = json!({"type":"object"});
    child["steps"] = json!({"lookup":{"type":"mcp.call","with":{
        "connection":{"literal":"service"},"tool":{"literal":remote::TOOL},
        "arguments":{"literal":{"customer_id":"c-42"}}
    },"routes":{"success":"$end"}}});
    let version = publish_through(
        &fixture,
        workflow,
        "binary-mcp-child",
        child.to_string(),
        directory(&fixture, connection.id),
    )
    .await
    .unwrap();
    let reviewer = fixture.target.actor.user_id();
    let mut setup = Box::pin(ToolSetup::bind_child(fixture, reviewer, version)).await;
    setup.target = bind_parent(&setup, connection.id).await;
    setup
}

async fn bind_parent(setup: &ToolSetup, connection: Uuid) -> BindingTarget {
    let p = setup.persistence();
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["resources"] = json!([{"slot":"service","kind":"mcp"}]);
    let version = publish_parent_with(
        &setup.fixture,
        "binary-parent-resource",
        source.to_string(),
        directory(&setup.fixture, connection),
    )
    .await
    .unwrap();
    let target = BindingTarget {
        binding: WorkflowBindingId::new(Uuid::new_v4()),
        ..setup.target
    };
    let service = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let state = service
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: BTreeMap::from([(
                ResourceName::parse("service").unwrap(),
                RuntimeResourceId::new(connection),
            )]),
            run_as: None,
            message_trigger: None,
        })
        .await
        .unwrap();
    service
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    target
}

async fn execute_child(setup: &ToolSetup, server: &remote::McpServer, child: ActivationRequest) {
    let runtime = McpRuntime::new(
        setup.persistence().clone(),
        remote::guarded_client(&server.url),
        lease(),
    );
    let worker = WorkflowWorker {
        port: setup.persistence(),
        handler: &runtime,
        notices: None,
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: lease(),
        poll: PollPolicy::new(std::time::Duration::from_millis(50), 16).unwrap(),
        active_jobs: None,
        monitoring: None,
    };
    for _ in 0..2 {
        worker
            .process(
                PollCandidate {
                    scope: child,
                    work: PollWork::Job,
                },
                &CancellationToken::new(),
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn doop_binary_mcp_child_actual_action_completion_error_and_parent_replay() {
    for (pdf, failed) in [(false, false), (true, false), (true, true)] {
        child_case(pdf, failed).await;
    }
}

async fn child_case(pdf: bool, failed: bool) {
    let mut raw: Value = serde_json::from_str(if pdf {
        include_str!("../../mcp/fixtures/doop/pdf.json")
    } else {
        include_str!("../../mcp/fixtures/doop/screenshot.json")
    })
    .unwrap();
    raw.as_object_mut().unwrap().remove("structuredContent");
    raw["isError"] = json!(failed);
    let bytes = if pdf {
        raw["content"][0]["resource"]["blob"].as_str().unwrap()
    } else {
        raw["content"][0]["data"].as_str().unwrap()
    };
    let server = remote::mcp_server(remote::Call::Json).await;
    server.set_result(raw.clone());
    let setup = Box::pin(setup(&server)).await;
    let p = setup.persistence();
    let claim = setup.claim("real-mcp-child").await;
    let scope = claim.fence.scope;
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED, json!({"question":"inspect"})),
        LlmTurn::text("\"No visual review\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = setup.runtime();
    assert!(matches!(
        Box::pin(runtime.execute(&kind(), &claim)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let child = setup.child_of(scope).await;
    execute_child(&setup, &server, child).await;
    assert_child_output(p, child, &raw, failed).await;
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    ));
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        Box::pin(runtime.execute(&kind(), &continued)).await,
        WorkflowHandlerOutcome::Completed(Ok(_))
    ));
    let saved = saved_result(p, scope).await;
    if failed {
        assert_eq!(saved["error"]["code"], CHILD_FAILED);
    } else {
        assert_eq!(saved["_meta"]["binaryContentOmitted"], true);
    }
    let messages = llm.observed()[1]["messages"].to_string();
    assert!(!messages.contains(bytes));
    assert!(messages.contains(if failed {
        CHILD_FAILED
    } else {
        "Visual inspection was not performed"
    }));
    assert!(matches!(
        Box::pin(runtime.execute(&kind(), &continued)).await,
        WorkflowHandlerOutcome::Completed(Ok(_))
    ));
    assert_eq!(server.calls(), 1);
    assert_eq!(call_parked(p, scope).await.children, 1);
    assert_eq!(llm.finish().await.unwrap(), 2);
}

async fn assert_child_output(
    p: &PostgresPersistence,
    child: ActivationRequest,
    raw: &Value,
    failed: bool,
) {
    let receipt: Value =
        sqlx::query_scalar("SELECT result FROM workflow_action_receipts WHERE run_id=$1")
            .bind(child.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        &receipt, raw,
        "the actual MCP child action retains its raw bounded receipt"
    );
    let committed: Option<Value> =
        sqlx::query_scalar("SELECT committed_output FROM workflow_executions WHERE id=$1")
            .bind(child.execution.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    if failed {
        assert!(
            committed.is_none(),
            "tool errors never commit a successful child output"
        );
    } else {
        let committed = committed.unwrap();
        assert_eq!(committed["_meta"]["binaryContentOmitted"], true);
        assert!(
            !committed
                .to_string()
                .contains(if raw["content"][0]["type"] == "image" {
                    raw["content"][0]["data"].as_str().unwrap()
                } else {
                    raw["content"][0]["resource"]["blob"].as_str().unwrap()
                })
        );
    }
}
