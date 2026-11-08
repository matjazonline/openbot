use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{
    Abort, Gate, GatePool, Stall, connect, resolution_behind,
};
use crate::application::local_agents::workflow::LocalRunProgress;
use crate::application::workflow::actions::ActionService;

impl GatePool for Setup {
    fn gate_pool(&self) -> &sqlx::PgPool {
        self.fixture.persistence.pool()
    }
}

#[tokio::test]
async fn cloudroom_workflow_lost_park_commit_ack_proves_atomic_saved_owner_or_no_operation() {
    let _injected = Injected::mark();
    for applies in [true, false] {
        let s = Setup::new().await;
        let claim = s.claim().await;
        let (mut observer, _) = connect(&s).await;
        let mut gate = Gate::arm(
            &s,
            "local_agent_operations",
            "INSERT",
            "true",
            Stall::Commit,
        )
        .await;
        let runtime = ActionService::new(local_agents::PostgresLocalAgentWorkflow::new(
            s.fixture.persistence.clone(),
            None,
        ));
        let inputs = serde_json::from_value(claim.activation.inputs.clone()).unwrap();
        let task =
            tokio::spawn(async move { runtime.run_local_agent(&claim, &inputs, lease()).await });
        let stalled = gate
            .stalled(&mut observer, &[&|| !task.is_finished()])
            .await;
        if !applies {
            gate.abort_on_release(Abort::Backend(stalled)).await;
        }
        resolution_behind(&mut observer, stalled, lease(), &[&|| !task.is_finished()]).await;
        gate.release().await;
        let result = task.await.unwrap();
        if applies {
            assert!(matches!(result.unwrap(), LocalRunProgress::Suspended(_)));
            assert_eq!(
                s.rows().await,
                json!({"calls":1,"operations":1,"intents":1,"markers":1,"receipts":0,"waits":1})
            );
        } else {
            assert!(result.is_err());
            assert_eq!(
                s.rows().await,
                json!({"calls":0,"operations":0,"intents":0,"markers":0,"receipts":0,"waits":0})
            );
        }
    }
}

#[tokio::test]
async fn cloudroom_workflow_lost_result_commit_ack_resolves_authorized_saved_receipt() {
    let _injected = Injected::mark();
    let s = Setup::new().await;
    let claim = s.claim().await;
    s.park(&claim).await;
    s.native_success().await;
    s.fixture.persistence.resume_wait(s.scope).await.unwrap();
    let next = s.claim().await;
    let (mut observer, _) = connect(&s).await;
    let mut gate = Gate::arm(
        &s,
        "workflow_action_receipts",
        "INSERT",
        "true",
        Stall::Commit,
    )
    .await;
    let runtime = ActionService::new(local_agents::PostgresLocalAgentWorkflow::new(
        s.fixture.persistence.clone(),
        None,
    ));
    let inputs = serde_json::from_value(next.activation.inputs.clone()).unwrap();
    let task = tokio::spawn(async move { runtime.run_local_agent(&next, &inputs, lease()).await });
    let stalled = gate
        .stalled(&mut observer, &[&|| !task.is_finished()])
        .await;
    resolution_behind(&mut observer, stalled, lease(), &[&|| !task.is_finished()]).await;
    gate.release().await;
    let LocalRunProgress::Completed(result) = task.await.unwrap().unwrap() else {
        panic!("saved receipt must resolve result")
    };
    assert_eq!(result["text"], "native output");
    assert_eq!(s.rows().await["receipts"], 1);
}
