use super::*;
use crate::application::workflow::{capacity::*, completion::*};

async fn admit(f: &AdmissionFixture, key: &str) -> ActivationRequest {
    let command = f.prepare(f.request(key, f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(
            sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
                .bind(command.first_execution_id().as_uuid())
                .fetch_one(f.persistence().pool())
                .await
                .unwrap(),
        ),
    }
}

async fn tenant(f: &AdmissionFixture) -> AdmissionFixture {
    let mut fixture = Fixture::in_database(
        f.binding.fixture._db.clone(),
        &Uuid::new_v4().simple().to_string(),
    )
    .await;
    fixture.target.workflow = WorkflowId::new(Uuid::new_v4());
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["workflow_id"] = json!(fixture.target.workflow.as_uuid());
    AdmissionFixture::with_binding(BindingFixture::from_fixture(fixture, source).await).await
}

fn lease() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(60)).unwrap()
}

async fn finish(p: &PostgresPersistence, claim: &ClaimedWorkflow) {
    p.complete_io(FencedWorkflowResult {
        fence: claim.fence,
        output: json!({"items":[],"token_count":0}),
    })
    .await
    .unwrap()
    .unwrap();
}

async fn budget(p: &PostgresPersistence, scope: ActivationRequest) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('usage',(SELECT jsonb_agg(to_jsonb(usage)) FROM workflow_root_budget_usage AS usage WHERE root_run_id=$1),'receipts',(SELECT jsonb_agg(to_jsonb(receipt) ORDER BY reservation_key) FROM workflow_budget_receipts AS receipt WHERE run_id=$1))")
        .bind(scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_capacity_sql_bounds_and_implicit_default_are_enforced() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    for sql in [
        "INSERT INTO workflow_capacity_policy VALUES (true,1,1)",
        "INSERT INTO workflow_capacity_policy VALUES (true,1025,1)",
        "INSERT INTO workflow_capacity_policy VALUES (true,2,0)",
        "INSERT INTO workflow_capacity_policy VALUES (true,2,2)",
        "INSERT INTO workflow_capacity_policy VALUES (false,16,2)",
    ] {
        let error = sqlx::query(sql).execute(p.pool()).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    p.claim_io(scope, worker(), lease()).await.unwrap().unwrap();
    let installed: (i32, i32) = sqlx::query_as(
        "SELECT global_limit,company_limit FROM workflow_capacity_policy WHERE singleton",
    )
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(installed, (16, 2));
    p.configure_capacity(WorkflowCapacityPolicy::default())
        .await
        .unwrap();
    assert!(
        p.configure_capacity(WorkflowCapacityPolicy::new(4, 2).unwrap())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn workflow_capacity_policy_is_global_immutable_and_bounded() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    for limits in [(0, 0), (1, 1), (2, 0), (2, 2), (1025, 1)] {
        assert!(WorkflowCapacityPolicy::new(limits.0, limits.1).is_err());
    }
    let wanted = WorkflowCapacityPolicy::new(3, 1).unwrap();
    let other = WorkflowCapacityPolicy::new(4, 2).unwrap();
    let barrier = Barrier::new(2);
    let configure = |limits| {
        let barrier = &barrier;
        async move {
            barrier.wait().await;
            PostgresPersistence::new(p.pool().clone())
                .configure_capacity(limits)
                .await
        }
    };
    let (a, b) = tokio::join!(configure(wanted), configure(other));
    assert_ne!(a.is_ok(), b.is_ok());
    let installed = if a.is_ok() { wanted } else { other };
    p.configure_capacity(installed).await.unwrap();
    for sql in [
        "UPDATE workflow_capacity_policy SET company_limit=1",
        "DELETE FROM workflow_capacity_policy",
    ] {
        let error = sqlx::query(sql).execute(p.pool()).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    let claim = p.claim_io(scope, worker(), lease()).await.unwrap().unwrap();
    assert!(
        p.configure_capacity(WorkflowCapacityPolicy::new(5, 1).unwrap())
            .await
            .is_err()
    );
    assert!(p.validate_io(claim.fence, lease()).await.unwrap());
}

#[tokio::test]
async fn workflow_capacity_competing_distinct_runs_enforce_both_maxima_and_release() {
    let (f, first) = fixture().await;
    let second = tenant(&f).await;
    let third = tenant(&f).await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(4, 2).unwrap())
        .await
        .unwrap();
    let mut scopes = vec![first];
    for index in 0..3 {
        if index > 0 {
            scopes.push(admit(&f, &format!("a{index}")).await);
        }
        scopes.push(admit(&second, &format!("b{index}")).await);
        scopes.push(admit(&third, &format!("c{index}")).await);
    }
    let barrier = Barrier::new(scopes.len());
    let claims = futures::future::join_all(scopes.iter().map(|scope| {
        let barrier = &barrier;
        async move {
            barrier.wait().await;
            PostgresPersistence::new(p.pool().clone())
                .claim_io(*scope, worker(), lease())
                .await
                .unwrap()
        }
    }))
    .await;
    let owned: Vec<_> = claims.iter().flatten().collect();
    assert_eq!(owned.len(), 4);
    let counts:Vec<(Uuid,i64)>=sqlx::query_as("SELECT company_id,count(*) FROM background_tasks WHERE status='processing' GROUP BY company_id")
        .fetch_all(p.pool()).await.unwrap();
    assert_eq!(counts.iter().map(|(_, count)| count).sum::<i64>(), 4);
    assert!(counts.iter().all(|(_, count)| *count <= 2));
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 4);
    let oldest_job: Uuid =
        sqlx::query_scalar("SELECT job_id FROM workflow_dispatch_demand ORDER BY ticket LIMIT 1")
            .fetch_one(p.pool())
            .await
            .unwrap();
    let denied = scopes
        .iter()
        .find(|scope| scope.job.0 == oldest_job)
        .unwrap();
    let before = snapshot(&f).await;
    let accounting = budget(p, *denied).await;
    assert_eq!(accounting["usage"][0]["activations"], 1);
    assert_eq!(accounting["receipts"].as_array().unwrap().len(), 1);
    assert!(
        p.claim_io(*denied, worker(), lease())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(budget(p, *denied).await, accounting);
    for claim in owned {
        finish(p, claim).await;
    }
    let fresh = PostgresPersistence::new(p.pool().clone());
    assert!(
        fresh
            .claim_io(*denied, worker(), lease())
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(budget(p, *denied).await, accounting);
}

#[tokio::test]
async fn workflow_capacity_expiry_restart_releases_without_refunding_attempts() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    let next = admit(&f, "next").await;
    let old = p.claim_io(scope, worker(), lease()).await.unwrap().unwrap();
    assert!(p.claim_io(next, worker(), lease()).await.unwrap().is_none());
    expire(&f, scope).await;
    let fresh = PostgresPersistence::new(p.pool().clone());
    let replacement = fresh
        .claim_io(next, worker(), lease())
        .await
        .unwrap()
        .unwrap();
    assert!(fresh.renew_io(old.fence, lease()).await.unwrap().is_none());
    assert!(
        fresh
            .complete_io(FencedWorkflowResult {
                fence: old.fence,
                output: json!({"items":[],"token_count":0})
            })
            .await
            .unwrap()
            .is_none()
    );
    assert!(fresh.retire_expired_io(scope, lease()).await.unwrap());
    assert!(!fresh.retire_expired_io(scope, lease()).await.unwrap());
    let debit: i32 = sqlx::query_scalar("SELECT retry_count FROM background_tasks WHERE id=$1")
        .bind(scope.job.0)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(debit, 1);
    finish(&fresh, &replacement).await;
    due(&f, scope).await;
    let retry = fresh
        .claim_io(scope, worker(), lease())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retry.fence.attempt.0, 2);
}

#[path = "capacity_renew_tests.rs"]
mod renew_tests;

#[tokio::test]
async fn workflow_capacity_count_prunes_retained_terminal_history() {
    let (f, first) = fixture().await;
    let p = f.persistence();
    for index in 0..256 {
        let scope = if index == 0 {
            first
        } else {
            admit(&f, &format!("history-{index}")).await
        };
        let claim = p.claim_io(scope, worker(), lease()).await.unwrap().unwrap();
        finish(p, &claim).await;
    }
    let current = admit(&f, "current").await;
    p.claim_io(current, worker(), lease())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("ANALYZE background_tasks,workflow_executions,workflow_runs")
        .execute(p.pool())
        .await
        .unwrap();
    let query = crate::adapters::persistence::workflow::capacity::LIVE_COMPANIES_SQL;
    let owners: Vec<Uuid> = sqlx::query_scalar(query)
        .bind(16_i64)
        .fetch_all(p.pool())
        .await
        .unwrap();
    assert_eq!(owners, vec![current.company.as_uuid()]);
    let plan: Vec<String> = sqlx::query_scalar(&format!("EXPLAIN (ANALYZE, BUFFERS) {query}"))
        .bind(16_i64)
        .fetch_all(p.pool())
        .await
        .unwrap();
    println!(
        "Capacity retained-history plan (256 terminal, 1 live):\n{}",
        plan.join("\n")
    );
    let waiting = admit(&f, "waiting").await;
    let query = crate::adapters::persistence::workflow::fair_polling::company_query();
    let selected: Uuid = sqlx::query_scalar(&query)
        .bind(None::<Uuid>)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(selected, waiting.company.as_uuid());
    let plan: Vec<String> = sqlx::query_scalar(&format!("EXPLAIN (ANALYZE, BUFFERS) {query}"))
        .bind(None::<Uuid>)
        .fetch_all(p.pool())
        .await
        .unwrap();
    println!(
        "Fair company discovery retained-history plan (256 terminal, 1 live, 1 pending):\n{}",
        plan.join("\n")
    );
}

#[path = "fairness_tests.rs"]
mod fairness_tests;
