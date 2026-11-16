//! Native observations are trusted protocol fixtures; source, broker, leases and
//! competing lifecycle transactions use their production PostgreSQL owners.
use super::live_tests::{claim, lease};
use super::*;
use crate::application::local_agents::protocol::LocalProtocolStore;
use crate::application::workflow::{
    lease::WorkflowLeases, mcp::McpActionPreparation, polling::WorkflowStepKind,
};
use std::time::Duration;

async fn session(f: &McpSetup) -> SessionId {
    let id: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(f.s.f.company)
    .bind(f.operation.as_uuid())
    .fetch_one(&f.s.f.database.pool)
    .await
    .unwrap();
    SessionId::new(id).unwrap()
}

async fn send(f: &McpSetup, lifetime: chrono::Duration) -> NativeMcpRequest {
    let id = OperationId::new(Uuid::now_v7()).unwrap();
    let operation = LocalAgentOperation::queued(
        id,
        LocalCommand::Send {
            session_id: session(f).await,
            thread_id: "thr_frozen".to_owned().try_into().unwrap(),
            prompt: "an explicit later native input"
                .to_owned()
                .try_into()
                .unwrap(),
        },
        chrono::Utc::now() + lifetime,
    )
    .unwrap();
    let p = f.s.f.persistence.as_ref();
    p.enqueue_operation(&OperationAdmission {
        company_id: f.s.f.company,
        actor_id: f.s.f.owner,
        machine_id: f.s.profile.selection.machine_id,
        logical_key: LogicalInvocationKey::parse(format!("native-lifecycle:{id}")).unwrap(),
        operation,
    })
    .await
    .unwrap();
    p.pending_commands(f.fence).await.unwrap();
    for observation in [
        agent_observation(4, id, LocalEvidence::Receipt {}),
        agent_observation(5, id, LocalEvidence::SubmissionIntent {}),
        turn(6, id, "turn_after_stop", TurnState::Running, None),
    ] {
        p.accept_observation(f.fence, &observation).await.unwrap();
    }
    let mut request = f.request(json!({"query":"explicit later input"}));
    request.operation = id;
    request.body.invocation.turn_id = "turn_after_stop".to_owned().try_into().unwrap();
    request.body.invocation.client_request_id = "creq_after_stop".to_owned().try_into().unwrap();
    request
}

async fn no_effect(f: &McpSetup) {
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1")
            .bind(f.s.f.company)
            .fetch_one(&f.s.f.database.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

async fn sweep_due(f: &McpSetup) {
    // A skipped run owner deliberately leaves the normal durable one-second backoff.
    let millis:i64=sqlx::query_scalar("SELECT (1000*EXTRACT(EPOCH FROM GREATEST(maintenance_after-clock_timestamp(),interval '0 seconds')))::bigint FROM native_mcp_calls WHERE company_id=$1 ORDER BY maintenance_after LIMIT 1")
        .bind(f.s.f.company).fetch_one(&f.s.f.database.pool).await.unwrap();
    assert!((0..=1000).contains(&millis));
    tokio::time::sleep(Duration::from_millis(millis as u64 + 30)).await;
}

#[tokio::test]
async fn native_mcp_stop_and_archive_commit_without_broker_owner_then_revoke_old_calls_but_allow_new_explicit_input()
 {
    for command in [SessionCommand::Stop, SessionCommand::Archive] {
        let f = Box::pin(McpSetup::new()).await;
        let p = f.s.f.persistence.as_ref();
        let response = p
            .native_mcp(&f.request(json!({"query":"old native input"})))
            .await
            .unwrap();
        let claim = Box::pin(claim(&f, response.run_id)).await;
        let mut competitor = f.s.f.database.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(f.s.f.company)
            .bind(response.run_id.as_uuid())
            .execute(&mut *competitor)
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            p.control_session(&f.s.control(session(&f).await, command)),
        )
        .await
        .expect("native control must never wait for a broker owner")
        .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), p.sweep_native_mcp(16))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        let active: bool = sqlx::query_scalar("SELECT public.native_mcp_run_live($1,$2)")
            .bind(f.s.f.company)
            .bind(response.run_id.as_uuid())
            .fetch_one(&f.s.f.database.pool)
            .await
            .unwrap();
        assert!(
            !active,
            "committed stop intent immediately revokes MCP, before native stop evidence"
        );
        let unresolved: bool = sqlx::query_scalar(
            "SELECT outcome IS NULL FROM local_agent_operations WHERE company_id=$1 AND id=$2",
        )
        .bind(f.s.f.company)
        .bind(f.operation.as_uuid())
        .fetch_one(&f.s.f.database.pool)
        .await
        .unwrap();
        assert!(
            unresolved,
            "stop intent is not proof that the native provider stopped"
        );
        competitor.rollback().await.unwrap();
        assert!(!p.validate_io(claim.fence, lease()).await.unwrap());
        assert!(
            Box::pin(p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into())))
                .await
                .is_err()
        );
        let mut late = f.request(json!({"query":"late new effect"}));
        late.body.invocation.model_call_id = "toolu_after_stop".to_owned().try_into().unwrap();
        assert!(p.native_mcp(&late).await.is_err());
        sweep_due(&f).await;
        assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 1);
        no_effect(&f).await;
        let later = send(&f, chrono::Duration::minutes(1)).await;
        let newer = p.native_mcp(&later).await.unwrap();
        assert_ne!(newer.run_id, response.run_id);
        let mut status = later;
        status.body.action = NativeMcpAction::Status {
            call_id: response.call_id,
        };
        let old = p.native_mcp(&status).await.unwrap();
        assert_eq!(old.origin.operation_id, f.operation);
        assert!(matches!(old.state, NativeMcpState::Cancelled));
        no_effect(&f).await;
    }
}

#[tokio::test]
async fn native_mcp_real_operation_expiration_never_waits_for_broker_and_competing_sweeps_finish_once()
 {
    let f = Box::pin(McpSetup::new()).await;
    let p = f.s.f.persistence.as_ref();
    let request = send(&f, chrono::Duration::seconds(3)).await;
    let response = p.native_mcp(&request).await.unwrap();
    let claim = Box::pin(claim(&f, response.run_id)).await;
    let mut competitor = f.s.f.database.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.s.f.company)
        .bind(response.run_id.as_uuid())
        .execute(&mut *competitor)
        .await
        .unwrap();
    let remaining = (response.deadline - chrono::Utc::now())
        .to_std()
        .unwrap_or_default();
    tokio::time::sleep(remaining + Duration::from_millis(30)).await;
    let (left, right) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(p.expire_operations(16), p.expire_operations(16))
    })
    .await
    .expect("operation-only expiration cannot wait for the locked broker");
    assert_eq!(left.unwrap() + right.unwrap(), 1);
    assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 0);
    competitor.rollback().await.unwrap();
    assert!(!p.validate_io(claim.fence, lease()).await.unwrap());
    assert!(
        Box::pin(p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into())))
            .await
            .is_err()
    );
    sweep_due(&f).await;
    let (left, right) = tokio::join!(p.sweep_native_mcp(16), p.sweep_native_mcp(16));
    assert_eq!(left.unwrap() + right.unwrap(), 1);
    no_effect(&f).await;
    match p.native_mcp(&request).await {
        Err(_) => {}
        Ok(replay) => {
            assert_eq!(
                (replay.call_id, replay.run_id),
                (response.call_id, response.run_id)
            );
            assert!(matches!(replay.state, NativeMcpState::Cancelled));
        }
    }
}
