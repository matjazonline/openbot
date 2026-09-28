use super::*;

#[tokio::test]
async fn workflow_io_claim_rejects_pure_wait_due_terminal_and_deadline() {
    for name in ["data.map", "wait.timer"] {
        let source = serde_json::from_str(&registry::example(name).unwrap().source).unwrap();
        let (f, scope) = fixture_source(source).await;
        let before = snapshot(&f).await;
        assert!(
            f.persistence()
                .claim_io(scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(snapshot(&f).await, before);
    }
    for change in [
        "UPDATE background_tasks SET run_at=clock_timestamp()+interval '1 hour'",
        "UPDATE workflow_runs SET state='waiting',waiting_reason='timer'",
        "UPDATE workflow_runs SET state='failed'",
        "UPDATE workflow_runs SET deadline=clock_timestamp()-interval '1 second',created_at=clock_timestamp()-interval '10 seconds'",
    ] {
        let (f, scope) = fixture().await;
        sqlx::query(change)
            .execute(f.persistence().pool())
            .await
            .unwrap();
        let before = snapshot(&f).await;
        assert!(
            f.persistence()
                .claim_io(scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(snapshot(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_io_real_renew_reclaim_cancel_contenders_serialize_run_first() {
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    expire(&f, scope).await;
    let barrier = Barrier::new(3);
    let renew = async {
        barrier.wait().await;
        f.persistence()
            .renew_io(claim.fence, policy())
            .await
            .unwrap()
    };
    let reclaim = async {
        barrier.wait().await;
        f.persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
    };
    let cancel = async {
        barrier.wait().await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
            .bind(scope.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE workflow_runs SET state='cancelled' WHERE id=$1")
            .bind(scope.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    };
    let (renewed, reclaimed, ()) = tokio::join!(renew, reclaim, cancel);
    assert!(renewed.is_none());
    assert!(reclaimed.is_none());
    assert!(
        !f.persistence()
            .validate_io(claim.fence, policy())
            .await
            .unwrap()
    );
    let state = snapshot(&f).await;
    assert_eq!(state["runs"][0]["state"], "cancelled");
    assert_eq!(state["attempts"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn workflow_io_contended_claim_cannot_cross_deadline_or_wait_forever() {
    for deadline in [true, false] {
        let (f, scope) = fixture().await;
        if deadline {
            sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+interval '150 milliseconds' WHERE id=$1").bind(scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
        }
        let before = snapshot(&f).await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
            .bind(scope.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        let release = async {
            tokio::time::sleep(Duration::from_millis(if deadline { 250 } else { 1200 })).await;
            tx.commit().await.unwrap();
        };
        let claim = f.persistence().claim_io(scope, worker(), policy());
        let (_, result) = tokio::join!(release, claim);
        if deadline {
            assert!(result.unwrap().is_none());
        } else {
            assert!(result.is_err());
        }
        assert_eq!(snapshot(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_io_pure_boundary_claim_freeze_supervised_result_integration() {
    use crate::application::workflow::{batch::*, supervise::*};
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let io: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["steps"]["start"]["routes"] = json!({"success":"load"});
    source["steps"]["load"] = io["steps"]["start"].clone();
    source["output_schema"] = json!(true);
    let (f, scope) = fixture_source(source).await;
    let batch = f
        .persistence()
        .advance_pure(scope, BatchBudget::new(64, Duration::from_secs(5)).unwrap())
        .await
        .unwrap();
    assert_eq!(batch.disposition, BatchDisposition::Boundary);
    assert_eq!(batch.completed, 1);
    let next = batch.continuation.unwrap();
    let claim = f
        .persistence()
        .claim_io(next, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let result = supervise_io(
        f.persistence(),
        claim,
        policy(),
        Instant::now() + Duration::from_secs(10),
        async { Ok(Value::Null) },
        std::future::pending(),
    )
    .await;
    assert!(matches!(result,SupervisedResult::Ready(result) if result.fence.scope==next));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_executions WHERE completed_at IS NOT NULL",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(count, 1, "I/O success has no counterfeit result commit");
}

#[tokio::test]
async fn workflow_io_existing_mismatched_scope_and_run_states_reject_all_fenced_operations() {
    let (f, scope) = fixture().await;
    let command = f.prepare(f.request("other", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let other_job: Uuid =
        sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
            .bind(command.first_execution_id().as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    for wrong in [
        ActivationRequest {
            run: command.proposed_run_id(),
            ..scope
        },
        ActivationRequest {
            execution: command.first_execution_id(),
            ..scope
        },
        ActivationRequest {
            job: WorkflowJobId(other_job),
            ..scope
        },
    ] {
        let before = snapshot(&f).await;
        assert!(
            f.persistence()
                .claim_io(wrong, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        let fence = WorkflowFence {
            scope: wrong,
            ..claim.fence
        };
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
    for change in [
        "UPDATE workflow_runs SET state='waiting',waiting_reason='timer'",
        "UPDATE workflow_runs SET state='failed',waiting_reason=NULL",
        "UPDATE workflow_runs SET state='running',waiting_reason=NULL,deadline=clock_timestamp()-interval '1 second',created_at=clock_timestamp()-interval '10 seconds'",
    ] {
        sqlx::query(change)
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
}

#[tokio::test]
async fn workflow_io_contended_renewal_never_resurrects_expired_lease() {
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '150 milliseconds' WHERE id=$1").bind(scope.job.0).execute(f.persistence().pool()).await.unwrap();
    let before = snapshot(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let release = async {
        tokio::time::sleep(Duration::from_millis(250)).await;
        tx.commit().await.unwrap();
    };
    let (_, renewed) = tokio::join!(release, f.persistence().renew_io(claim.fence, policy()));
    assert!(renewed.unwrap().is_none());
    assert_eq!(snapshot(&f).await, before);
}
