use super::*;

async fn control(f: &ModelFixture, key: &str) -> CancelCommand {
    let scope = f.write.fence.scope;
    CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: f.native.fixture.binding.target.actor,
        command_key: IdempotencyKey::parse(key).unwrap(),
        expected_revision: f
            .native
            .fixture
            .persistence()
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    }
}

async fn fail(f: &ModelFixture) {
    use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("provider.rejected").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    });
    assert!(
        f.native
            .fixture
            .persistence()
            .settled()
            .release_io(f.write.fence, f.write.lease_policy, cause)
            .await
            .unwrap()
    );
}

async fn retry_failed(f: &ModelFixture) {
    let command = control(f, "history-retry").await;
    let result = f
        .native
        .fixture
        .persistence()
        .retry(RetryCommand {
            company_id: command.company_id,
            run_id: command.run_id,
            actor: command.actor,
            command_key: command.command_key,
            expected_revision: command.expected_revision,
        })
        .await
        .unwrap();
    assert!(matches!(result, RetryResult::Applied { .. }));
}

#[tokio::test]
async fn agent_history_completion_cancel_before_acceptance_has_input_and_no_answer() {
    let f = Box::pin(settled_history()).await;
    let p = f.native.fixture.persistence();
    raise(&f, 5).await;
    assert!(matches!(
        p.cancel(control(&f, "cancel-before").await).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    let saved = exchange(&f).await;
    assert_eq!(saved["state"], "cancelled");
    assert!(saved["output"].is_null());
    assert_eq!(saved["access_level"], 5);
    assert!(matches!(
        p.update_workflow_checkpoint(f.write, accepted(&f))
            .await
            .unwrap(),
        WriteOutcome::OwnershipLost
    ));
    assert_eq!(exchange(&f).await, saved);
    assert_eq!(accounting(&f).await["calls"], 1);
}

#[tokio::test]
async fn agent_history_completion_cancel_after_acceptance_preserves_the_accepted_exchange() {
    let f = Box::pin(settled_history()).await;
    let p = f.native.fixture.persistence();
    p.update_workflow_checkpoint(f.write, accepted(&f))
        .await
        .unwrap();
    let saved = exchange(&f).await;
    assert!(matches!(
        p.cancel(control(&f, "cancel-after").await).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    assert_eq!(exchange(&f).await, saved);
    assert_eq!(saved["state"], "completed");
    assert_eq!(accounting(&f).await["calls"], 1);
}

#[tokio::test]
async fn agent_history_completion_workflow_failure_and_control_retry_preserve_accepted_output() {
    let f = Box::pin(settled_history()).await;
    let p = f.native.fixture.persistence();
    p.update_workflow_checkpoint(f.write, accepted(&f))
        .await
        .unwrap();
    let saved = exchange(&f).await;
    fail(&f).await;
    assert_eq!(exchange(&f).await, saved);
    retry_failed(&f).await;
    assert_eq!(exchange(&f).await, saved);
    let claim = p
        .settled()
        .claim_io(f.write.fence.scope, worker(), f.write.lease_policy)
        .await
        .unwrap()
        .unwrap();
    let prepared = crate::application::workflow::agent::AgentPreparation::prepare_agent(
        p,
        &claim,
        f.write.lease_policy,
    )
    .await
    .unwrap()
    .expect_ready()
    .prepared;
    let WriteOutcome::AlreadyApplied(checkpoint) = p
        .open_workflow_checkpoint(
            claim.fence,
            &prepared.spec(claim.fence).unwrap(),
            f.write.lease_policy,
        )
        .await
        .unwrap()
    else {
        panic!("accepted output must replay")
    };
    assert_eq!(
        checkpoint
            .final_output
            .as_ref()
            .unwrap()
            .structured
            .as_ref()
            .unwrap()
            .body(),
        r#"{"answer":[null,3.5]}"#
    );
    assert_eq!(exchange(&f).await, saved);
    assert_eq!(accounting(&f).await["calls"], 1);
}

#[tokio::test]
async fn agent_history_completion_safe_control_retry_reactivates_same_checkpoint_without_refunding()
{
    let f = Box::pin(settled_history()).await;
    let p = f.native.fixture.persistence();
    let before = exchange(&f).await;
    let usage = accounting(&f).await;
    fail(&f).await;
    assert_eq!(exchange(&f).await["state"], "failed");
    let failed = exchange(&f).await;
    let deletion =
        sqlx::query("DELETE FROM agent_history_exchanges WHERE company_id=$1 AND execution_id=$2")
            .bind(f.write.fence.scope.company.as_uuid())
            .bind(f.write.fence.scope.execution.as_uuid())
            .execute(p.pool())
            .await
            .unwrap_err();
    assert_eq!(
        deletion.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert_eq!(exchange(&f).await, failed);
    retry_failed(&f).await;
    let queued = exchange(&f).await;
    assert_eq!(queued["state"], "queued");
    for field in [
        "id",
        "position",
        "input_digest",
        "provenance",
        "checkpoint_id",
        "conversation_id",
        "activated_at",
    ] {
        assert_eq!(queued[field], before[field], "{field}");
    }
    assert!(queued["finished_at"].is_null());
    let claim = p
        .settled()
        .claim_io(f.write.fence.scope, worker(), f.write.lease_policy)
        .await
        .unwrap()
        .unwrap();
    let prepared = crate::application::workflow::agent::AgentPreparation::prepare_agent(
        p,
        &claim,
        f.write.lease_policy,
    )
    .await
    .unwrap()
    .expect_ready()
    .prepared;
    let WriteOutcome::AlreadyApplied(checkpoint) = p
        .open_workflow_checkpoint(
            claim.fence,
            &prepared.spec(claim.fence).unwrap(),
            f.write.lease_policy,
        )
        .await
        .unwrap()
    else {
        panic!("existing checkpoint expected")
    };
    assert_eq!(checkpoint, f.run);
    assert_eq!(
        exchange(&f).await["state"],
        "active",
        "AlreadyApplied open must still commit reactivation"
    );
    assert_eq!(accounting(&f).await, usage);
    assert!(matches!(
        p.update_workflow_checkpoint(f.write, accepted(&f))
            .await
            .unwrap(),
        WriteOutcome::OwnershipLost
    ));
    let write = WorkflowCheckpointWrite {
        fence: claim.fence,
        ..f.write
    };
    let WriteOutcome::Applied(checkpoint) =
        p.reserve_workflow_execution(write, 1000).await.unwrap()
    else {
        panic!("new owner reserves time")
    };
    let write = WorkflowCheckpointWrite {
        expected_revision: checkpoint.revision,
        ..write
    };
    assert!(matches!(
        p.update_workflow_checkpoint(write, accepted(&f))
            .await
            .unwrap(),
        WriteOutcome::Applied(_)
    ));
    assert_eq!(exchange(&f).await["state"], "completed");
    assert_eq!(accounting(&f).await, usage);
}

#[path = "agent_history_retry_deletion_tests.rs"]
mod deletion;

#[path = "agent_history_reopen_ambiguity_tests.rs"]
mod ambiguity;
