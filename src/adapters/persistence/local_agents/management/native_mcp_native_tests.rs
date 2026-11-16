//! Opt-in local native CLI proof. All application, protocol and effect owners are real.
use super::*;
use crate::adapters::http::routes::local_machine_connection::tests::tls::TestTlsServer;
use crate::adapters::persistence::local_agents::test_support::mcp_workflow::{eventually, worker};
use crate::adapters::persistence::workflow::native_mcp_remote_fixture as remote;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
#[path = "native_mcp_native_peer.rs"]
mod peer;
use peer::NativePeer;
#[path = "native_mcp_reconciliation_tests.rs"]
mod reconciliation_tests;

async fn start_native(s: &mut Setup, peer: &mut NativePeer) -> OperationId {
    let ready = peer.read("ready.json").await;
    let project = ready["projectId"]
        .as_str()
        .unwrap()
        .to_owned()
        .try_into()
        .unwrap();
    eventually(async || {
        let live:bool=sqlx::query_scalar("SELECT COALESCE(capabilities_sequence>0 AND connection_expires_at>clock_timestamp(),false) FROM local_machines WHERE company_id=$1 AND id=$2")
            .bind(s.f.company).bind(s.profile.selection.machine_id.as_uuid()).fetch_one(&s.f.database.pool).await.unwrap();
        live.then_some(())
    }).await;
    s.f.cases
        .set_projects(
            s.f.owner,
            s.f.company,
            s.profile.selection.machine_id,
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
    s.profile.selection.project_id = project;
    let catalog:Value=sqlx::query_scalar("SELECT cap.catalog FROM local_machine_capabilities AS cap JOIN local_machines AS machine ON machine.company_id=cap.company_id AND machine.id=cap.machine_id AND machine.capabilities_sequence=cap.sequence WHERE machine.company_id=$1 AND machine.id=$2")
        .bind(s.f.company).bind(s.profile.selection.machine_id.as_uuid()).fetch_one(&s.f.database.pool).await.unwrap();
    let provider = catalog["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["provider"] == "claude-code")
        .unwrap();
    let model = provider["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["model"]
                .as_str()
                .is_some_and(|name| name.contains("sonnet"))
        })
        .expect("actual advertised Sonnet model");
    s.profile.selection.model = model["model"]
        .as_str()
        .unwrap()
        .to_owned()
        .try_into()
        .unwrap();
    s.profile.selection.policy.reasoning =
        serde_json::from_value(model["reasoning_levels"][0].clone()).unwrap();
    s.f.cases.save_profile(s.f.owner, &s.profile).await.unwrap();
    let mut request = s.request();
    request.prompt="NATIVE_COMPANY_MCP: list and describe the existing company CRM tool, then call it exactly once.".to_owned().try_into().unwrap();
    s.f.persistence.start_session(&request).await.unwrap();
    request.request_id
}

async fn broker(s: &Setup, operation: OperationId) -> (Uuid, crate::domain::workflow::RunId) {
    eventually(async || {
        sqlx::query_as::<_, (Uuid, Uuid)>(
            "SELECT id,run_id FROM native_mcp_calls WHERE company_id=$1 AND operation_id=$2",
        )
        .bind(s.f.company)
        .bind(operation.as_uuid())
        .fetch_optional(&s.f.database.pool)
        .await
        .unwrap()
        .map(|(call, run)| (call, crate::domain::workflow::RunId::new(run)))
    })
    .await
}

async fn approve(
    s: &Setup,
    run: crate::domain::workflow::RunId,
    remote: &remote::McpServer,
    hold: Duration,
) {
    crate::adapters::persistence::local_agents::test_support::mcp_workflow::approve(
        s.f.persistence.as_ref(),
        CompanyId::new(s.f.company),
        WorkflowActor::authenticated(s.f.owner).unwrap(),
        run,
        remote,
        hold,
    )
    .await;
}

async fn status(s: &Setup, operation: OperationId, call: Uuid) -> NativeMcpResponse {
    let invocation:Value=sqlx::query_scalar("SELECT jsonb_build_object('thread_id',native_thread_id,'turn_id',native_turn_id,'client_request_id',client_request_id,'model_call_id',model_call_id) FROM native_mcp_calls WHERE company_id=$1 AND id=$2")
        .bind(s.f.company).bind(call).fetch_one(&s.f.database.pool).await.unwrap();
    for attempt in 0..100 {
        let identity:Value=sqlx::query_scalar("SELECT jsonb_build_object('installation_id',installation_id,'credential_generation',credential_generation,'connection_generation',connection_generation) FROM local_machines WHERE company_id=$1 AND id=$2")
            .bind(s.f.company).bind(s.profile.selection.machine_id.as_uuid()).fetch_one(&s.f.database.pool).await.unwrap();
        let result =
            s.f.persistence
                .native_mcp(&NativeMcpRequest {
                    verifier: s.paired.candidate.credential.verifier(),
                    operation,
                    identity: serde_json::from_value(identity).unwrap(),
                    body: NativeMcpBody {
                        invocation: serde_json::from_value(invocation.clone()).unwrap(),
                        action: NativeMcpAction::Status {
                            call_id: NativeMcpCallId::new(call).unwrap(),
                        },
                    },
                })
                .await;
        match result {
            Err(AppError::InvalidCredentials | AppError::ServiceUnavailable(_)) if attempt < 99 => {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            result => return result.unwrap(),
        }
    }
    unreachable!()
}

pub(super) async fn allow_connection(s: &Setup) {
    use crate::application::workflow::connection_level::ConnectionLevelCommand;
    let (connection, revision): (Uuid, i64) = sqlx::query_as(
        "SELECT id,revision FROM company_mcp_connections WHERE company_id=$1 AND slug='native-crm'",
    )
    .bind(s.f.company)
    .fetch_one(&s.f.database.pool)
    .await
    .unwrap();
    let mut db = s.f.database.pool.acquire().await.unwrap();
    let (_, access_level) = crate::adapters::persistence::workflow::native_mcp::source_facts_on(
        &mut db,
        CompanyId::new(s.f.company),
        WorkflowActor::authenticated(s.f.owner).unwrap(),
        &crate::domain::local_agents::SessionAssociation::Conversation {
            conversation_id: s.conversation,
        },
    )
    .await
    .unwrap();
    drop(db);
    s.f.persistence
        .set_mcp_level(ConnectionLevelCommand {
            company: CompanyId::new(s.f.company),
            actor: WorkflowActor::authenticated(s.f.owner).unwrap(),
            connection,
            expected_revision: revision,
            access_level,
        })
        .await
        .unwrap();
}

async fn queue_status(
    s: &Setup,
    peer: &mut NativePeer,
    operation: OperationId,
    call: Uuid,
) -> OperationId {
    let session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(s.f.company)
    .bind(operation.as_uuid())
    .fetch_one(&s.f.database.pool)
    .await
    .unwrap();
    let command = s.control(
        SessionId::new(session).unwrap(),
        SessionCommand::Send {
            prompt: format!("NATIVE_COMPANY_MCP_STATUS: {call} Read the retained status only.")
                .try_into()
                .unwrap(),
        },
    );
    s.f.persistence.control_session(&command).await.unwrap();
    eventually(async || {
        let bytes = std::fs::read(peer.directory.join("connector.json")).ok()?;
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(record) = value["commands"]
            .as_array()?
            .iter()
            .find(|record| record["id"] == json!(command.request_id))
        {
            assert!(
                !matches!(record["stage"].as_str(), Some("uncertain" | "terminal")),
                "queued native B refused before its SDK receipt: {record}"
            );
        }
        let mapped = value["mappings"]
            .as_array()?
            .iter()
            .any(|mapping| mapping["operationId"] == json!(command.request_id));
        let queued = value["commands"].as_array()?.iter().any(|record| {
            record["id"] == json!(command.request_id)
                && record["sdkCalls"] == 1
                && record["queueId"].is_string()
        });
        (mapped && queued).then_some(())
    })
    .await;
    let owner: Uuid = sqlx::query_scalar(
        "SELECT operation_id FROM native_mcp_calls WHERE company_id=$1 AND id=$2",
    )
    .bind(s.f.company)
    .bind(call)
    .fetch_one(&s.f.database.pool)
    .await
    .unwrap();
    assert_eq!(owner, operation.as_uuid());
    command.request_id
}

#[tokio::test]
#[ignore = "requires the owned patched Cloudroom checkout, Node24 and local scripted native CLI fixture"]
async fn cloudroom_native_company_mcp_real_cli_plugin_pg_approval_worker_and_gateway() {
    let remote = remote::mcp_server(remote::Call::Hang).await;
    remote.set_tools(json!([{"name":"lookup","description":"Lookup","inputSchema":{"type":"object","required":["query"],"additionalProperties":false,"properties":{"query":{"type":"string","maxLength":128}}},"outputSchema":{"type":"object"}}]));
    remote.set_result(json!({"content":[{"type":"text","text":"Ada"}],"structuredContent":{"name":"Ada","tier":3},"isError":false}));
    let f = Fixture::new().await.expect("native MCP PostgreSQL fixture");
    let paired = f.pair(f.company).await;
    let profile = f.profile(paired.receipt.machine.id);
    let conversation = f.conversation(f.company).await;
    let mut s = prepare_agent(
        Setup {
            f,
            paired,
            profile,
            conversation,
        },
        &remote.url,
    )
    .await;
    allow_connection(&s).await;
    let server = TestTlsServer::start(s.f.persistence.clone()).await;
    let mut peer = NativePeer::spawn(&s, &server).await;
    let operation = start_native(&mut s, &mut peer).await;
    let (call, run) = broker(&s, operation).await;
    let cancel = CancellationToken::new();
    let _cancel_guard = cancel.clone().drop_guard();
    let audit = peer.audit(&s, run, cancel.clone());
    let thread: String = sqlx::query_scalar(
        "SELECT native_thread_id FROM native_mcp_calls WHERE company_id=$1 AND id=$2",
    )
    .bind(s.f.company)
    .bind(call)
    .fetch_one(&s.f.database.pool)
    .await
    .unwrap();
    let followup = queue_status(&s, &mut peer, operation, call).await;
    peer.write(
        "native.json",
        json!({"threadId":thread,"followupOperation":followup}),
    );
    let job = tokio::spawn(worker(
        s.f.persistence.clone(),
        remote.url.clone(),
        run,
        cancel.clone(),
    ));
    let approval_hold = std::env::var("OPENBOT_NATIVE_APPROVAL_HOLD_MS")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("native fixture approval hold milliseconds")
        })
        .unwrap_or(15_100);
    assert!((15_100..=65_100).contains(&approval_hold));
    approve(&s, run, &remote, Duration::from_millis(approval_hold)).await;
    eventually(async || {
        if remote.calls() == 1 {
            return Some(());
        }
        let state =
            crate::adapters::persistence::local_agents::test_support::mcp_workflow::native_state(
                s.f.persistence.as_ref(),
                CompanyId::new(s.f.company),
                run,
            )
            .await;
        assert!(
            !matches!(state["state"].as_str(), Some("cancelled" | "failed")),
            "native run ended before gateway: {state}"
        );
        None
    })
    .await;
    assert!(
        matches!(
            status(&s, operation, call).await.state,
            NativeMcpState::Pending
        ),
        "healthy in-flight dispatch keeps native tool pending"
    );
    let native_open: bool = sqlx::query_scalar(
        "SELECT outcome IS NULL FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(s.f.company)
    .bind(operation.as_uuid())
    .fetch_one(&s.f.database.pool)
    .await
    .unwrap();
    assert!(native_open);
    remote.release_call();
    let proof = peer.read("proof.json").await;
    assert_eq!(proof["intent"]["operation"], json!(operation));
    assert_eq!(proof["intent"]["modelCall"], "toolu_company_call");
    assert_eq!(proof["intent"]["request"], proof["clientRequestId"]);
    assert_ne!(proof["clientRequestId"], proof["clientRequestB"]);
    let response = status(&s, operation, call).await;
    assert!(matches!(response.state, NativeMcpState::Completed { .. }));
    assert_eq!(
        response.origin.invocation.client_request_id.as_str(),
        proof["clientRequestId"].as_str().unwrap()
    );
    assert_eq!(
        remote.arguments(),
        vec![json!({"query":"actual native company MCP"})]
    );
    assert_eq!(remote.calls(), 1);
    assert_eq!(remote.count("POST tools/call"), 1);
    cancel.cancel();
    job.await.unwrap();
    audit.await.unwrap();
    peer.close().await;
    server.close().await;
}
