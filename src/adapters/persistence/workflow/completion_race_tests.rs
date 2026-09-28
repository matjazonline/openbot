use super::*;
use crate::adapters::persistence::workflow::completion::complete_on;

async fn cancel(f: &AdmissionFixture, scope: ActivationRequest, barrier: &Barrier) {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    barrier.wait().await;
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE workflow_runs SET state='cancelled' WHERE id=$1 AND state IN ('queued','running')",
    )
    .bind(scope.run.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn workflow_completion_cancel_competes_without_stale_progression() {
    for _ in 0..3 {
        let (f, claim) = claimed(chain()).await;
        let barrier = Barrier::new(2);
        let commit = async {
            barrier.wait().await;
            f.persistence()
                .complete_io(result(claim.fence))
                .await
                .unwrap()
        };
        let (saved, ()) = tokio::join!(commit, cancel(&f, claim.fence.scope, &barrier));
        let state = state(&f).await;
        assert_eq!(state["runs"][0]["state"], "cancelled");
        assert_eq!(
            state["executions"].as_array().unwrap().len(),
            if saved.is_some() { 2 } else { 1 }
        );
        assert_eq!(
            state["attempts"][0]["status"],
            if saved.is_some() {
                "completed"
            } else {
                "processing"
            }
        );
        if let Some(saved) = saved {
            assert!(
                f.persistence()
                    .claim_io(saved.successor.unwrap(), worker(), policy())
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[tokio::test]
async fn workflow_completion_reclaim_competes_without_stale_progression() {
    let (f, claim) = claimed(chain()).await;
    expire(&f, claim.fence.scope).await;
    let barrier = Barrier::new(2);
    let commit = async {
        barrier.wait().await;
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .unwrap()
    };
    let reclaim = async {
        barrier.wait().await;
        f.persistence()
            .claim_io(claim.fence.scope, worker(), policy())
            .await
            .unwrap()
    };
    let (saved, reclaimed) = tokio::join!(commit, reclaim);
    assert!(saved.is_none());
    assert!(reclaimed.is_none());
    due(&f, claim.fence.scope).await;
    let replacement = f
        .persistence()
        .claim_io(claim.fence.scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let barrier = Barrier::new(2);
    let old = async {
        barrier.wait().await;
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .unwrap()
    };
    let new = async {
        barrier.wait().await;
        f.persistence()
            .complete_io(result(replacement.fence))
            .await
            .unwrap()
    };
    let (old, new) = tokio::join!(old, new);
    assert!(old.is_none());
    assert!(new.is_some());
    assert_eq!(state(&f).await["executions"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn workflow_completion_inactive_run_and_lock_contention_refuse() {
    for (run_state, extra) in [
        ("waiting", ",waiting_reason='timer'"),
        ("cancelled", ""),
        ("failed", ""),
        ("succeeded", ""),
    ] {
        let (f, claim) = claimed(chain()).await;
        sqlx::raw_sql(&format!(
            "UPDATE workflow_runs SET state='{run_state}'{extra}"
        ))
        .execute(f.persistence().pool())
        .await
        .unwrap();
        let before = state(&f).await;
        assert!(
            f.persistence()
                .complete_io(result(claim.fence))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(state(&f).await, before);
    }
    let (f, claim) = claimed(chain()).await;
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '150 milliseconds' WHERE id=$1").bind(claim.fence.scope.job.0).execute(f.persistence().pool()).await.unwrap();
    let before = state(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(claim.fence.scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let unlock = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        tx.commit().await.unwrap();
    };
    let (_, saved) = tokio::join!(unlock, f.persistence().complete_io(result(claim.fence)));
    assert!(saved.unwrap().is_none());
    assert_eq!(state(&f).await, before);
}

#[tokio::test]
async fn workflow_completion_deferred_expiry_and_deadline_commit_guard() {
    for boundary in ["lease", "deadline"] {
        let (f, claim) = claimed(chain()).await;
        let statement = if boundary == "lease" {
            "UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '400 milliseconds' WHERE id=$1"
        } else {
            "UPDATE workflow_runs SET deadline=clock_timestamp()+interval '400 milliseconds' WHERE id=$1"
        };
        let id = if boundary == "lease" {
            claim.fence.scope.job.0
        } else {
            claim.fence.scope.run.as_uuid()
        };
        sqlx::query(statement)
            .bind(id)
            .execute(f.persistence().pool())
            .await
            .unwrap();
        let before = state(&f).await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        complete_on(&mut tx, result(claim.fence))
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(450)).await;
        assert!(tx.commit().await.is_err(), "{boundary}");
        assert_eq!(state(&f).await, before, "{boundary}");
    }
}

#[tokio::test]
async fn workflow_completion_final_write_expiry_rolls_back_attempt_and_result() {
    let (f, claim) = claimed(chain()).await;
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '150 milliseconds' WHERE id=$1").bind(claim.fence.scope.job.0).execute(f.persistence().pool()).await.unwrap();
    let before = state(&f).await;
    sqlx::raw_sql("CREATE FUNCTION completion_delay() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.status='completed' THEN PERFORM pg_sleep(0.2); END IF; RETURN NEW; END $$; CREATE TRIGGER completion_delay AFTER UPDATE ON task_attempts FOR EACH ROW EXECUTE FUNCTION completion_delay();").execute(f.persistence().pool()).await.unwrap();
    assert!(
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .is_err()
    );
    assert_eq!(state(&f).await, before);
}
