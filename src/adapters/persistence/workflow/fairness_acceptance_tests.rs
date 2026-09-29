use super::*;

#[tokio::test]
async fn workflow_fairness_faults_roll_back_cursor_demand_and_activation() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    sqlx::raw_sql("CREATE FUNCTION fail_cursor() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'cursor fault'; END $$; CREATE TRIGGER fail_cursor AFTER INSERT ON workflow_poll_companies FOR EACH ROW EXECUTE FUNCTION fail_cursor();")
        .execute(p.pool()).await.unwrap();
    assert!(p.poll_fair(worker(), 1).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_poll_workers")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::raw_sql("DROP TRIGGER fail_cursor ON workflow_poll_companies; DROP FUNCTION fail_cursor(); CREATE FUNCTION fail_demand() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'demand fault'; END $$; CREATE TRIGGER fail_demand AFTER INSERT ON workflow_dispatch_demand FOR EACH ROW EXECUTE FUNCTION fail_demand();")
        .execute(p.pool()).await.unwrap();
    let before = snapshot(&f).await;
    let accounting = budget(p, scope).await;
    assert!(p.claim_io(scope, worker(), lease()).await.is_err());
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(budget(p, scope).await, accounting);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_dispatch_demand")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn workflow_fairness_schema_enforces_cursor_owner_and_single_offer() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    let error =
        sqlx::query("INSERT INTO workflow_poll_companies (worker_id,company_id) VALUES ($1,$2)")
            .bind(worker().0)
            .bind(scope.company.as_uuid())
            .execute(p.pool())
            .await
            .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    for i in 0..2 {
        let result=sqlx::query("INSERT INTO workflow_dispatch_demand (company_id,worker_id,run_id,execution_id,job_id,opportunity_until) VALUES ($1,$2,$3,$4,$5,clock_timestamp()+interval '45 seconds')")
            .bind(Uuid::new_v4()).bind(worker().0).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).execute(p.pool()).await;
        if i == 0 {
            result.unwrap();
        } else {
            assert_eq!(
                result
                    .unwrap_err()
                    .as_database_error()
                    .unwrap()
                    .code()
                    .as_deref(),
                Some("23505")
            );
        }
    }
    let w = worker();
    p.poll_fair(w, 1).await.unwrap();
    sqlx::query("DELETE FROM workflow_poll_workers WHERE worker_id=$1")
        .bind(w.0)
        .execute(p.pool())
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_poll_companies WHERE worker_id=$1")
            .bind(w.0)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn workflow_fairness_discovery_never_waits_for_run_or_capacity_locks() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    let mut gate = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(scope.run.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT singleton FROM workflow_capacity_policy FOR UPDATE")
        .execute(&mut *gate)
        .await
        .unwrap();
    let page = p.poll_fair(worker(), 128).await.unwrap();
    assert_eq!(page.candidates[0].scope, scope);
    gate.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_fairness_claim_waiting_for_run_does_not_hold_capacity() {
    let (f, scope) = fixture().await;
    let other = tenant(&f).await;
    let second = admit(&other, "other").await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    let mut gate = p.pool().begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(scope.run.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let claimant = tokio::spawn({
        let p = p.clone();
        async move { p.claim_io(scope, worker(), lease()).await }
    });
    super::super::renew_tests::blocked(p.pool(), pid).await;
    let independent = p
        .claim_io(second, worker(), lease())
        .await
        .unwrap()
        .unwrap();
    gate.rollback().await.unwrap();
    assert!(claimant.await.unwrap().unwrap().is_some());
    assert_eq!(independent.fence.scope, second);
}

#[tokio::test]
async fn workflow_fairness_cleanup_skips_explicitly_locked_stale_parents() {
    let (f, _) = fixture().await;
    let p = f.persistence();
    let owners = [worker(), worker()];
    for owner in owners {
        p.poll_fair(owner, 1).await.unwrap();
    }
    sqlx::query("UPDATE workflow_poll_workers SET touched_at=clock_timestamp()-interval '2 days'")
        .execute(p.pool())
        .await
        .unwrap();
    let mut gates = vec![];
    for owner in owners {
        let mut gate = p.pool().begin().await.unwrap();
        sqlx::query("SELECT worker_id FROM workflow_poll_workers WHERE worker_id=$1 FOR UPDATE")
            .bind(owner.0)
            .execute(&mut *gate)
            .await
            .unwrap();
        gates.push(gate);
    }
    // Both stale parents remain locked throughout cleanup by a third worker.
    p.poll_fair(worker(), 1).await.unwrap();
    let retained: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_poll_workers WHERE worker_id=ANY($1)")
            .bind(owners.map(|owner| owner.0).as_slice())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(retained, 2);
    for gate in gates {
        gate.rollback().await.unwrap();
    }
    let (a, b) = tokio::join!(p.poll_fair(owners[0], 1), p.poll_fair(owners[1], 1));
    assert!(a.is_ok() && b.is_ok());
}

#[tokio::test]
async fn workflow_fairness_deleted_company_hints_are_inert_and_cursors_retire() {
    let (f, scope) = fixture().await;
    let other = tenant(&f).await;
    let second = admit(&other, "survivor").await;
    let p = f.persistence();
    let w = worker();
    p.poll_fair(w, 128).await.unwrap();
    p.poll_fair(w, 128).await.unwrap();
    sqlx::query("INSERT INTO workflow_dispatch_demand (company_id,worker_id,run_id,execution_id,job_id,opportunity_until) VALUES ($1,$2,$3,$4,$5,clock_timestamp()+interval '45 seconds')")
        .bind(scope.company.as_uuid()).bind(worker().0).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).execute(p.pool()).await.unwrap();
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(scope.company.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    let page = p.poll_fair(w, 128).await.unwrap();
    assert!(
        page.candidates
            .iter()
            .all(|candidate| candidate.scope.company == second.company)
    );
    assert!(
        p.claim_io(second, worker(), lease())
            .await
            .unwrap()
            .is_some()
    );
    let retained:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM workflow_poll_companies WHERE company_id=$1)+(SELECT count(*) FROM workflow_dispatch_demand WHERE company_id=$1)")
        .bind(scope.company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(retained, 0);
}
