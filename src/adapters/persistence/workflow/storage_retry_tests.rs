use super::*;
use std::time::Duration as WallDuration;

async fn wait_owner(f: &AdmissionFixture, excluded: Option<i32>, event: Option<&str>) -> i32 {
    tokio::time::timeout(WallDuration::from_secs(4), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database()
                AND wait_event_type='Lock' AND pid IS DISTINCT FROM $1 AND ($2::text IS NULL OR wait_event=$2) LIMIT 1")
                .bind(excluded).bind(event).fetch_optional(f.persistence().pool()).await.unwrap();
            if let Some(pid) = pid { return pid; }
            tokio::time::sleep(WallDuration::from_millis(10)).await;
        }
    }).await.expect("production competitor reached observed ownership barrier")
}
async fn install_barrier(f: &AdmissionFixture, retry_first: bool) {
    let sql = if retry_first {
        "CREATE FUNCTION retention_retry_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
         IF OLD.state='failed' AND NEW.state='running' THEN PERFORM pg_advisory_xact_lock(602208); END IF; RETURN NEW; END $$;
         CREATE TRIGGER retention_retry_barrier BEFORE UPDATE ON workflow_runs FOR EACH ROW EXECUTE FUNCTION retention_retry_barrier()"
    } else {
        "CREATE FUNCTION retention_retry_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
         PERFORM pg_advisory_xact_lock(602208); RETURN OLD; END $$;
         CREATE TRIGGER retention_retry_barrier BEFORE DELETE ON background_tasks FOR EACH ROW EXECUTE FUNCTION retention_retry_barrier()"
    };
    sqlx::raw_sql(sql)
        .execute(f.persistence().pool())
        .await
        .unwrap();
}
async fn race(retry_first: bool) {
    let (f, scope) = Box::pin(lease_tests::retryable_fixture()).await;
    usage(&f, scope, Some("reported")).await;
    let root = age(&f, scope).await;
    // Only date setup uses the common fault fixture; every production guard is active in the race.
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let retry = RetryCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: f.binding.target.actor,
        command_key: IdempotencyKey::parse("retention-race-retry").unwrap(),
        expected_revision: f
            .persistence()
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    install_barrier(&f, retry_first).await;
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(602208)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let p = f.persistence().clone();
    if retry_first {
        let writer = tokio::spawn(async move { p.retry(retry).await });
        wait_owner(&f, None, Some("advisory")).await;
        assert_eq!(f.persistence().retire_tree(root).await.unwrap(), None);
        blocker.commit().await.unwrap();
        assert!(matches!(
            writer.await.unwrap().unwrap(),
            RetryResult::Applied { .. }
        ));
        let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
        assert_eq!(state, "running");
        assert_eq!(count(&f, "model_usage_calls").await, 1);
        assert_eq!(count(&f, "model_usage_history").await, 0);
    } else {
        let collector = tokio::spawn(async move { p.retire_tree(root).await });
        let owner = wait_owner(&f, None, Some("advisory")).await;
        let p = f.persistence().clone();
        let writer = tokio::spawn(async move { p.retry(retry).await });
        wait_owner(&f, Some(owner), None).await;
        blocker.commit().await.unwrap();
        assert_eq!(collector.await.unwrap().unwrap().unwrap().calls, 1);
        assert!(matches!(
            writer.await.unwrap().unwrap(),
            RetryResult::NotFound
        ));
        assert_eq!(count(&f, "workflow_runs").await, 0);
        assert_eq!(count(&f, "background_tasks").await, 0);
        assert_eq!(count(&f, "model_usage_calls").await, 0);
        assert_eq!(count(&f, "model_usage_history").await, 1);
    }
    let dangling: i64 = sqlx::query_scalar("SELECT count(*) FROM background_tasks AS job LEFT JOIN workflow_executions AS step
        ON step.company_id=job.company_id AND step.id=job.workflow_execution_id WHERE job.workflow_execution_id IS NOT NULL AND step.id IS NULL")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(dangling, 0);
}
#[tokio::test]
async fn run_retention_retry_first_revival_owns_and_preserves_live_work() {
    race(true).await;
}
#[tokio::test]
async fn run_retention_first_fences_waiting_production_retry_without_partial_archive() {
    race(false).await;
}
