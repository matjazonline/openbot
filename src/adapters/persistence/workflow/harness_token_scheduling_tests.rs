use super::*;

pub(super) async fn exceed(f: &TokenFixture) {
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let mut request = reservation();
    request.input_tokens = 50;
    request.output_tokens = 50;
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(model.write, model_request(&request), Some(&notices(p)))
        .await
        .unwrap()
    else {
        panic!("admission failed")
    };
    let run = permit.consume();
    let mut turn = response(&request);
    turn.input_tokens = 100;
    turn.output_tokens = 100;
    assert!(matches!(
        p.settle_workflow_model(
            WorkflowCheckpointWrite {
                expected_revision: run.revision,
                ..model.write
            },
            turn,
            Some(&notices(p))
        )
        .await
        .unwrap(),
        WorkflowModelSettlement::Parked { .. }
    ));
}

async fn job(f: &TokenFixture, scope: ActivationRequest) -> Value {
    sqlx::query_scalar(
        "SELECT to_jsonb(job)-'updated_at'-'run_at' FROM background_tasks AS job WHERE id=$1",
    )
    .bind(scope.job.0)
    .fetch_one(f.model.native.fixture.persistence().pool())
    .await
    .unwrap()
}

async fn due(f: &TokenFixture, scope: ActivationRequest) {
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(scope.job.0)
    .execute(f.model.native.fixture.persistence().pool())
    .await
    .unwrap();
}

