use super::*;
use crate::application::workflow::waits::WorkflowWaits;

/// Category B (07-D1 §D1.2a): a descendant running beside its agent ancestor, which keeps its
/// own claim and token state, is a state the runtime never produces (a parent parks on its
/// child); these cases guard the shared root token budget against it.
pub(super) async fn child(f: &TokenFixture, parent: ActivationRequest) -> ActivationRequest {
    f.model.native.fixture.lineage_child(parent).await
}

pub(super) async fn park(f: &TokenFixture) {
    let model = &f.model;
    let p = model.native.fixture.persistence();
    assert!(matches!(
        p.reserve_workflow_model(
            model.write,
            model_request(&reservation()),
            Some(&notices(p))
        )
        .await
        .unwrap(),
        WorkflowModelAdmission::Parked(_)
    ));
}

pub(super) async fn child_checkpoint(f: &TokenFixture) -> WorkflowCheckpointWrite {
    let scope = child(f, f.model.write.fence.scope).await;
    let p = f.model.native.fixture.persistence();
    let policy = f.model.write.lease_policy;
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    let mut identity = f.model.run.identity.clone();
    identity.task_id = scope.job.0;
    let spec = WorkflowCheckpointSpec {
        identity,
        initial_prompt: "untrusted child context".into(),
        policy: f.model.run.policy,
        prepared: f.model.native.row["workflow_prepared"].clone(),
    };
    let WriteOutcome::Applied(run) = p
        .open_workflow_checkpoint(claim.fence, &spec, policy)
        .await
        .unwrap()
    else {
        panic!("child open failed")
    };
    let write = WorkflowCheckpointWrite {
        fence: claim.fence,
        checkpoint: run.run_id,
        expected_revision: run.revision,
        lease_policy: policy,
    };
    let WriteOutcome::Applied(run) = p.reserve_workflow_execution(write, 1000).await.unwrap()
    else {
        panic!("child execution failed")
    };
    WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..write
    }
}

#[tokio::test]
async fn workflow_harness_token_sibling_usage_reparks_exact_request_without_debit() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let child = child_checkpoint(&f).await;
    park(&f).await;
    let paused = stored(&f.model.native.fixture, f.model.run.run_id.0).await;
    let approve = command(p, f.model.write.fence.scope, f.reviewer, "approve").await;
    p.submit_decision(approve.clone()).await.unwrap();
    assert!(matches!(
        p.reserve_workflow_model(child, model_request(&reservation()), Some(&notices(p)))
            .await
            .unwrap(),
        WorkflowModelAdmission::New(_)
    ));
    let write = resumed(&f.model, &paused).await;
    let decision: Value = sqlx::query_scalar(
        "SELECT token_proposal FROM workflow_decisions WHERE company_id=$1 AND id=$2",
    )
    .bind(write.fence.scope.company.as_uuid())
    .bind(Uuid::parse_str(paused["model_blocked_decision"].as_str().unwrap()).unwrap())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let reservation: ModelReservation =
        serde_json::from_value(decision["request"]["reservation"].clone()).unwrap();
    assert!(matches!(
        p.reserve_workflow_model(write, model_request(&reservation), Some(&notices(p)))
            .await
            .unwrap(),
        WorkflowModelAdmission::Parked(_)
    ));
    let reparking = stored(&f.model.native.fixture, f.model.run.run_id.0).await;
    assert_ne!(
        reparking["model_blocked_decision"],
        paused["model_blocked_decision"]
    );
    assert_eq!(accounting(&f.model).await["calls"], 1);
    assert_eq!(
        accounting(&f.model).await["rows"].as_array().unwrap().len(),
        1
    );
    p.submit_decision(approve).await.unwrap();
    assert_eq!(
        stored(&f.model.native.fixture, f.model.run.run_id.0).await,
        reparking
    );
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(write.fence.scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(state, "waiting");
}

#[tokio::test]
async fn workflow_harness_token_approved_request_reparks_when_usage_itself_exceeds_allowance() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let child = child_checkpoint(&f).await;
    park(&f).await;
    let paused = stored(&f.model.native.fixture, f.model.run.run_id.0).await;
    p.submit_decision(command(p, f.model.write.fence.scope, f.reviewer, "approve").await)
        .await
        .unwrap();
    let request = reservation();
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(child, model_request(&request), Some(&notices(p)))
        .await
        .unwrap()
    else {
        panic!("child admission failed")
    };
    let run = permit.consume();
    let mut turn = response(&request);
    turn.input_tokens = 200;
    turn.output_tokens = 200;
    assert!(matches!(
        p.settle_workflow_model(
            WorkflowCheckpointWrite {
                expected_revision: run.revision,
                ..child
            },
            turn,
            Some(&notices(p))
        )
        .await
        .unwrap(),
        WorkflowModelSettlement::Parked { .. }
    ));
    // The root's approved reservation still needs a real claim to produce its
    // next exact proposal, even though the generic pending gate is closed.
    let write = resumed(&f.model, &paused).await;
    let proposal: Value =
        sqlx::query_scalar("SELECT token_proposal FROM workflow_decisions WHERE id=$1")
            .bind(Uuid::parse_str(paused["model_blocked_decision"].as_str().unwrap()).unwrap())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let saved: ModelReservation =
        serde_json::from_value(proposal["request"]["reservation"].clone()).unwrap();
    assert!(matches!(
        p.reserve_workflow_model(write, model_request(&saved), Some(&notices(p)))
            .await
            .unwrap(),
        WorkflowModelAdmission::Parked(_)
    ));
    assert_eq!(accounting(&f.model).await["calls"], 1);
}

