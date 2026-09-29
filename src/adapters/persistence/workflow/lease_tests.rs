use super::*;
use crate::application::workflow::{activation::*, lease::*};
use std::time::Duration;
use tokio::{sync::Barrier, time::Instant};

#[path = "capacity_tests.rs"]
mod capacity_tests;

fn policy() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(3)).unwrap()
}
fn worker() -> WorkflowWorkerId {
    WorkflowWorkerId(Uuid::new_v4())
}
async fn fixture() -> (AdmissionFixture, ActivationRequest) {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object","properties":{"value":{"type":"integer","minimum":1,"maximum":1000}},"required":["value"]});
    source["steps"]["start"]["with"]["max_tokens"] = json!({"ref":"/input/value"});
    fixture_source(source).await
}
async fn fixture_source(source: Value) -> (AdmissionFixture, ActivationRequest) {
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(source).await).await;
    let command = f.prepare(f.request("lease", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    (f, scope)
}
async fn snapshot(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('executions',(SELECT jsonb_agg(to_jsonb(execution)) FROM workflow_executions AS execution),'jobs',(SELECT jsonb_agg(to_jsonb(job)) FROM background_tasks AS job),'attempts',(SELECT jsonb_agg(to_jsonb(attempt)) FROM task_attempts AS attempt),'runs',(SELECT jsonb_agg(to_jsonb(run)) FROM workflow_runs AS run))").fetch_one(f.persistence().pool()).await.unwrap()
}
async fn expire(f: &AdmissionFixture, scope: ActivationRequest) {
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(scope.job.0).execute(f.persistence().pool()).await.unwrap();
}
async fn due(f: &AdmissionFixture, scope: ActivationRequest) {
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(scope.job.0)
    .execute(f.persistence().pool())
    .await
    .unwrap();
}
#[tokio::test]
async fn workflow_io_real_competing_claims_lost_ack_crash_backoff_and_frozen_retry() {
    let (f, scope) = fixture().await;
    let barrier = Barrier::new(2);
    let barrier = &barrier;
    let f_ref = &f;
    let claim = |worker| async move {
        barrier.wait().await;
        f_ref
            .persistence()
            .claim_io(scope, worker, policy())
            .await
            .unwrap()
    };
    let (left, right) = tokio::join!(claim(worker()), claim(worker()));
    assert_eq!(
        usize::from(left.is_some()) + usize::from(right.is_some()),
        1
    );
    let first = left.or(right).unwrap();
    let before = snapshot(&f).await;
    // A lost acknowledgment never authorizes another invocation.
    assert!(
        f.persistence()
            .claim_io(scope, first.fence.worker, policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(snapshot(&f).await, before);
    expire(&f, scope).await;
    assert!(
        !f.persistence()
            .validate_io(first.fence, policy())
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .renew_io(first.fence, policy())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !f.persistence()
            .release_io(first.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let after = snapshot(&f).await;
    assert_eq!(after["jobs"][0]["retry_count"], 1);
    assert_eq!(after["attempts"][0]["stop_reason"], "lease_lost");
    assert!(
        f.persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(snapshot(&f).await, after);
    sqlx::query("UPDATE workflow_runs SET input = '{\"value\":999}' WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    due(&f, scope).await;
    let second = f
        .persistence()
        .claim_io(scope, first.fence.worker, policy())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first.fence.generation, second.fence.generation);
    assert_eq!(second.fence.attempt, WorkflowAttempt(2));
    assert_eq!(first.activation, second.activation);
    assert_eq!(second.activation.inputs["max_tokens"], 1);
    assert!(
        !f.persistence()
            .release_io(first.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .renew_io(first.fence, policy())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !f.persistence()
            .validate_io(first.fence, policy())
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .release_io(second.fence, policy(), LeaseReleaseCause::Deadline)
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    due(&f, scope).await;
    let third = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(third.fence.attempt, WorkflowAttempt(3));
    assert!(
        f.persistence()
            .release_io(third.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    due(&f, scope).await;
    assert!(
        f.persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let final_state = snapshot(&f).await;
    assert_eq!(final_state["attempts"].as_array().unwrap().len(), 3);
    assert_eq!(final_state["jobs"][0]["retry_count"], 3);
    let reason: Option<String> = sqlx::query_scalar(
        "SELECT stop_reason FROM task_attempts WHERE task_id=$1 AND attempt_number=2",
    )
    .bind(scope.job.0)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(reason.as_deref(), Some("timed_out"));
}
#[tokio::test]
async fn workflow_io_claim_rolls_back_at_each_write_boundary() {
    for (table, condition) in [
        ("workflow_executions", "NEW.activated_at IS NOT NULL"),
        ("background_tasks", "NEW.status='processing'"),
        ("task_attempts", "true"),
    ] {
        let (f, scope) = fixture().await;
        let before = snapshot(&f).await;
        sqlx::raw_sql(&format!("CREATE FUNCTION lease_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {condition} THEN RAISE EXCEPTION 'injected lease failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER lease_fault AFTER INSERT OR UPDATE ON {table} FOR EACH ROW EXECUTE FUNCTION lease_fault();")).execute(f.persistence().pool()).await.unwrap();
        assert!(
            f.persistence()
                .claim_io(scope, worker(), policy())
                .await
                .is_err()
        );
        assert_eq!(snapshot(&f).await, before);
        sqlx::raw_sql(&format!(
            "DROP TRIGGER lease_fault ON {table}; DROP FUNCTION lease_fault();"
        ))
        .execute(f.persistence().pool())
        .await
        .unwrap();
        assert!(
            f.persistence()
                .claim_io(scope, worker(), policy())
                .await
                .unwrap()
                .is_some()
        );
    }
}
#[tokio::test]
async fn workflow_io_exact_scope_fence_and_run_state_refuse_without_writes() {
    let (f, scope) = fixture().await;
    for invalid in [
        ActivationRequest {
            job: WorkflowJobId(Uuid::new_v4()),
            ..scope
        },
        ActivationRequest {
            company: CompanyId::new(Uuid::new_v4()),
            ..scope
        },
        ActivationRequest {
            run: RunId::new(Uuid::new_v4()),
            ..scope
        },
        ActivationRequest {
            execution: ExecutionId::new(Uuid::new_v4()),
            ..scope
        },
    ] {
        let before = snapshot(&f).await;
        assert!(
            f.persistence()
                .claim_io(invalid, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(snapshot(&f).await, before);
    }
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    for fence in [
        WorkflowFence {
            worker: worker(),
            ..claim.fence
        },
        WorkflowFence {
            generation: WorkflowGeneration(Uuid::new_v4()),
            ..claim.fence
        },
        WorkflowFence {
            attempt: WorkflowAttempt(2),
            ..claim.fence
        },
    ] {
        let before = snapshot(&f).await;
        assert!(
            f.persistence()
                .renew_io(fence, policy())
                .await
                .unwrap()
                .is_none()
        );
        assert!(!f.persistence().validate_io(fence, policy()).await.unwrap());
        assert!(
            !f.persistence()
                .release_io(fence, policy(), LeaseReleaseCause::LocalInterruption)
                .await
                .unwrap()
        );
        assert_eq!(snapshot(&f).await, before);
    }
    sqlx::query("UPDATE workflow_runs SET state='cancelled' WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let before = snapshot(&f).await;
    assert!(
        f.persistence()
            .renew_io(claim.fence, policy())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !f.persistence()
            .validate_io(claim.fence, policy())
            .await
            .unwrap()
    );
    assert!(
        !f.persistence()
            .release_io(claim.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    assert_eq!(snapshot(&f).await, before);
}
#[tokio::test]
async fn workflow_io_external_handler_has_no_transaction_and_returns_fence() {
    use crate::application::workflow::supervise::*;
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let fence = claim.fence;
    let handler = async {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE NOWAIT")
            .bind(scope.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM workflow_executions WHERE id=$1 FOR UPDATE NOWAIT")
            .bind(scope.execution.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM background_tasks WHERE id=$1 FOR UPDATE NOWAIT")
            .bind(scope.job.0)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        Ok(Value::Null)
    };
    let result = supervise_io(
        f.persistence(),
        claim,
        policy(),
        Instant::now() + Duration::from_secs(10),
        handler,
        std::future::pending(),
    )
    .await;
    assert!(matches!(result,SupervisedResult::Ready(result) if result.fence==fence));
    let state = snapshot(&f).await;
    assert_eq!(state["jobs"][0]["status"], "processing");
    assert_eq!(state["executions"][0]["completed_at"], Value::Null);
}

#[path = "lease_adversarial_tests.rs"]
mod adversarial_tests;

#[path = "completion_tests.rs"]
mod completion_tests;

#[path = "wait_tests.rs"]
mod wait_tests;

#[path = "polling_tests.rs"]
mod polling_tests;

#[path = "recovery_tests.rs"]
mod recovery_tests;

#[path = "pending_io_tests.rs"]
mod pending_io_tests;

#[path = "maintenance_tests.rs"]
mod maintenance_tests;

#[path = "control_tests.rs"]
mod control_tests;
