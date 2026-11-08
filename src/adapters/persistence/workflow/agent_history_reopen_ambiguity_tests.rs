use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{Abort,Gate,Stall,connect,resolution_behind};
use crate::application::workflow::agent::AgentPreparation;

struct Reopen {
    fixture: ModelFixture,
    claim: ClaimedWorkflow,
    spec: WorkflowCheckpointSpec,
    outcome: AppResult<WriteOutcome<RunCheckpoint>>,
    before: Value,
}

async fn ambiguous(abort: Abort) -> Reopen {
    let f = Box::pin(settled_history()).await;
    fail(&f).await;
    retry_failed(&f).await;
    let p = f.native.fixture.persistence();
    let policy = LeasePolicy::new(Duration::from_secs(6)).unwrap();
    let claim = p
        .claim_io(f.write.fence.scope, worker(), f.write.lease_policy)
        .await
        .unwrap()
        .unwrap();
    let prepared = p
        .prepare_agent(&claim, policy)
        .await
        .unwrap()
        .expect_ready()
        .prepared;
    let spec = prepared.spec(claim.fence).unwrap();
    let task_spec = prepared.spec(claim.fence).unwrap();
    let before = exchange(&f).await;
    assert_eq!(before["state"], "queued");
    let mut gate = Gate::arm(
        &f.native.fixture,
        "agent_history_exchanges",
        "UPDATE",
        "OLD.state='queued' AND NEW.state='active'",
        Stall::Commit,
    )
    .await;
    let pending = {
        let p = p.clone();
        let fence = claim.fence;
        tokio::spawn(async move { p.open_workflow_checkpoint(fence, &task_spec, policy).await })
    };
    let (mut observer, _) = connect(&f.native.fixture).await;
    let stalled = gate
        .stalled(&mut observer, &[&|| !pending.is_finished()])
        .await;
    resolution_behind(
        &mut observer,
        stalled,
        policy,
        &[&|| !pending.is_finished()],
    )
    .await;
    gate.abort_on_release(abort).await;
    gate.drain().await;
    let outcome = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap();
    gate.abort_on_release(Abort::Nothing).await;
    assert_eq!(accounting(&f).await["calls"], 1);
    let saved = exchange(&f).await;
    for field in [
        "id",
        "position",
        "input_digest",
        "provenance",
        "checkpoint_id",
        "activated_at",
    ] {
        assert_eq!(saved[field], before[field], "{field}");
    }
    Reopen {
        fixture: f,
        claim,
        spec,
        outcome,
        before,
    }
}

#[tokio::test]
async fn agent_history_completion_ambiguous_reopen_committed_proves_reactivation_and_current_refusal()
 {
    let _injected = Injected::mark();
    let result = ambiguous(Abort::Nothing).await;
    let f = &result.fixture;
    let p = f.native.fixture.persistence();
    assert!(matches!(result.outcome,Ok(WriteOutcome::AlreadyApplied(ref run)) if run==&f.run));
    let saved = exchange(f).await;
    assert_eq!(saved["state"], "active");
    assert!(matches!(
        p.open_workflow_checkpoint(f.write.fence, &result.spec, f.write.lease_policy)
            .await
            .unwrap(),
        WriteOutcome::OwnershipLost
    ));
    sqlx::query("DELETE FROM company_model_connections WHERE company_id=$1")
        .bind(f.write.fence.scope.company.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    assert!(
        p.prepare_agent(&result.claim, f.write.lease_policy)
            .await
            .is_err()
    );
    let write = WorkflowCheckpointWrite {
        fence: result.claim.fence,
        ..f.write
    };
    assert!(
        p.reserve_workflow_model(write, model_request(&reservation()), None)
            .await
            .is_err()
    );
    assert_eq!(exchange(f).await, saved);
    assert_eq!(accounting(f).await["calls"], 1);
}

#[tokio::test]
async fn agent_history_completion_ambiguous_reopen_aborted_reports_missing_effect_and_recovers() {
    let _injected = Injected::mark();
    let result = ambiguous(Abort::Every).await;
    let f = &result.fixture;
    let p = f.native.fixture.persistence();
    assert!(
        result.outcome.is_err(),
        "old checkpoint cannot prove missing activation: {:?}",
        result.outcome
    );
    assert_eq!(exchange(f).await, result.before);
    let reopened = p
        .open_workflow_checkpoint(result.claim.fence, &result.spec, f.write.lease_policy)
        .await
        .unwrap();
    assert!(matches!(reopened,WriteOutcome::AlreadyApplied(ref run) if run==&f.run));
    assert_eq!(exchange(f).await["state"], "active");
    assert!(matches!(
        p.open_workflow_checkpoint(f.write.fence, &result.spec, f.write.lease_policy)
            .await
            .unwrap(),
        WriteOutcome::OwnershipLost
    ));
    assert_eq!(accounting(f).await["calls"], 1);
}
