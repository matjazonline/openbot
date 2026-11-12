use super::*;
use crate::application::local_agents::lifecycle::LocalAgentMaintenance;
use crate::application::workflow::storage::{RetentionRoot, WorkflowStorageMaintenance};

async fn age_run(f: &McpSetup, run: crate::domain::workflow::RunId) -> RetentionRoot {
    let mut tx = f.s.f.database.pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE workflow_runs DISABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    let finished_at = sqlx::query_scalar("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '100 days',finished_at=clock_timestamp()-interval '91 days' WHERE company_id=$1 AND id=$2 RETURNING finished_at")
        .bind(f.s.f.company).bind(run.as_uuid()).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    RetentionRoot {
        company: CompanyId::new(f.s.f.company),
        run,
        finished_at,
    }
}

async fn settle_a_start_b(f: &McpSetup) -> NativeMcpRequest {
    let p = &f.s.f.persistence;
    p.accept_observation(
        f.fence,
        &turn(
            4,
            f.operation,
            "turn_native_mcp",
            TurnState::Completed,
            Some("A finished"),
        ),
    )
    .await
    .unwrap();
    let session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(f.s.f.company)
    .bind(f.operation.as_uuid())
    .fetch_one(&f.s.f.database.pool)
    .await
    .unwrap();
    let control = f.s.control(
        SessionId::new(session).unwrap(),
        SessionCommand::Send {
            prompt: "Read A's retained status".to_owned().try_into().unwrap(),
        },
    );
    p.control_session(&control).await.unwrap();
    p.pending_commands(f.fence).await.unwrap();
    for observation in [
        agent_observation(5, control.request_id, LocalEvidence::Receipt {}),
        agent_observation(6, control.request_id, LocalEvidence::SubmissionIntent {}),
        turn(
            7,
            control.request_id,
            "turn_native_B",
            TurnState::Running,
            None,
        ),
    ] {
        p.accept_observation(f.fence, &observation).await.unwrap();
    }
    let mut request = f.request(json!({"query":"B"}));
    request.operation = control.request_id;
    request.body.invocation.turn_id = "turn_native_B".to_owned().try_into().unwrap();
    request.body.invocation.client_request_id = "creq_B".to_owned().try_into().unwrap();
    request.body.invocation.model_call_id = "toolu_B".to_owned().try_into().unwrap();
    request
}

async fn age_operation(f: &McpSetup) {
    let mut tx = f.s.f.database.pool.begin().await.unwrap();
    for name in [
        "local_operation_inputs_immutable",
        "local_operation_settlement",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE local_agent_operations DISABLE TRIGGER {name}"
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    sqlx::query("UPDATE local_agent_operations SET created_at=clock_timestamp()-interval '100 days',settled_at=clock_timestamp()-interval '91 days' WHERE company_id=$1 AND id=$2 AND settled_at IS NOT NULL")
        .bind(f.s.f.company).bind(f.operation.as_uuid()).execute(&mut *tx).await.unwrap();
    for name in [
        "local_operation_inputs_immutable",
        "local_operation_settlement",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE local_agent_operations ENABLE TRIGGER {name}"
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn native_mcp_retention_competitors_keep_identity_and_later_status_after_collecting_original_payload()
 {
    let f = McpSetup::new().await;
    let p = &f.s.f.persistence;
    let a = p
        .native_mcp(&f.request(json!({"query":"A"})))
        .await
        .unwrap();
    let mut cancel = f.request(json!({}));
    cancel.body.action = NativeMcpAction::CancelInvocation;
    p.native_mcp(&cancel).await.unwrap();
    assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 1);
    let root = age_run(&f, a.run_id).await;
    assert!(
        p.retire_tree(root).await.unwrap().is_none(),
        "unsettled native tool pins its real run"
    );
    let mut b = settle_a_start_b(&f).await;
    age_operation(&f).await;
    p.retain_local_agents(None).await.unwrap();
    let pinned: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(f.s.f.company)
    .bind(f.operation.as_uuid())
    .fetch_one(&f.s.f.database.pool)
    .await
    .unwrap();
    assert_eq!(
        pinned, 1,
        "live broker reference pins original native operation"
    );
    let level: i16 = sqlx::query_scalar(
        "SELECT content_level::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(f.s.f.company)
    .bind(a.run_id.as_uuid())
    .fetch_one(&f.s.f.database.pool)
    .await
    .unwrap();
    // The immediate retirement guard permits this resolved old run, but the deferred
    // witness must reject detaching it while its actual run still exists.
    let mut tx = f.s.f.database.pool.begin().await.unwrap();
    sqlx::query("UPDATE native_mcp_calls SET run_id=NULL,input=NULL,source_operation_id=NULL,source_session_id=NULL,retained_content_level=$3::smallint::access_level,terminal_outcome='cancelled',retired_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(f.s.f.company).bind(a.call_id.as_uuid()).bind(level).execute(&mut *tx).await.unwrap();
    let error = sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
    let (left, right) = tokio::join!(p.retire_tree(root), p.retire_tree(root));
    assert_eq!(
        usize::from(left.unwrap().is_some()) + usize::from(right.unwrap().is_some()),
        1
    );
    let tombstone:(bool,bool,i16,Uuid)=sqlx::query_as("SELECT input IS NULL,source_operation_id IS NULL,retained_content_level::smallint,original_run_id FROM native_mcp_calls WHERE company_id=$1 AND id=$2")
        .bind(f.s.f.company).bind(a.call_id.as_uuid()).fetch_one(&f.s.f.database.pool).await.unwrap();
    assert_eq!(tombstone, (true, true, level, a.run_id.as_uuid()));
    p.retain_local_agents(None).await.unwrap();
    let removed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(f.s.f.company)
    .bind(f.operation.as_uuid())
    .fetch_one(&f.s.f.database.pool)
    .await
    .unwrap();
    assert_eq!(removed, 0);
    b.body.action = NativeMcpAction::Status { call_id: a.call_id };
    let status = p.native_mcp(&b).await.unwrap();
    assert!(matches!(status.state, NativeMcpState::Collected));
    assert_eq!(status.origin.operation_id, f.operation);
    assert_eq!(
        status.origin.invocation.client_request_id.as_str(),
        "creq_native"
    );
    assert_eq!((status.call_id, status.run_id), (a.call_id, a.run_id));
    assert!(
        p.native_mcp(&f.request(json!({"query":"A"})))
            .await
            .is_err(),
        "collected A cannot admit again"
    );
    b.body.action = NativeMcpAction::Call {
        alias: AgentToolAlias::parse("crm.lookup").unwrap(),
        arguments: json!({"query":"A"}),
    };
    b.body.invocation.client_request_id = "creq_native".to_owned().try_into().unwrap();
    b.body.invocation.model_call_id = "toolu_native".to_owned().try_into().unwrap();
    assert!(
        p.native_mcp(&b).await.is_err(),
        "B cannot borrow A's immutable invocation identity"
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM native_mcp_calls WHERE company_id=$1")
            .bind(f.s.f.company)
            .fetch_one(&f.s.f.database.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}
