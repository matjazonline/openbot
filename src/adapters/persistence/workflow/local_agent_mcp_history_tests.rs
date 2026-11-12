//! The native ACK is the ordinary trusted protocol fixture; all workflow/effect/
//! reconciliation owners are real. This is not a native CLI or provider-ledger proof.
use super::*;
use crate::adapters::persistence::local_agents::test_support::mcp_workflow::{
    approve, eventually, worker,
};
use crate::adapters::persistence::workflow::native_mcp_remote_fixture as remote;
use crate::application::local_agents::{LocalMachineUseCases, MachineChange};
use crate::application::use_cases::mcp::McpPersistence;
use crate::application::workflow::{actions::*, connection_level::ConnectionLevelCommand};
use crate::domain::workflow::{ActionInvocationId, ActionRemoteMarkerId};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

async fn unknown() -> (Setup, NativeTurn, NativeMcpResponse, remote::McpServer) {
    let remote = remote::mcp_server(remote::Call::Fail).await;
    remote.set_tools(json!([{"name":"lookup","description":"Lookup","inputSchema":{"type":"object","required":["query"],"additionalProperties":false,"properties":{"query":{"type":"string","maxLength":128}}},"outputSchema":{"type":"object"}}]));
    let (s, native) = Box::pin(originating_at(&remote.url)).await;
    let p = &s.fixture.persistence;
    let (connection, revision): (Uuid, i64) = sqlx::query_as(
        "SELECT id,revision FROM company_mcp_connections WHERE company_id=$1 AND slug='native-crm'",
    )
    .bind(s.scope.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    p.set_mcp_level(ConnectionLevelCommand {
        company: s.scope.company,
        actor: s.fixture.target.actor,
        connection,
        expected_revision: revision,
        access_level: crate::entities::access_level::AccessLevel::L5,
    })
    .await
    .unwrap();
    let response = p.native_mcp(&request(&s, &native)).await.unwrap();
    let actor: Uuid =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(s.scope.company.as_uuid())
            .bind(response.run_id.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let cancel = CancellationToken::new();
    let guard = cancel.clone().drop_guard();
    let task = tokio::spawn(worker(
        Arc::new(p.clone()),
        remote.url.clone(),
        response.run_id,
        cancel.clone(),
    ));
    approve(
        p,
        s.scope.company,
        WorkflowActor::authenticated(actor).unwrap(),
        response.run_id,
        &remote,
        Duration::ZERO,
    )
    .await;
    eventually(async || {
        let waiting:bool=sqlx::query_scalar("SELECT state='waiting' AND waiting_reason='reconciliation' FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(s.scope.company.as_uuid()).bind(response.run_id.as_uuid()).fetch_one(p.pool()).await.unwrap();
        waiting.then_some(())
    }).await;
    cancel.cancel();
    task.await.unwrap();
    drop(guard);
    assert_eq!(remote.calls(), 1);
    (s, native, response, remote)
}

async fn retire_original(s: &Setup, response: &NativeMcpResponse) {
    let p = &s.fixture.persistence;
    p.cancel(CancelCommand {
        company_id: s.scope.company,
        run_id: s.scope.run,
        actor: s.fixture.target.actor,
        command_key: IdempotencyKey::parse("retire-native-history-parent").unwrap(),
        expected_revision: p
            .head(s.scope.company, s.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    })
    .await
    .unwrap();
    let cases = LocalMachineUseCases::new(
        Arc::new(p.clone()),
        Arc::new(p.clone()),
        "native-history-test-key-distinct-32-bytes".into(),
    );
    cases
        .change(
            s.fixture.target.actor.user_id(),
            s.scope.company.as_uuid(),
            s.profile.selection.machine_id,
            MachineChange::Revoke,
        )
        .await
        .unwrap();
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND user_id=(SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2)")
        .bind(s.scope.company.as_uuid()).bind(response.run_id.as_uuid()).execute(p.pool()).await.unwrap();
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=(SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2)")
        .bind(s.scope.company.as_uuid()).bind(response.run_id.as_uuid()).execute(p.pool()).await.unwrap();
    sqlx::query("UPDATE agent_binding_grants SET revoked_at=clock_timestamp() WHERE company_id=$1")
        .bind(s.scope.company.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 1);
}

#[tokio::test]
async fn native_mcp_workflow_unknown_history_survives_real_parent_cancel_machine_revoke_and_original_actor_removal()
 {
    let (s, native, response, remote) = Box::pin(unknown()).await;
    Box::pin(retire_original(&s, &response)).await;
    let p = &s.fixture.persistence;
    let (operation,marker,invocation):(Value,Uuid,Uuid)=sqlx::query_as("SELECT intent.operation,marker.id,intent.id FROM workflow_action_intents AS intent JOIN workflow_action_dispatches AS marker ON marker.company_id=intent.company_id AND marker.invocation_id=intent.id WHERE intent.company_id=$1 AND intent.run_id=$2")
        .bind(s.scope.company.as_uuid()).bind(response.run_id.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let frozen = FrozenAction::restore(operation).unwrap();
    let command = ReconcileActionCommand {
        actor: s.fixture.target.actor,
        scope: frozen.scope(),
        subject: ApprovalSubject {
            invocation: ActionInvocationId::new(invocation),
            argument_digest: frozen.argument_digest().clone(),
        },
        marker: ActionRemoteMarkerId::new(marker),
        target: ReconciliationTarget::Remote,
        expected_revision: p
            .head(s.scope.company, response.run_id)
            .await
            .unwrap()
            .unwrap()
            .revision,
        command_key: IdempotencyKey::parse("native-parent-historical-note").unwrap(),
        input: EvidenceInput::UnknownNote {
            note: "Fixture remote effect remains unknown after source retirement".into(),
            claimed: ClaimedDisposition::Unknown,
        },
    };
    let engine = crate::infra::workflow::WorkflowEngine::for_tests(p.clone());
    let service = engine.services().reconciliation();
    let result =
        Box::pin(service.reconcile(&command, &CancellationToken::new(), Duration::from_secs(5)))
            .await
            .unwrap();
    assert!(matches!(
        result.outcome,
        ReconciliationOutcome::UnknownRecorded
    ));
    assert!(
        Box::pin(service.reconcile(&command, &CancellationToken::new(), Duration::from_secs(5)))
            .await
            .unwrap()
            .replayed
    );
    assert_eq!(remote.calls(), 1);
    assert!(p.native_mcp(&request(&s, &native)).await.is_err());
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(s.scope.company.as_uuid())
            .bind(response.run_id.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(state, "cancelled");
}
