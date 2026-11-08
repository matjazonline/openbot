use super::*;
use crate::adapters::persistence::workflow::tests::action_outcome_support::CompletedOutcome;
use crate::application::workflow::{
    checkpoints::WorkflowCheckpointStore,
    lease::LeaseReleaseCause,
    polling::{WorkflowPolling, WorkflowStepKind},
    worker::WorkflowHandler,
};

fn native_guard(error: sqlx::Error, message: &str) {
    let error = error.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("23514"));
    assert!(
        error.message().contains(message),
        "unexpected guard: {}",
        error.message()
    );
}

#[tokio::test]
async fn agent_history_ordering_native_guards_refuse_pointer_theft_nonowner_activation_and_wait_retag()
 {
    let f = Box::pin(HistoryThread::new()).await;
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let queued = f.next().await;
    let wait = parked(&f, &queued).await;
    for target in [None, Some(queued.fence.scope.execution.as_uuid())] {
        let error = sqlx::query(
            "UPDATE agent_history_conversations SET active_exchange_id=$2 WHERE company_id=$1",
        )
        .bind(owner.fence.scope.company.as_uuid())
        .bind(target)
        .execute(f.s.pool())
        .await
        .unwrap_err();
        native_guard(error, "unfinished agent turn");
    }
    let error=sqlx::query("UPDATE agent_history_exchanges SET state='active',checkpoint_id=$3,activated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(queued.fence.scope.company.as_uuid()).bind(queued.fence.scope.execution.as_uuid()).bind(Uuid::new_v4())
        .execute(f.s.pool()).await.unwrap_err();
    native_guard(error, "does not own its turn");
    let error=sqlx::query("UPDATE workflow_waits SET reason='timer',agent_exchange_id=NULL,parked_attempt_number=NULL WHERE company_id=$1 AND id=$2")
        .bind(queued.fence.scope.company.as_uuid()).bind(wait.as_uuid()).execute(f.s.pool()).await.unwrap_err();
    native_guard(error, "wait identity is immutable");
    assert_eq!(
        pointer(&f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
    assert!(
        f.s.p()
            .confirm_suspension(queued.fence, wait)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn agent_history_ordering_native_pointer_cannot_reference_another_tenants_exchange() {
    let f = Box::pin(HistoryThread::new()).await;
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let foreign_fixture = Fixture::in_database(f.s.fixture._db.clone(), "-foreign-pointer").await;
    let foreign = Box::pin(HistoryThread::in_fixture(foreign_fixture)).await;
    let claim = foreign.next().await;
    let identity = foreign.identity(&claim).await;
    let error=sqlx::query("UPDATE agent_history_conversations SET active_exchange_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(identity.company.as_uuid()).bind(identity.conversation.as_uuid()).bind(owner.fence.scope.execution.as_uuid())
        .execute(foreign.s.pool()).await.unwrap_err();
    native_guard(error, "requires the eligible live source");
    assert_eq!(pointer(&foreign).await, None);
    assert_eq!(
        pointer(&f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
}

#[tokio::test]
async fn agent_history_recovery_replacement_before_checkpoint_keeps_turn_and_fences_old_preparation()
 {
    let f = Box::pin(HistoryThread::new()).await;
    let old = f.next().await;
    let launch =
        f.s.p()
            .prepare_agent(&old, policy())
            .await
            .unwrap()
            .expect_ready();
    let spec = launch.prepared.spec(old.fence).unwrap();
    assert!(
        f.s.p()
            .release_io(old.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    // Advance only the fixture's retry scheduling time; every ownership guard stays enabled.
    sqlx::query("UPDATE background_tasks SET run_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND status='pending'")
        .bind(old.fence.scope.company.as_uuid()).bind(old.fence.scope.job.0).execute(f.s.pool()).await.unwrap();
    let new =
        f.s.p()
            .claim_io(old.fence.scope, WorkflowWorkerId(Uuid::new_v4()), policy())
            .await
            .unwrap()
            .unwrap();
    assert_ne!(old.fence, new.fence);
    assert_eq!(pointer(&f).await, Some(old.fence.scope.execution.as_uuid()));
    assert!(f.s.p().prepare_agent(&old, policy()).await.is_err());
    assert!(matches!(
        f.s.p()
            .open_workflow_checkpoint(old.fence, &spec, policy())
            .await
            .unwrap(),
        crate::services::harness::runs::WriteOutcome::OwnershipLost
    ));
    assert!(
        !f.s.p()
            .release_io(old.fence, policy(), LeaseReleaseCause::HandlerFailure)
            .await
            .unwrap()
    );
    let new_launch =
        f.s.p()
            .prepare_agent(&new, policy())
            .await
            .unwrap()
            .expect_ready();
    assert_eq!(
        new_launch.prepared.history_exchange(),
        launch.prepared.history_exchange()
    );
    f.s.p()
        .open_workflow_checkpoint(
            new.fence,
            &new_launch.prepared.spec(new.fence).unwrap(),
            policy(),
        )
        .await
        .unwrap();
    assert_eq!(pointer(&f).await, Some(old.fence.scope.execution.as_uuid()));
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1")
            .bind(old.fence.scope.company.as_uuid())
            .fetch_one(f.s.pool())
            .await
            .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn agent_history_recovery_revoked_wait_head_reaches_normal_terminal_owner_and_frees_successor()
 {
    let f = Box::pin(HistoryThread::new()).await;
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let queued = f.next().await;
    parked(&f, &queued).await;
    let next = f.next().await;
    parked(&f, &next).await;
    cancel(&f, &owner, "revoke-owner").await;
    let revoked=sqlx::query("UPDATE company_model_connections SET models=ARRAY['other-model'] WHERE company_id=$1 AND provider='openai'")
        .bind(queued.fence.scope.company.as_uuid()).execute(f.s.pool()).await.unwrap();
    assert_eq!(
        revoked.rows_affected(),
        1,
        "fixture must revoke actual selected model access"
    );
    assert!(matches!(
        f.s.p().resume_wait(queued.fence.scope).await.unwrap(),
        WaitProgress::Continued
    ));
    let claim =
        f.s.p()
            .claim_io(
                queued.fence.scope,
                WorkflowWorkerId(Uuid::new_v4()),
                policy(),
            )
            .await
            .unwrap()
            .unwrap();
    let failure = Box::pin(runtime(&f).execute(&WorkflowStepKind("agent.run".into()), &claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(
        failure.failure.class(),
        crate::domain::workflow::FailureClass::Terminal
    );
    assert!(
        f.s.p()
            .release_io(
                claim.fence,
                policy(),
                LeaseReleaseCause::Classified(failure)
            )
            .await
            .unwrap()
    );
    assert_eq!(pointer(&f).await, None);
    assert!(matches!(
        f.s.p().resume_wait(next.fence.scope).await.unwrap(),
        WaitProgress::Continued
    ));
    let calls: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_harness_runs WHERE company_id=$1")
            .bind(claim.fence.scope.company.as_uuid())
            .fetch_one(f.s.pool())
            .await
            .unwrap();
    assert_eq!(calls, 0);
}

#[tokio::test]
async fn agent_history_recovery_invalid_model_settlement_releases_without_accepting_an_answer() {
    use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
    let f = Box::pin(HistoryThread::new()).await;
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let queued = f.next().await;
    parked(&f, &queued).await;
    let llm = scripted_llm(vec![
        LlmTurn::text("invalid"),
        LlmTurn::text("still invalid"),
        LlmTurn::text("last invalid"),
    ])
    .await;
    register_scripted_agent_base_url(f.agent, &llm.base_url);
    assert!(
        Box::pin(runtime(&f).execute(&WorkflowStepKind("agent.run".into()), &owner))
            .await
            .completed_result()
            .is_err()
    );
    let row:Value=sqlx::query_scalar("SELECT jsonb_build_object('state',state,'output',output) FROM agent_history_exchanges WHERE company_id=$1 AND id=$2")
        .bind(owner.fence.scope.company.as_uuid()).bind(owner.fence.scope.execution.as_uuid()).fetch_one(f.s.pool()).await.unwrap();
    assert_eq!(row, json!({"state":"failed","output":null}));
    assert_eq!(pointer(&f).await, None);
    assert!(matches!(
        f.s.p().resume_wait(queued.fence.scope).await.unwrap(),
        WaitProgress::Continued
    ));
    assert_eq!(llm.finish().await.unwrap(), 3);
}

#[tokio::test]
async fn agent_history_recovery_queued_deadline_vs_resume_settles_once_without_releasing_another_owner()
 {
    let f = Box::pin(HistoryThread::new()).await;
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let queued = f.next().await;
    parked(&f, &queued).await;
    sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE company_id=$1 AND id=$2")
        .bind(queued.fence.scope.company.as_uuid()).bind(queued.fence.scope.run.as_uuid()).execute(f.s.pool()).await.unwrap();
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(f.s.pool())
        .await
        .unwrap();
    let (expired, resumed) = tokio::join!(
        f.s.p().expire_run(queued.fence.scope),
        f.s.p().resume_wait(queued.fence.scope)
    );
    expired.unwrap();
    assert!(!matches!(resumed.unwrap(), WaitProgress::Continued));
    assert_eq!(
        pointer(&f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
    let state: String = sqlx::query_scalar(
        "SELECT state FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(queued.fence.scope.company.as_uuid())
    .bind(queued.fence.scope.execution.as_uuid())
    .fetch_one(f.s.pool())
    .await
    .unwrap();
    assert_eq!(state, "failed");
}

#[tokio::test]
async fn agent_history_recovery_company_teardown_removes_turn_and_parked_companion_proofs() {
    let f = Box::pin(HistoryThread::new()).await;
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let queued = f.next().await;
    parked(&f, &queued).await;
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(owner.fence.scope.company.as_uuid())
        .execute(f.s.pool())
        .await
        .unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_history_exchanges")
        .fetch_one(f.s.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn agent_history_recovery_deferred_companion_pin_blocks_exchange_only_delete_at_commit() {
    let f = Box::pin(HistoryThread::new()).await;
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let queued = f.next().await;
    let wait = parked(&f, &queued).await;
    cancel(&f, &queued, "cancel-pinned-companion").await;
    let error = sqlx::query("DELETE FROM agent_history_exchanges WHERE company_id=$1 AND id=$2")
        .bind(queued.fence.scope.company.as_uuid())
        .bind(queued.fence.scope.execution.as_uuid())
        .execute(f.s.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert_eq!(
        pointer(&f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
    assert!(
        f.s.p()
            .confirm_suspension(queued.fence, wait)
            .await
            .unwrap()
    );
}
