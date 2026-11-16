use super::*;
use std::time::Duration;

#[tokio::test]
async fn cloudroom_workflow_approval_parks_before_native_admission_and_grants_one_effect() {
    let s = Setup::with_inputs(|inputs, owner| {
        inputs["approval"] = json!({"literal":{"reviewers":[{"kind":"principal","id":owner}],
            "deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()}});
    })
    .await;
    let first = s.claim().await;
    let approval = s.park(&first).await;
    assert_eq!(
        s.rows().await,
        json!({"calls":1,"operations":0,"intents":1,"markers":0,"receipts":0,"waits":1})
    );
    let command = crate::adapters::persistence::workflow::tests::approval_test_support::command(
        &s.fixture.persistence,
        s.scope,
        s.fixture.target.actor,
        "approve",
    )
    .await;
    s.fixture
        .persistence
        .submit_decision(command.clone())
        .await
        .unwrap();
    let next = s.claim().await;
    let effect = s.park(&next).await;
    assert_ne!(effect, approval);
    assert_eq!(
        s.rows().await,
        json!({"calls":1,"operations":1,"intents":1,"markers":1,"receipts":0,"waits":2})
    );
    s.fixture
        .persistence
        .submit_decision(command)
        .await
        .unwrap();
    assert_eq!(s.park(&next).await, effect);
    assert_eq!(s.rows().await["operations"], 1);
}

#[tokio::test]
async fn cloudroom_workflow_completion_racing_cancellation_retains_evidence_without_revival() {
    use crate::application::workflow::{CancelCommand, CancelResult, IdempotencyKey};
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let first = s.claim().await;
    s.park(&first).await;
    let native = s.native_started().await;
    let cancel = CancelCommand {
        company_id: s.scope.company,
        run_id: s.scope.run,
        actor: s.fixture.target.actor,
        command_key: IdempotencyKey::parse("local-cancel").unwrap(),
        expected_revision: p
            .head(s.scope.company, s.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    let (cancelled, ()) = tokio::join!(p.cancel(cancel), s.native_complete(&native));
    assert!(matches!(cancelled.unwrap(), CancelResult::Applied { .. }));
    p.sweep_waits(8).await.unwrap();
    assert!(
        p.claim_io(s.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none()
    );
    let (state,outcome,output):(String,String,String)=sqlx::query_as("SELECT run.state,operation.outcome,operation.output FROM workflow_runs AS run JOIN local_agent_operations AS operation ON operation.company_id=run.company_id WHERE run.id=$1 AND operation.id=$2")
        .bind(s.scope.run.as_uuid()).bind(native.operation.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(state, "cancelled");
    assert_eq!(outcome, "succeeded");
    assert_eq!(output, "native output");
    assert_eq!(s.rows().await["receipts"], 0);
}

#[tokio::test]
async fn cloudroom_workflow_machine_withdrawal_refuses_parked_output_and_live_delivery() {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let claim = s.claim().await;
    s.park(&claim).await;
    s.native_success().await;
    sqlx::query("UPDATE local_machines SET state='disabled' WHERE company_id=$1 AND id=$2")
        .bind(s.scope.company.as_uuid())
        .bind(s.profile.selection.machine_id.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    p.resume_wait(s.scope).await.unwrap();
    assert!(
        p.claim_io(s.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none()
    );
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(s.scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(state, "failed");
    assert_eq!(s.rows().await["receipts"], 0);
}

#[tokio::test]
async fn cloudroom_publication_native_output_is_data_and_direct_followup_cannot_reopen_workflow() {
    use crate::application::local_agents::management::{
        ControlSession, LocalAgentManagementStore, SessionCommand,
    };
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let claim = s.claim().await;
    s.park(&claim).await;
    let fence = s.native_success().await;
    p.resume_wait(s.scope).await.unwrap();
    let next = s.claim().await;
    let result = s
        .handler()
        .execute(&WorkflowStepKind(KIND.into()), &next)
        .await
        .completed_result()
        .unwrap();
    let session =
        SessionId::new(Uuid::parse_str(result["session_id"].as_str().unwrap()).unwrap()).unwrap();
    assert!(
        p.complete_io(FencedWorkflowResult {
            fence: next.fence,
            output: result,
            policy: lease()
        })
        .await
        .unwrap()
        .is_some()
    );
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(s.scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(state, "succeeded");
    let before = s.rows().await;
    let request = ControlSession {
        actor: s.fixture.target.actor.user_id(),
        company: s.scope.company.as_uuid(),
        session_id: session,
        request_id: OperationId::new(Uuid::now_v7()).unwrap(),
        command: SessionCommand::Send {
            prompt: LocalPrompt::try_from("Explicit human follow-up".to_owned()).unwrap(),
        },
    };
    let direct = p.control_session(&request).await.unwrap();
    assert_eq!(direct.origin, OperationOrigin::Direct);
    let deliveries = p.pending_commands(fence).await.unwrap();
    assert!(
        deliveries
            .iter()
            .any(|item| item.operation.id() == request.request_id),
        "terminal original workflow does not prevent an explicit authorized human follow-up"
    );
    let after = s.rows().await;
    assert_eq!(after["calls"], before["calls"]);
    assert_eq!(after["waits"], before["waits"]);
    assert_eq!(after["receipts"], before["receipts"]);
    let published:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM message_deliveries)+(SELECT count(*) FROM workflow_message_actions)").fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        published, 0,
        "native text never publishes a message by itself"
    );
    assert!(
        p.claim_io(s.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none()
    );
    let error = sqlx::query("UPDATE local_agent_operations SET origin='workflow' WHERE id=$1")
        .bind(request.request_id.as_uuid())
        .execute(p.pool())
        .await
        .unwrap_err();
    assert!(
        error
            .as_database_error()
            .is_some_and(|error| error.code().as_deref() == Some("23514")),
        "operation origin cannot be rewritten"
    );
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO local_agent_operations(company_id,machine_id,installation_id,id,session_id,logical_key,origin,actor_id,command,payload_digest,deadline,dispatch) SELECT company_id,machine_id,installation_id,gen_random_uuid(),session_id,'unowned:'||gen_random_uuid()::text,'workflow',actor_id,command,payload_digest,deadline,dispatch FROM local_agent_operations WHERE id=$1")
        .bind(request.request_id.as_uuid()).execute(&mut *tx).await.unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:local_workflow_operation_owner")
    );
}

#[tokio::test]
async fn cloudroom_workflow_private_run_output_and_every_direct_control_require_original_association()
 {
    use crate::application::local_agents::management::{
        ControlSession, LocalAgentManagementStore, SessionCommand,
    };
    let s = Setup::configured(|_, _| {}, TestAssociation::PrivateConversation).await;
    let p = &s.fixture.persistence;
    let claim = s.claim().await;
    s.park(&claim).await;
    s.native_success().await;
    let session: Uuid = sqlx::query_scalar("SELECT session_id FROM workflow_local_agent_calls")
        .fetch_one(p.pool())
        .await
        .unwrap();
    let session = SessionId::new(session).unwrap();
    let admin: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM company_members WHERE company_id=$1 AND role='admin'",
    )
    .bind(s.scope.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(matches!(
        p.read_session(admin, s.scope.company.as_uuid(), session)
            .await,
        Err(AppError::NotFound(_))
    ));
    for command in [
        SessionCommand::Inspect,
        SessionCommand::Stop,
        SessionCommand::Archive,
        SessionCommand::Send {
            prompt: LocalPrompt::try_from("forged".to_owned()).unwrap(),
        },
    ] {
        let request = ControlSession {
            actor: admin,
            company: s.scope.company.as_uuid(),
            session_id: session,
            request_id: OperationId::new(Uuid::now_v7()).unwrap(),
            command,
        };
        assert!(matches!(
            p.control_session(&request).await,
            Err(AppError::NotFound(_))
        ));
    }
    assert_eq!(s.rows().await["operations"], 1);
    assert!(
        p.read_session(
            s.fixture.target.actor.user_id(),
            s.scope.company.as_uuid(),
            session
        )
        .await
        .is_ok()
    );
}

#[tokio::test]
async fn cloudroom_workflow_offline_operation_expires_at_frozen_deadline_without_native_work() {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    // Compress the fixture clock horizon. Restore its immutable-snapshot guard before
    // production code freezes the operation, then use real database time for expiry.
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("ALTER TABLE workflow_runs DISABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE workflow_runs SET deadline=clock_timestamp()+interval '2 seconds' WHERE id=$1",
    )
    .bind(s.scope.run.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let first = s.claim().await;
    let wait = s.park(&first).await;
    assert!(p.confirm_suspension(first.fence, wait).await.unwrap());
    let before:Value=sqlx::query_scalar("SELECT jsonb_build_object('deadline',deadline,'digest',payload_digest,'attempted',delivery_attempted_at) FROM local_agent_operations")
        .fetch_one(p.pool()).await.unwrap();
    assert!(before["attempted"].is_null());
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert_eq!(p.expire_operations(8).await.unwrap(), 1);
    p.sweep_waits(8).await.unwrap();
    assert!(
        p.claim_io(s.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none()
    );
    let after:Value=sqlx::query_scalar("SELECT jsonb_build_object('deadline',deadline,'digest',payload_digest,'outcome',outcome,'output',output) FROM local_agent_operations")
        .fetch_one(p.pool()).await.unwrap();
    assert_eq!(before["deadline"], after["deadline"]);
    assert_eq!(before["digest"], after["digest"]);
    assert_eq!(after["outcome"], "expired");
    assert!(after["output"].is_null());
    assert_eq!(s.rows().await["receipts"], 0);
}
