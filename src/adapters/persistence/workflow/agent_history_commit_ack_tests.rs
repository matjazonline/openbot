use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{Abort,Gate,GatePool,Stall,connect,resolution_behind};
use crate::application::workflow::agent::AgentPreparationResult;

struct HistoryPool<'a>(&'a PostgresPersistence);
impl GatePool for HistoryPool<'_> {
    fn gate_pool(&self) -> &sqlx::PgPool {
        self.0.pool()
    }
}
struct EnqueueAck {
    setup: AgentSetup,
    outcome: AppResult<AgentPreparationResult>,
}

async fn prepare_ambiguously(abort: Abort) -> EnqueueAck {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let p = &setup.fixture.persistence;
    let fixture = HistoryPool(p);
    let mut gate = Gate::arm(
        &fixture,
        "agent_history_exchanges",
        "INSERT",
        "true",
        Stall::Commit,
    )
    .await;
    let (mut observer, _) = connect(&fixture).await;
    let pending = {
        let p = p.clone();
        let claim = setup.claim.clone();
        tokio::spawn(async move { p.prepare_agent(&claim, lease()).await })
    };
    let stalled = gate
        .stalled(&mut observer, &[&|| !pending.is_finished()])
        .await;
    resolution_behind(
        &mut observer,
        stalled,
        lease(),
        &[&|| !pending.is_finished()],
    )
    .await;
    gate.abort_on_release(abort).await;
    gate.drain().await;
    let outcome = pending.await.unwrap();
    gate.abort_on_release(Abort::Nothing).await;
    assert_eq!(usage_rows(p, setup.claim.fence.scope.run).await, 0);
    EnqueueAck { setup, outcome }
}

#[tokio::test]
async fn agent_history_storage_ambiguous_enqueue_committed_reuses_one_position_before_checkpoint() {
    let injected = Injected::mark();
    let ack = prepare_ambiguously(Abort::Nothing).await;
    assert!(ack.outcome.is_ok());
    assert!(injected.outcomes().iter().any(|event|event.op=="agent_history.prepare" && event.outcome=="resolved_applied"));
    let rows = captured(&ack.setup).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, ack.setup.claim.fence.scope.execution.as_uuid());
    assert_eq!(rows[0].position, 1);
    assert_eq!(rows[0].state, "queued");
    assert!(rows[0].checkpoint_id.is_none());
    ack.setup
        .fixture
        .persistence
        .prepare_agent(&ack.setup.claim, lease())
        .await
        .unwrap()
        .expect_ready();
    assert_eq!(captured(&ack.setup).await, rows);
}

#[tokio::test]
async fn agent_history_storage_ambiguous_enqueue_aborted_keeps_counter_available_for_recovery() {
    let injected = Injected::mark();
    let ack = prepare_ambiguously(Abort::Every).await;
    assert!(ack.outcome.is_err());
    assert!(injected.outcomes().iter().any(
        |event| event.op == "agent_history.prepare" && event.outcome == "resolved_not_applied"
    ));
    assert!(captured(&ack.setup).await.is_empty());
    ack.setup
        .fixture
        .persistence
        .prepare_agent(&ack.setup.claim, lease())
        .await
        .unwrap()
        .expect_ready();
    let rows = captured(&ack.setup).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].position, 1);
    assert_eq!(rows[0].id, ack.setup.claim.fence.scope.execution.as_uuid());
    assert!(rows[0].checkpoint_id.is_none());
}

#[tokio::test]
async fn agent_history_completion_actual_final_commit_ack_loss_replays_without_another_provider_call()
 {
    let injected = Injected::mark();
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"accepted\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let fixture = HistoryPool(&setup.fixture.persistence);
    let mut gate = Gate::arm(
        &fixture,
        "task_harness_runs",
        "UPDATE",
        "NEW.state='completed'",
        Stall::Commit,
    )
    .await;
    let (mut observer, _) = connect(&fixture).await;
    let pending = {
        let runtime = agent_runtime(&setup);
        let claim = setup.claim.clone();
        tokio::spawn(async move {
            Box::pin(runtime.execute(&WorkflowStepKind("agent.run".into()), &claim)).await
        })
    };
    gate.stalled(&mut observer, &[&|| !pending.is_finished()])
        .await;
    let first = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap();
    assert!(first.completed_result().is_err());
    assert!(
        injected
            .outcomes()
            .iter()
            .any(|event| event.op == "checkpoint.update" && event.outcome == "unknown")
    );
    gate.drain().await;
    let rows = captured(&setup).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "completed");
    assert_eq!(rows[0].output, Some(json!("accepted")));
    let replay = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await;
    assert_eq!(replay.completed_result().unwrap(), json!("accepted"));
    assert_eq!(captured(&setup).await, rows);
    assert_eq!(
        usage_rows(&setup.fixture.persistence, setup.claim.fence.scope.run).await,
        1
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
}
