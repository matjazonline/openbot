//! A frozen preparation keeps the department context it froze (01-departments p03 B3,
//! 01DEP-P03-D11, G3-F1): `department_context_`.
use super::*;

/// The run's channel is owned by a department with `charter`.
async fn owned(f: &HistoryThread, claim: &ClaimedWorkflow, charter: &str) -> Uuid {
    let company = claim.fence.scope.company.as_uuid();
    let channel: Uuid =
        sqlx::query_scalar("SELECT channel_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(company)
            .bind(claim.fence.scope.run.as_uuid())
            .fetch_one(f.s.pool())
            .await
            .unwrap();
    let department = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO departments (id, company_id, name, slug, charter, created_by) \
         VALUES ($1, $2, 'Sales', 'sales', $3, \
           '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Test\"}')",
    )
    .bind(department)
    .bind(company)
    .bind(charter)
    .execute(f.s.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE channels SET department_id=$1 WHERE company_id=$2 AND id=$3")
        .bind(department)
        .bind(company)
        .bind(channel)
        .execute(f.s.pool())
        .await
        .unwrap();
    department
}

/// G3-F1: the preamble is stored and the frozen history's budget counts its length, so a charter
/// edited between the first preparation and a retry -- a longer one here -- must neither fail
/// the retry's budget check nor its document comparison, and the retry sends what was frozen.
#[tokio::test]
async fn department_context_frozen_preparation_replays_the_department_it_froze() {
    let mut f = Box::pin(HistoryThread::new()).await;
    f.s.fixture.persistence =
        f.s.fixture
            .persistence
            .clone()
            .with_app_domain("agents.test");
    let claim = f.next().await;
    let department = owned(&f, &claim, "Win and keep customers.").await;
    let first =
        f.s.p()
            .prepare_agent(&claim, policy())
            .await
            .unwrap()
            .expect_ready();
    let preamble = first.prepared.preamble().to_owned();
    assert!(
        preamble.contains("Department: Sales (sales)."),
        "{preamble}"
    );
    assert!(preamble.contains("Charter:\nWin and keep customers."));
    assert!(preamble.contains("Desks around this department:"));
    assert!(first.prepared.frozen_history().is_some());
    let saved = frozen(&f, &claim).await;
    assert_eq!(saved["department"]["charter"], "Win and keep customers.");

    sqlx::query("UPDATE departments SET charter=repeat('Grow every account. ', 40) WHERE id=$1")
        .bind(department)
        .execute(f.s.pool())
        .await
        .unwrap();
    let retry =
        f.s.p()
            .prepare_agent(&claim, policy())
            .await
            .expect("the retry restores what it froze")
            .expect_ready();
    assert_eq!(retry.prepared.preamble(), preamble);
    assert_eq!(
        retry.prepared.spec(claim.fence).unwrap().prepared,
        first.prepared.spec(claim.fence).unwrap().prepared
    );
    assert_eq!(frozen(&f, &claim).await, saved);
}

/// 01-departments 04B.4: the run list and the board narrow to the runs on a department's desks;
/// the department is the desk's now, so another department, or none owning the desk, lists none.
#[tokio::test]
async fn department_views_runs_narrow_to_the_departments_desks() {
    use crate::application::workflow::board::RunBoardScope;
    use crate::application::workflow::reads::{PageRequest, RunFilter, WorkflowReads};
    use crate::entities::department::DepartmentId;
    let f = Box::pin(HistoryThread::new()).await;
    let claim = f.next().await;
    let company = claim.fence.scope.company;
    let run = claim.fence.scope.run.as_uuid();
    let sales = DepartmentId::new(owned(&f, &claim, "Sell.").await);
    let other = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO departments (id, company_id, name, slug, created_by) \
         VALUES ($1, $2, 'Other', 'other', \
           '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Test\"}')",
    )
    .bind(other)
    .bind(company.as_uuid())
    .execute(f.s.pool())
    .await
    .unwrap();
    let listed = |department| {
        let p = f.s.p();
        async move {
            p.list_runs(
                company,
                &RunFilter {
                    department,
                    ..RunFilter::default()
                },
                PageRequest::new(None, Some(50)).unwrap(),
            )
            .await
            .unwrap()
            .items
            .iter()
            .map(|summary| summary.run.as_uuid())
            .collect::<Vec<_>>()
        }
    };
    assert!(listed(None).await.contains(&run));
    assert_eq!(listed(Some(sales)).await, [run]);
    assert!(listed(Some(DepartmentId::new(other))).await.is_empty());

    let cutoff = chrono::Utc::now() - chrono::Duration::days(1);
    let counted = |department| {
        let p = f.s.p();
        async move {
            p.run_board_counts(
                company,
                &RunBoardScope {
                    department,
                    ..RunBoardScope::default()
                },
                cutoff,
            )
            .await
            .unwrap()
            .iter()
            .map(|count| count.runs)
            .sum::<u64>()
        }
    };
    assert_eq!(counted(Some(sales)).await, counted(None).await);
    assert!(counted(Some(sales)).await >= 1);
    assert_eq!(counted(Some(DepartmentId::new(other))).await, 0);
}

