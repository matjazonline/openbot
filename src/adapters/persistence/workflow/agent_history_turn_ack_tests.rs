use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{Abort,Gate,GatePool,Stall,connect,resolution_behind,waiters};

impl GatePool for HistoryThread {
    fn gate_pool(&self) -> &sqlx::PgPool {
        self.s.pool()
    }
}
struct ParkAck {
    f: Box<HistoryThread>,
    owner: ClaimedWorkflow,
    queued: ClaimedWorkflow,
    result: AppResult<AgentPreparationResult>,
}
async fn ambiguous_park(abort: Abort) -> ParkAck {
    let f = Box::new(Box::pin(HistoryThread::new()).await);
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let queued = f.next().await;
    let mut gate = Gate::arm(
        &*f,
        "workflow_waits",
        "INSERT",
        "NEW.reason='agent_turn'",
        Stall::Commit,
    )
    .await;
    let (mut observer, _) = connect(&*f).await;
    let policy = LeasePolicy::new(Duration::from_secs(6)).unwrap();
    let pending = {
        let p = f.s.p().clone();
        let claim = queued.clone();
        tokio::spawn(async move { p.prepare_agent(&claim, policy).await })
    };
    let original = gate
        .stalled(&mut observer, &[&|| !pending.is_finished()])
        .await;
    resolution_behind(
        &mut observer,
        original,
        policy,
        &[&|| !pending.is_finished()],
    )
    .await;
    gate.abort_on_release(abort).await;
    gate.drain().await;
    let result = pending.await.unwrap();
    gate.abort_on_release(Abort::Nothing).await;
    ParkAck {
        f,
        owner,
        queued,
        result,
    }
}

