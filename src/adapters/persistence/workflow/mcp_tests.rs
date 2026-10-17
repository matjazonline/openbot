//! 04.10 MCP: real publish → bind → admit → claim → `mcp.call` against a recording MCP
//! server, plus the race/recovery matrix of decision 04.10-D2/D3.
use super::action_outcome_support::*;
use super::*;
use crate::adapters::{
    mcp::{HttpMcpClient, policy::EndpointPolicy},
    persistence::credentials::CredentialCipher,
    workflow_mcp::McpRuntime,
    workflow_source::WorkflowSourceDecoder,
};
use crate::application::use_cases::mcp::{
    McpConnectionWrite, McpCredentialPersistence, McpPersistence,
};
use crate::application::workflow::{
    actions::*,
    activation::{ActivationRequest, WorkflowJobId},
    lease::*,
    mcp::*,
    polling::{PollCandidate, PollPolicy, PollWork, WorkflowStepKind},
    worker::WorkflowWorker,
};
use crate::domain::entities::correlation::CorrelationId;
use crate::entities::mcp::{CompanyMcpConnection, McpAuth, McpDiscoveredTool};
use secrecy::SecretString;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use tokio_util::sync::CancellationToken;

#[path = "mcp_test_support.rs"]
mod support;
use support::*;

#[path = "mcp_reconciliation_tests.rs"]
mod reconciliation_tests;

fn code(failure: &WorkflowFailure) -> &str {
    failure.failure.code().as_str()
}