/// 01-departments 04C.2 (B2-F4): usage by department follows the call's run to its desk and the
/// desk's department now: a call on a department-owned desk is under that department's name.
#[tokio::test]
async fn department_views_usage_names_the_department_of_the_runs_desk() {
    use crate::application::services::model_usage::{
        ModelUsageStore, UsageDepartment, UsageGrouping, UsageQuery, UsageScope,
    };
    let f = Box::pin(HistoryThread::new()).await;
    let claim = f.next().await;
    let company = claim.fence.scope.company.as_uuid();
    let sales = owned(&f, &claim, "Sell.").await;
    // An agent job's call names its agent (`model_usage_agent_kind`).
    let agent =
        f.s.p()
            .prepare_agent(&claim, policy())
            .await
            .unwrap()
            .expect_ready()
            .prepared
            .agent_id();
    let row: serde_json::Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('company_id',run.company_id,'request_id',gen_random_uuid(), \
           'task_id',task.id,'agent_id',$3::uuid,'workflow_id',run.workflow_id, \
           'version_id',run.version_id,'run_id',run.id,'execution_id',task.workflow_execution_id, \
           'root_run_id',budget.root_run_id,'provider','openai','model','fixture', \
           'reserved_input',100,'reserved_output',50,'requested_at',statement_timestamp()) \
         FROM workflow_runs AS run JOIN background_tasks AS task ON task.company_id=run.company_id \
           AND task.workflow_execution_id=$2 \
         JOIN workflow_run_budgets AS budget ON budget.company_id=run.company_id \
           AND budget.run_id=run.id WHERE run.id=$1 LIMIT 1",
    )
    .bind(claim.fence.scope.run.as_uuid())
    .bind(claim.fence.scope.execution.as_uuid())
    .bind(agent)
    .fetch_one(f.s.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO model_usage_calls SELECT (jsonb_populate_record(NULL::model_usage_calls,$1)).*",
    )
    .bind(row)
    .execute(f.s.pool())
    .await
    .unwrap();
    let owner: Uuid = sqlx::query_scalar("SELECT user_id FROM companies WHERE id = $1")
        .bind(company)
        .fetch_one(f.s.pool())
        .await
        .unwrap();
    let end = chrono::Utc::now() + chrono::Duration::minutes(1);
    let report =
        f.s.p()
            .usage_report(UsageQuery {
                scope: UsageScope {
                    company,
                    actor: owner,
                },
                start: end - chrono::Duration::days(1),
                end,
                grouping: UsageGrouping::Department,
                agent: None,
                workflow: None,
            })
            .await
            .unwrap();
    let named: Vec<(Option<UsageDepartment>, u64)> = report
        .groups
        .iter()
        .map(|group| (group.department.clone(), group.calls))
        .collect();
    assert_eq!(
        named,
        [(
            Some(UsageDepartment {
                id: Some(sales),
                name: "Sales".into(),
            }),
            1
        )]
    );
}