#[tokio::test]
async fn agent_history_recovery_park_commit_ack_loss_proves_retired_attempt_and_survives_fast_wake()
{
    let injected = Injected::mark();
    let ack = ambiguous_park(Abort::Nothing).await;
    let wait = match ack.result {
        Ok(AgentPreparationResult::Parked(wait)) => wait,
        Err(error) => panic!("park receipt was not proven: {error}"),
        _ => panic!("committed queued work must return only its park receipt"),
    };
    assert!(injected.outcomes().iter().any(|event|event.op=="agent_history.prepare"&&event.outcome=="resolved_applied"));
    assert_eq!(
        pointer(&ack.f).await,
        Some(ack.owner.fence.scope.execution.as_uuid())
    );
    assert!(
        ack.f
            .s
            .p()
            .confirm_suspension(ack.queued.fence, wait)
            .await
            .unwrap()
    );
    cancel(&ack.f, &ack.owner, "ack-cancel-owner").await;
    assert!(matches!(
        ack.f
            .s
            .p()
            .resume_wait(ack.queued.fence.scope)
            .await
            .unwrap(),
        WaitProgress::Continued
    ));
    assert!(
        ack.f
            .s
            .p()
            .confirm_suspension(ack.queued.fence, wait)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn agent_history_recovery_aborted_park_commit_has_no_receipt_and_retries_same_frozen_activation()
 {
    let injected = Injected::mark();
    let ack = ambiguous_park(Abort::Every).await;
    assert!(ack.result.is_err());
    assert!(injected.outcomes().iter().any(
        |event| event.op == "agent_history.prepare" && event.outcome == "resolved_not_applied"
    ));
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1")
            .bind(ack.queued.fence.scope.company.as_uuid())
            .fetch_one(ack.f.s.pool())
            .await
            .unwrap();
    assert_eq!(rows, 1);
    assert_eq!(
        pointer(&ack.f).await,
        Some(ack.owner.fence.scope.execution.as_uuid())
    );
    assert!(
        ack.f
            .s
            .p()
            .validate_io(ack.queued.fence, policy())
            .await
            .unwrap()
    );
    let wait = parked(&ack.f, &ack.queued).await;
    assert!(
        ack.f
            .s
            .p()
            .confirm_suspension(ack.queued.fence, wait)
            .await
            .unwrap()
    );
    let position: i64 = sqlx::query_scalar(
        "SELECT position FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(ack.queued.fence.scope.company.as_uuid())
    .bind(ack.queued.fence.scope.execution.as_uuid())
    .fetch_one(ack.f.s.pool())
    .await
    .unwrap();
    assert_eq!(position, 2);
}

#[tokio::test]
async fn agent_history_recovery_unknown_final_ack_keeps_turn_until_commit_and_completed_replay_cannot_release_successor()
 {
    use crate::adapters::persistence::workflow::tests::action_outcome_support::CompletedOutcome;
    use crate::application::workflow::{polling::WorkflowStepKind, worker::WorkflowHandler};
    use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
    let _injected = Injected::mark();
    let f = Box::new(Box::pin(HistoryThread::new()).await);
    let llm = scripted_llm(vec![LlmTurn::text("\"accepted\"")]).await;
    register_scripted_agent_base_url(f.agent, &llm.base_url);
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let queued = f.next().await;
    parked(&f, &queued).await;
    let mut gate = Gate::arm(
        &*f,
        "task_harness_runs",
        "UPDATE",
        "NEW.state='completed'",
        Stall::Commit,
    )
    .await;
    let (mut observer, _) = connect(&*f).await;
    let pending = {
        let runtime = runtime(&f);
        let claim = owner.clone();
        tokio::spawn(async move {
            Box::pin(runtime.execute(&WorkflowStepKind("agent.run".into()), &claim)).await
        })
    };
    let writer = gate
        .stalled(&mut observer, &[&|| !pending.is_finished()])
        .await;
    assert_eq!(
        pointer(&f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
    assert!(!queued_ready(&f, &queued).await);
    let first = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap();
    assert!(first.completed_result().is_err());
    assert_eq!(
        pointer(&f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
    competing_writers(&f, &owner, &queued, &mut observer, writer).await;
    gate.drain().await;
    assert_eq!(pointer(&f).await, None);
    let accepted:Value=sqlx::query_scalar("SELECT output FROM agent_history_exchanges WHERE company_id=$1 AND id=$2 AND state='completed'")
        .bind(owner.fence.scope.company.as_uuid()).bind(owner.fence.scope.execution.as_uuid()).fetch_one(f.s.pool()).await.unwrap();
    assert_eq!(accepted, json!("accepted"));
    assert!(matches!(
        f.s.p().resume_wait(queued.fence.scope).await.unwrap(),
        WaitProgress::Continued
    ));
    assert_eq!(
        pointer(&f).await,
        Some(queued.fence.scope.execution.as_uuid())
    );
    let replay = Box::pin(runtime(&f).execute(&WorkflowStepKind("agent.run".into()), &owner)).await;
    assert_eq!(replay.completed_result().unwrap(), json!("accepted"));
    assert_eq!(
        pointer(&f).await,
        Some(queued.fence.scope.execution.as_uuid())
    );
    cancel(&f, &owner, "cancel-after-final").await;
    assert_eq!(
        pointer(&f).await,
        Some(queued.fence.scope.execution.as_uuid())
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
}

async fn timeout_behind<T: Send + 'static>(
    observer: &mut sqlx::PgConnection,
    writer: i32,
    operation: impl std::future::Future<Output = AppResult<T>> + Send + 'static,
) {
    let pending = tokio::spawn(operation);
    waiters(
        observer,
        writer,
        1,
        "native writer behind final checkpoint",
        &[&|| !pending.is_finished()],
    )
    .await;
    assert!(matches!(
        pending.await.unwrap(),
        Err(AppError::DatabaseTimeout(_))
    ));
}

async fn competing_writers(
    f: &HistoryThread,
    owner: &ClaimedWorkflow,
    queued: &ClaimedWorkflow,
    observer: &mut sqlx::PgConnection,
    writer: i32,
) {
    let p = f.s.p().clone();
    let scope = queued.fence.scope;
    timeout_behind(observer, writer, async move { p.resume_wait(scope).await }).await;
    let command = CancelCommand {
        company_id: owner.fence.scope.company,
        run_id: owner.fence.scope.run,
        actor: f.s.owner(),
        command_key: IdempotencyKey::parse("cancel-behind-final").unwrap(),
        expected_revision: f
            .s
            .p()
            .head(owner.fence.scope.company, owner.fence.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    let p = f.s.p().clone();
    timeout_behind(observer, writer, async move { p.cancel(command).await }).await;
    assert_eq!(
        pointer(f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
}

async fn queued_ready(f: &HistoryThread, claim: &ClaimedWorkflow) -> bool {
    sqlx::query_scalar("SELECT agent_history_turn_ready($1,$2)")
        .bind(claim.fence.scope.company.as_uuid())
        .bind(claim.fence.scope.execution.as_uuid())
        .fetch_one(f.s.pool())
        .await
        .unwrap()
}