#[tokio::test]
async fn workflow_mcp_json_and_sse_run_end_to_end_with_typed_refs() {
    for call in [Call::Json, Call::Sse] {
        let setup = Box::pin(mcp_setup(call)).await;
        let p = &setup.fixture.persistence;
        let scope = admitted(&setup, "mcp-e2e").await;
        let runtime = setup.runtime();
        let worker = WorkflowWorker {
            port: p,
            handler: &runtime,
            worker: WorkflowWorkerId(Uuid::new_v4()),
            lease: lease(),
            poll: PollPolicy::new(Duration::from_millis(50), 16).unwrap(),
        };
        let shutdown = CancellationToken::new();
        worker
            .process(
                PollCandidate {
                    scope,
                    work: PollWork::Job,
                },
                &shutdown,
            )
            .await
            .unwrap();
        // The typed argument reference resolved from the run input; the bearer credential
        // travelled only on the wire.
        assert_eq!(
            setup.server.arguments(),
            vec![json!({"customer_id":"c-42"})]
        );
        assert!(
            setup
                .server
                .auth()
                .iter()
                .all(|auth| auth == &format!("Bearer {TOKEN}"))
        );
        assert_eq!(setup.server.calls(), 1, "{call:?}");
        let output: Value =
            sqlx::query_scalar("SELECT committed_output FROM workflow_executions WHERE id=$1")
                .bind(scope.execution.as_uuid())
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(
            output,
            json!({"content":[{"type":"text","text":"Ada"}],"structuredContent":{"name":"Ada","tier":3},"isError":false}),
            "protocol _meta is not part of the committed envelope"
        );
        let next = job_scope(p, setup.company(), scope.run, "result").await;
        let pending: bool =
            sqlx::query_scalar("SELECT status='pending' FROM background_tasks WHERE id=$1")
                .bind(next.job.0)
                .fetch_one(p.pool())
                .await
                .unwrap();
        if pending {
            worker
                .process(
                    PollCandidate {
                        scope: next,
                        work: PollWork::Job,
                    },
                    &shutdown,
                )
                .await
                .unwrap();
        }
        let (state, mapped): (String, Value) = sqlx::query_as("SELECT run.state, execution.committed_output FROM workflow_runs AS run JOIN workflow_executions AS execution ON execution.run_id=run.id AND execution.id=$2 WHERE run.id=$1")
            .bind(scope.run.as_uuid())
            .bind(next.execution.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(mapped, json!({"name":"Ada","tier":3}));
        assert_eq!(state, "succeeded");
        let stored: String =
            sqlx::query_scalar("SELECT string_agg(result::text, '') FROM workflow_action_receipts")
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert!(!stored.contains(TOKEN));
    }
}

#[tokio::test]
async fn workflow_mcp_competitors_restart_and_direct_tool_share_one_call() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let p = &setup.fixture.persistence;
    let claim = claimed(&setup).await;
    let runtime = setup.runtime();
    let step = kind();
    let (a, b) = tokio::join!(runtime.run(&step, &claim), runtime.run(&step, &claim));
    assert!(a.is_ok() || b.is_ok());
    assert_eq!(
        setup.server.calls(),
        1,
        "competing claimants: one tools/call"
    );
    assert_eq!(action_counts(p).await, counts(1, 1, 1));
    let seen = setup.server.log().len();
    // Restart: a fresh runtime whose client may not even reach the endpoint.
    let restarted = McpRuntime::new(
        p.clone(),
        Arc::new(HttpMcpClient::new(EndpointPolicy::default())),
        lease(),
    );
    let saved = restarted.run(&kind(), &claim).await.unwrap();
    assert_eq!(saved["structuredContent"]["name"], "Ada");
    // The same logical invocation through the direct agent tool path shares the receipt.
    let RemoteDispatchObservation::Committed(direct) = restarted
        .call_tool(claim.fence, tool_call("model-1"), &CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("committed receipt");
    };
    assert_eq!(direct.result, saved);
    assert_eq!(
        setup.server.log().len(),
        seen,
        "committed results make no MCP request"
    );
    // A changed argument set is a new invocation and needs a new model call id.
    let mut changed = tool_call("model-1");
    changed.arguments = json!({"customer_id":"c-43"});
    assert!(
        runtime
            .call_tool(claim.fence, changed, &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(
        setup.server.calls(),
        1,
        "reused model call id with new arguments is refused"
    );
    let mut fresh = tool_call("model-2");
    fresh.arguments = json!({"customer_id":"c-43"});
    assert!(matches!(
        runtime
            .call_tool(claim.fence, fresh, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(setup.server.calls(), 2);
    assert_eq!(action_counts(p).await, counts(2, 2, 2));
    let mappings: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_model_calls")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(mappings, 2);
}

#[tokio::test]
async fn workflow_mcp_tool_error_commits_receipt_and_takes_final_error() {
    let setup = Box::pin(mcp_setup(Call::ToolError)).await;
    let p = &setup.fixture.persistence;
    let claim = claimed(&setup).await;
    let runtime = setup.runtime();
    let failure = runtime.run(&kind(), &claim).await.unwrap_err();
    assert_eq!(code(&failure), "mcp.tool_error");
    assert_eq!(action_counts(p).await, counts(1, 1, 1));
    let receipt: Value = sqlx::query_scalar("SELECT result FROM workflow_action_receipts")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(
        receipt,
        json!({"content":[{"type":"text","text":"customer locked"}],"isError":true}),
        "bounded diagnostics; an error carries no structured output"
    );
    // Direct-tool parity: same invocation, classified tool error, no new call.
    let RemoteDispatchObservation::Committed(direct) = runtime
        .call_tool(claim.fence, tool_call("model-1"), &CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("committed receipt");
    };
    assert!(mcp_result_is_error(&direct.result));
    assert_eq!(setup.server.calls(), 1);
    assert_eq!(
        retire(p, &claim, lease(), failure).await,
        Retired::new("failed", None, "mcp.tool_error", "terminal", "safe", 0)
    );
}

#[tokio::test]
async fn workflow_mcp_resource_links_stay_inert_and_echoed_secrets_are_redacted() {
    for call in [Call::Link, Call::Echo] {
        let setup = Box::pin(mcp_setup(call)).await;
        let p = &setup.fixture.persistence;
        let claim = claimed(&setup).await;
        let result = setup.runtime().run(&kind(), &claim).await.unwrap();
        match call {
            Call::Link => {
                assert_eq!(result["content"][0]["type"], "resource_link");
                assert_eq!(result["isError"], false, "absent isError is false");
                tokio::time::sleep(Duration::from_millis(100)).await;
                assert_eq!(setup.server.count("GET /linked"), 0, "never fetched");
            }
            _ => {
                let text = result.to_string();
                assert!(
                    !text.contains(TOKEN) && text.contains("[REDACTED]"),
                    "{text}"
                );
                let stored: String =
                    sqlx::query_scalar("SELECT result::text FROM workflow_action_receipts")
                        .fetch_one(p.pool())
                        .await
                        .unwrap();
                assert!(!stored.contains(TOKEN));
            }
        }
    }
}

#[tokio::test]
async fn workflow_mcp_output_violation_and_oversize_stay_unknown_without_second_call() {
    for call in [Call::BadStructured, Call::Oversize] {
        let setup = Box::pin(mcp_setup(call)).await;
        let p = &setup.fixture.persistence;
        let claim = claimed(&setup).await;
        let runtime = setup.runtime();
        let failure = runtime.run(&kind(), &claim).await.unwrap_err();
        assert_eq!(code(&failure), "mcp.unresolved", "{call:?}");
        assert_eq!(action_counts(p).await, counts(1, 1, 0));
        let seen = setup.server.log().len();
        assert!(runtime.run(&kind(), &claim).await.is_err());
        assert_eq!(
            setup.server.log().len(),
            seen,
            "possible dispatch: no MCP request"
        );
        assert_eq!(
            retire(p, &claim, lease(), failure).await,
            Retired::new(
                "waiting",
                Some("reconciliation"),
                "mcp.unresolved",
                "retryable",
                "unknown",
                1
            )
        );
    }
}

#[tokio::test]
async fn workflow_mcp_drift_and_missing_tool_fail_before_any_marker() {
    let drifted =
        json!([{"name":TOOL,"inputSchema":{"type":"object"},"outputSchema":output_schema()}]);
    for tools in [drifted, json!([])] {
        let setup = Box::pin(mcp_setup(Call::Json)).await;
        let p = &setup.fixture.persistence;
        setup.server.set_tools(tools);
        let claim = claimed(&setup).await;
        let failure = setup.runtime().run(&kind(), &claim).await.unwrap_err();
        assert_eq!(code(&failure), "mcp.contract_changed");
        assert_eq!(setup.server.calls(), 0);
        assert_eq!(setup.server.count("POST tools/list"), 1);
        assert_eq!(action_counts(p).await, counts(0, 0, 0));
        assert_eq!(
            retire(p, &claim, lease(), failure).await,
            Retired::new(
                "failed",
                None,
                "mcp.contract_changed",
                "terminal",
                "safe",
                0
            )
        );
    }
}

#[tokio::test]
async fn workflow_mcp_unavailable_before_marker_retries_and_dispatches_once() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let p = &setup.fixture.persistence;
    setup.server.fail_initialize(true);
    let scope = admitted(&setup, "mcp-unavailable").await;
    let first = claim(p, scope).await;
    let failure = setup.runtime().run(&kind(), &first).await.unwrap_err();
    assert_eq!(code(&failure), "mcp.unavailable");
    assert_eq!(action_counts(p).await, counts(0, 0, 0));
    assert_eq!(
        retire(p, &first, lease(), failure).await,
        Retired::new("running", None, "mcp.unavailable", "retryable", "safe", 0)
    );
    setup.server.fail_initialize(false);
    let second = reclaim(p, scope).await;
    assert!(setup.runtime().run(&kind(), &second).await.is_ok());
    assert_eq!(setup.server.calls(), 1);
    assert_eq!(action_counts(p).await, counts(1, 1, 1));
}

#[tokio::test]
async fn workflow_mcp_refusals_and_protection_are_terminal_without_network() {
    for change in [
        "disabled",
        "token",
        "operation",
        "grant",
        "member",
        "approval",
    ] {
        let setup = Box::pin(mcp_setup(Call::Json)).await;
        let p = &setup.fixture.persistence;
        let company = setup.company();
        let claim = claimed(&setup).await;
        let current = setup.current().await;
        match change {
            "disabled" | "grant" => {
                let mut write = connection_write(&setup.server.url);
                if change == "disabled" {
                    write.enabled = false;
                } else {
                    write.tool_grants.clear();
                }
                p.update_mcp_connection(company.as_uuid(), current.id, current.revision, write)
                    .await
                    .unwrap();
            }
            "token" => {
                p.replace_mcp_token(company.as_uuid(), current.id, current.revision, None)
                    .await
                    .unwrap();
            }
            "operation" => {
                p.revoke_mcp_operation(
                    company,
                    setup.actor(),
                    McpOperationRevoke {
                        connection: current.id,
                        catalog_revision: current.revision,
                        name: TypeName::parse(TOOL).unwrap(),
                        version: setup.operation,
                    },
                )
                .await
                .unwrap();
            }
            "approval" => {
                let mut protected = operation(company);
                protected.approval_required = true;
                p.register_mcp_operation(
                    company,
                    setup.actor(),
                    McpOperationWrite {
                        connection: current.id,
                        catalog_revision: current.revision,
                        expected_version: Some(setup.operation),
                        operation: protected,
                    },
                )
                .await
                .unwrap();
            }
            _ => {
                sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
                    .bind(company.as_uuid())
                    .bind(setup.actor().user_id())
                    .execute(p.pool())
                    .await
                    .unwrap();
            }
        }
        let runtime = setup.runtime();
        // Direct-tool parity first (it changes nothing when refused or protected).
        let direct = runtime
            .call_tool(claim.fence, tool_call("model-1"), &CancellationToken::new())
            .await;
        if change == "approval" {
            assert!(matches!(
                direct,
                Ok(RemoteDispatchObservation::ApprovalRequired)
            ));
        } else {
            assert!(direct.is_err(), "{change}");
        }
        let failure = runtime.run(&kind(), &claim).await.unwrap_err();
        let expected = if change == "approval" {
            "mcp.approval_required"
        } else {
            "mcp.rejected"
        };
        assert_eq!(code(&failure), expected, "{change}");
        assert!(setup.server.log().is_empty(), "{change}: no MCP request");
        assert_eq!(action_counts(p).await, counts(0, 0, 0), "{change}");
        assert_eq!(
            retire(p, &claim, lease(), failure).await,
            Retired::new("failed", None, expected, "terminal", "safe", 0),
            "{change}"
        );
    }
}

#[path = "mcp_catalog_tests.rs"]
mod catalog_tests;
#[path = "mcp_recovery_tests.rs"]
mod recovery_tests;