#[tokio::test]
async fn workflow_harness_token_pending_pause_preserves_attempts_and_approval_releases() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let root = f.model.write.fence.scope;
    let child = refusal::child(&f, root).await;
    exceed(&f).await;
    let before = job(&f, child).await;
    let policy = f.model.write.lease_policy;
    for _ in 0..2 {
        assert!(
            p.settled()
                .claim_io(child, worker(), policy)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(job(&f, child).await, before);
    }
    let deferred: bool =
        sqlx::query_scalar("SELECT run_at>clock_timestamp() FROM background_tasks WHERE id=$1")
            .bind(child.job.0)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert!(deferred);
    p.submit_decision(command(p, root, f.reviewer, "approve").await)
        .await
        .unwrap();
    assert!(
        p.settled()
            .claim_io(child, worker(), policy)
            .await
            .unwrap()
            .is_none()
    );
    due(&f, child).await;
    assert!(
        p.settled()
            .claim_io(child, worker(), policy)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn workflow_harness_token_processing_pause_charges_one_failure_only() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let child = refusal::child(&f, f.model.write.fence.scope).await;
    let policy = f.model.write.lease_policy;
    let claim = p
        .settled()
        .claim_io(child, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    exceed(&f).await;
    assert!(
        p.settled()
            .complete_io(FencedWorkflowResult {
                fence: claim.fence,
                output: json!({"items":[],"token_count":0}),
                policy
            })
            .await
            .unwrap()
            .is_none()
    );
    let after = job(&f, child).await;
    assert_eq!(after["status"], "pending");
    assert_eq!(after["retry_count"], 1);
    let code: String = sqlx::query_scalar(
        "SELECT workflow_failure_code FROM task_attempts WHERE task_id=$1 AND attempt_number=$2",
    )
    .bind(child.job.0)
    .bind(claim.fence.attempt.0)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(code, "workflow.tokens_awaiting_approval");
    due(&f, child).await;
    for _ in 0..2 {
        assert!(
            p.settled()
                .claim_io(child, worker(), policy)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(job(&f, child).await, after);
    }
}

#[tokio::test]
async fn workflow_harness_token_pause_last_attempt_terminalizes() {
    use crate::application::workflow::lease::LeaseReleaseCause;
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let child = refusal::child(&f, f.model.write.fence.scope).await;
    let policy = f.model.write.lease_policy;
    let max = job(&f, child).await["max_retries"].as_i64().unwrap();
    for _ in 1..max {
        let claim = p
            .settled()
            .claim_io(child, worker(), policy)
            .await
            .unwrap()
            .unwrap();
        assert!(
            p.settled()
                .release_io(claim.fence, policy, LeaseReleaseCause::LocalInterruption)
                .await
                .unwrap()
        );
        due(&f, child).await;
    }
    let claim = p
        .settled()
        .claim_io(child, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    exceed(&f).await;
    assert!(
        p.settled()
            .complete_io(FencedWorkflowResult {
                fence: claim.fence,
                output: json!({"items":[],"token_count":0}),
                policy
            })
            .await
            .unwrap()
            .is_none()
    );
    let after = job(&f, child).await;
    assert_eq!(after["status"], "failed");
    assert_eq!(after["retry_count"], max);
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(child.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(state, "failed");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE run_id=$1")
        .bind(child.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn workflow_harness_token_deferred_sibling_retires_after_denial() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let root = f.model.write.fence.scope;
    let child = refusal::child(&f, root).await;
    let policy = f.model.write.lease_policy;
    exceed(&f).await;
    assert!(
        p.settled()
            .claim_io(child, worker(), policy)
            .await
            .unwrap()
            .is_none()
    );
    p.submit_decision(command(p, root, f.reviewer, "reject").await)
        .await
        .unwrap();
    due(&f, child).await;
    assert!(
        p.settled()
            .claim_io(child, worker(), policy)
            .await
            .unwrap()
            .is_none()
    );
    let after = job(&f, child).await;
    assert_eq!(after["status"], "failed");
    assert_eq!(after["retry_count"], 0);
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1")
        .bind(child.job.0)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 0);
}

#[tokio::test]
async fn workflow_harness_token_competing_deferred_claims_do_not_lock_ancestor() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let root = f.model.write.fence.scope;
    let child = refusal::child(&f, root).await;
    let policy = f.model.write.lease_policy;
    exceed(&f).await;
    let before = job(&f, child).await;
    let mut ancestor = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(root.run.as_uuid())
        .execute(&mut *ancestor)
        .await
        .unwrap();
    let settled = p.settled();
    let claims = async {
        tokio::join!(
            settled.claim_io(child, worker(), policy),
            settled.claim_io(child, worker(), policy)
        )
    };
    let (left, right) = tokio::time::timeout(std::time::Duration::from_secs(3), claims)
        .await
        .unwrap();
    assert!(left.unwrap().is_none());
    assert!(right.unwrap().is_none());
    assert_eq!(job(&f, child).await, before);
    ancestor.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_harness_token_pure_pending_reports_deferral_and_resumes() {
    use crate::application::workflow::batch::{BatchBudget, BatchDisposition, WorkflowBatch};
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    let end = source["steps"]["start"]["routes"]["success"].clone();
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"data.map","with":{
        "value":{"literal":10},"output_schema":{"literal":{"type":"integer"}}},
        "routes":{"success":end}});
    let f = configured_source(source).await;
    let p = f.model.native.fixture.persistence();
    let root = f.model.write.fence.scope;
    let child = refusal::child(&f, root).await;
    let policy = f.model.write.lease_policy;
    let claim = p
        .settled()
        .claim_io(child, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    let pure = p
        .settled()
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output: json!({"items":[],"token_count":0}),
            policy,
        })
        .await
        .unwrap()
        .unwrap()
        .successor
        .unwrap();
    exceed(&f).await;
    let budget = BatchBudget::new(10, std::time::Duration::from_secs(1)).unwrap();
    let before = job(&f, pure).await;
    let result = p.advance_pure(pure, budget).await.unwrap();
    assert_eq!(result.disposition, BatchDisposition::Deferred);
    assert_eq!(result.completed, 0);
    assert!(result.continuation.is_none());
    assert_eq!(job(&f, pure).await, before);
    assert!(p.advance_pure(pure, budget).await.is_err());
    p.submit_decision(command(p, root, f.reviewer, "approve").await)
        .await
        .unwrap();
    due(&f, pure).await;
    let result = p.advance_pure(pure, budget).await.unwrap();
    assert_eq!(result.disposition, BatchDisposition::Completed);
    assert_eq!(result.completed, 1);
}

#[tokio::test]
async fn workflow_harness_token_deferral_is_capped_by_run_deadline() {
    use crate::application::workflow::polling::WorkflowPolling;
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let child = refusal::child(&f, f.model.write.fence.scope).await;
    exceed(&f).await;
    sqlx::query(
        "UPDATE workflow_runs SET deadline=clock_timestamp()+interval '4 seconds' WHERE id=$1",
    )
    .bind(child.run.as_uuid())
    .execute(p.pool())
    .await
    .unwrap();
    assert!(
        p.settled()
            .claim_io(child, worker(), f.model.write.lease_policy)
            .await
            .unwrap()
            .is_none()
    );
    let capped: bool = sqlx::query_scalar("SELECT job.run_at=run.deadline FROM background_tasks AS job JOIN workflow_runs AS run ON run.id=$2 WHERE job.id=$1")
        .bind(child.job.0).bind(child.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(capped);
    sqlx::query(
        "UPDATE workflow_runs SET deadline=clock_timestamp()-interval '1 millisecond' WHERE id=$1",
    )
    .bind(child.run.as_uuid())
    .execute(p.pool())
    .await
    .unwrap();
    assert!(p.expire_run(child).await.unwrap());
    let after = job(&f, child).await;
    assert_eq!(after["status"], "failed");
    assert_eq!(after["retry_count"], 0);
}

#[tokio::test]
async fn workflow_harness_token_pause_cannot_replace_hard_model_budget_failure() {
    use crate::application::workflow::budget::{
        BudgetDisposition, BudgetReservation, BudgetReservationKey, BudgetReservationResult,
    };
    use crate::domain::workflow::{BudgetCharge, BudgetResource};
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let child = refusal::child(&f, f.model.write.fence.scope).await;
    let policy = f.model.write.lease_policy;
    let claim = p
        .settled()
        .claim_io(child, worker(), policy)
        .await
        .unwrap()
        .unwrap();
    exceed(&f).await;
    let charge = BudgetReservation::new(
        claim.fence,
        BudgetReservationKey::parse("hard-limit").unwrap(),
        BudgetCharge::new(BudgetResource::ModelCall, 10).unwrap(),
    )
    .unwrap();
    assert_eq!(
        p.settled().reserve_budget(charge, policy).await.unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Exhausted)
    );
    let failure: Value = sqlx::query_scalar("SELECT jsonb_build_object('code',workflow_failure_code,'class',workflow_failure_class) FROM task_attempts WHERE task_id=$1 AND attempt_number=$2")
        .bind(child.job.0).bind(claim.fence.attempt.0).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        failure,
        json!({"code":"workflow.root_budget_exhausted","class":"terminal"})
    );
    assert_eq!(job(&f, child).await["status"], "failed");
}