#[tokio::test]
async fn workflow_harness_token_inflight_settlement_retains_truth_after_sibling_denial() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let child = child_checkpoint(&f).await;
    let mut request = reservation();
    request.input_tokens = 50;
    request.output_tokens = 50;
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(child, model_request(&request), Some(&notices(p)))
        .await
        .unwrap()
    else {
        panic!("child admission failed")
    };
    let run = permit.consume();
    let write = WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..child
    };
    park(&f).await;
    p.submit_decision(command(p, f.model.write.fence.scope, f.reviewer, "reject").await)
        .await
        .unwrap();
    let turn = response(&request);
    let WorkflowModelSettlement::Refused(saved) = p
        .settle_workflow_model(write, turn.clone(), Some(&notices(p)))
        .await
        .unwrap()
    else {
        panic!("refusal lost")
    };
    assert_eq!(saved.turns.last(), Some(&turn));
    let before = accounting(&f.model).await;
    assert_eq!(before["rows"][0]["input_tokens"], 70);
    assert_eq!(before["rows"][0]["output_tokens"], 50);
    assert!(matches!(
        p.settle_workflow_model(write, turn, Some(&notices(p)))
            .await
            .unwrap(),
        WorkflowModelSettlement::Refused(_)
    ));
    assert_eq!(accounting(&f.model).await, before);
    let write = WorkflowCheckpointWrite {
        expected_revision: saved.revision,
        ..write
    };
    assert!(matches!(
        p.reserve_workflow_model(write, model_request(&reservation()), Some(&notices(p)))
            .await
            .unwrap(),
        WorkflowModelAdmission::TokenRefused
    ));
}

#[tokio::test]
async fn workflow_harness_token_refusal_stops_admitted_descendants_and_inflight_output() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let root = f.model.write.fence.scope;
    let first = child(&f, root).await;
    let second = child(&f, first).await;
    let policy = f.model.write.lease_policy;
    let claim = p
        .settled()
        .claim_io(first, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    park(&f).await;
    p.submit_decision(command(p, root, f.reviewer, "reject").await)
        .await
        .unwrap();
    assert!(
        p.settled()
            .claim_io(second, worker(), policy)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        p.settled()
            .complete_io(FencedWorkflowResult {
                fence: claim.fence,
                output: json!({}),
                policy
            })
            .await
            .unwrap()
            .is_none()
    );
    let code: String = sqlx::query_scalar(
        "SELECT workflow_failure_code FROM task_attempts WHERE task_id=$1 AND attempt_number=$2",
    )
    .bind(first.job.0)
    .bind(claim.fence.attempt.0)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(code, "workflow.tokens_rejected");
    let states: Vec<String> =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=ANY($1)")
            .bind(vec![first.run.as_uuid(), second.run.as_uuid()])
            .fetch_all(p.pool())
            .await
            .unwrap();
    assert_eq!(states, vec!["failed", "failed"]);
    assert!(
        crate::adapters::persistence::workflow::token_budget::require_progression(
            &mut p.pool().acquire().await.unwrap(),
            first
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn workflow_harness_token_expiry_is_terminal_and_late_approval_cannot_clear_refusal() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let scope = f.model.write.fence.scope;
    park(&f).await;
    let approve = command(p, scope, f.reviewer, "approve").await;
    sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1 AND state='waiting'")
        .bind(scope.run.as_uuid()).execute(p.pool()).await.unwrap();
    p.resume_wait(scope).await.unwrap();
    assert!(p.submit_decision(approve).await.is_err());
    let refusal: String = sqlx::query_scalar(
        "SELECT token_refusal FROM workflow_root_budget_usage WHERE company_id=$1",
    )
    .bind(scope.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(refusal, "expired");
    assert_eq!(accounting(&f.model).await["rows"], json!([]));
}

#[tokio::test]
async fn workflow_harness_token_cancel_and_approval_contend_without_late_resurrection() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let scope = f.model.write.fence.scope;
    park(&f).await;
    let approve = command(p, scope, f.reviewer, "approve").await;
    let cancel = CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: f.model.native.fixture.binding.target.actor,
        command_key: IdempotencyKey::parse("cancel-token").unwrap(),
        expected_revision: p
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    let (approval, cancellation) = tokio::join!(p.submit_decision(approve), p.cancel(cancel));
    let cancellation = cancellation.unwrap();
    if matches!(cancellation, CancelResult::Applied { .. }) {
        assert!(approval.is_err());
        let refusal: String = sqlx::query_scalar(
            "SELECT token_refusal FROM workflow_root_budget_usage WHERE company_id=$1",
        )
        .bind(scope.company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        assert_eq!(refusal, "cancelled");
        assert!(
            p.settled()
                .claim_io(scope, worker(), f.model.write.lease_policy)
                .await
                .unwrap()
                .is_none()
        );
    } else {
        assert!(matches!(
            cancellation,
            CancelResult::RevisionConflict { .. }
        ));
        approval.unwrap();
    }
    assert_eq!(accounting(&f.model).await["rows"], json!([]));
}
