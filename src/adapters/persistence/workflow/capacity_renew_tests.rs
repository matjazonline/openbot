use super::*;

pub(super) async fn blocked(pool: &sqlx::PgPool, blocker: i32) -> i32 {
    tokio::time::timeout(Duration::from_secs(2),async {
        loop {
            let pid:Option<i32>=sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)) LIMIT 1")
                .bind(blocker).fetch_optional(pool).await.unwrap();
            if let Some(pid)=pid {return pid;}
            tokio::task::yield_now().await;
        }
    }).await.unwrap()
}

#[tokio::test]
async fn workflow_capacity_uncommitted_renewal_serializes_claim_across_old_expiry() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    let next = admit(&f, "contender").await;
    let claim = p.claim_io(scope, worker(), lease()).await.unwrap().unwrap();
    let mut gate = p.pool().begin().await.unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(919029)")
        .execute(&mut *gate)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION test_gate_renewal() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(919029); RETURN NEW; END $$; CREATE TRIGGER test_gate_renewal AFTER UPDATE OF lock_expires_at ON background_tasks FOR EACH ROW WHEN (OLD.status='processing' AND NEW.status='processing') EXECUTE FUNCTION test_gate_renewal();")
        .execute(p.pool()).await.unwrap();
    // Set the old committed expiry before installing the gate trigger's condition.
    sqlx::query("ALTER TABLE background_tasks DISABLE TRIGGER test_gate_renewal")
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '300 milliseconds' WHERE id=$1")
        .bind(scope.job.0).execute(p.pool()).await.unwrap();
    sqlx::query("ALTER TABLE background_tasks ENABLE TRIGGER test_gate_renewal")
        .execute(p.pool())
        .await
        .unwrap();
    let renew = tokio::spawn({
        let p = p.clone();
        async move { p.renew_io(claim.fence, lease()).await }
    });
    let renew_pid = blocked(p.pool(), gate_pid).await;
    // Observe that the OLD committed expiry is past while the extension is gated.
    loop {
        let expired: bool = sqlx::query_scalar(
            "SELECT lock_expires_at<=clock_timestamp() FROM background_tasks WHERE id=$1",
        )
        .bind(scope.job.0)
        .fetch_one(p.pool())
        .await
        .unwrap();
        if expired {
            break;
        }
        tokio::task::yield_now().await;
    }
    let contender = tokio::spawn({
        let p = p.clone();
        async move { p.claim_io(next, worker(), lease()).await }
    });
    let claim_pid = blocked(p.pool(), renew_pid).await;
    assert_ne!(claim_pid, renew_pid);
    gate.rollback().await.unwrap();
    assert!(renew.await.unwrap().unwrap().is_some());
    assert!(contender.await.unwrap().unwrap().is_none());
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 1);
}
