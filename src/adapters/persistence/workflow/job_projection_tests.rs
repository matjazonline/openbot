use super::*;
use crate::{
    entities::{dashboard::DashboardWindow, task::TaskStatus},
    services::dashboard_snapshot::DashboardPersistence,
};

/// p08 D4.5: the dashboard's job figures are the workflow queue's -- every job, with a channel or
/// without one; token spend is the usage ledger's; and another company's form reads none of it.
#[tokio::test]
async fn workflow_job_projection_dashboard_counts_every_workflow_job() {
    let (f, workflow) = scoped_fixture().await;
    let p = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let mut jobs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM background_tasks WHERE company_id = $1 AND queue_kind = 'workflow'",
    )
    .bind(company)
    .fetch_all(p.pool())
    .await
    .unwrap();
    jobs.sort();
    assert_eq!(
        jobs.len(),
        3,
        "the fixture's jobs: company-wide, channel, and thread"
    );
    sqlx::query("UPDATE background_tasks SET created_at = CURRENT_TIMESTAMP - interval '10 minutes' WHERE company_id = $1")
        .bind(company).execute(p.pool()).await.unwrap();
    // A second attempt, 90 seconds long: one attempt and one retry in the window.
    sqlx::query("INSERT INTO task_attempts (id,task_id,attempt_number,execution_generation,status,worker_id,machine_id,started_at,finished_at) VALUES (gen_random_uuid(),$1,2,gen_random_uuid(),'completed',gen_random_uuid(),'projection',CURRENT_TIMESTAMP - make_interval(secs => 90),CURRENT_TIMESTAMP)")
        .bind(workflow).execute(p.pool()).await.unwrap();
    sqlx::query("INSERT INTO model_usage_calls (company_id, request_id, task_id, agent_id, workflow_id, version_id, run_id, execution_id, root_run_id, provider, model, reserved_input, reserved_output, input_tokens, output_tokens, usage_source, settled_at) \
        SELECT $1, gen_random_uuid(), job.id, agent.id, run.workflow_id, run.version_id, run.id, execution.id, run.id, 'openai', 'gpt-4o', 64, 64, 40, 2, 'reported', clock_timestamp() \
          FROM background_tasks AS job \
          JOIN workflow_executions AS execution ON execution.company_id = job.company_id AND execution.id = job.workflow_execution_id \
          JOIN workflow_runs AS run ON run.company_id = execution.company_id AND run.id = execution.run_id \
          JOIN agents AS agent ON agent.company_id = job.company_id AND agent.slug = 'worker' \
         WHERE job.company_id = $1 AND job.id = $2")
        .bind(company).bind(workflow).execute(p.pool()).await.unwrap();
    for scope in [None, Some(company)] {
        let snapshot = p
            .dashboard_snapshot(scope, DashboardWindow::last_hour())
            .await
            .unwrap();
        assert_eq!(snapshot.tasks.count_of(TaskStatus::Pending), 3);
        assert_eq!(snapshot.tasks.due_now, 3);
        assert_eq!(snapshot.queue_depth.last().unwrap().open, 3);
        let mut outstanding: Vec<Uuid> = snapshot.outstanding.iter().map(|job| job.id).collect();
        outstanding.sort();
        assert_eq!(outstanding, jobs);
        assert!(
            snapshot
                .outstanding
                .iter()
                .any(|job| job.channel_id.is_none() && job.channel_name.is_none()),
            "a company-wide job is listed without a channel"
        );
        assert_eq!(snapshot.attempts.attempts, 1);
        assert_eq!(snapshot.attempts.retries, 1);
        assert_eq!(snapshot.attempts.p95_ms, Some(90_000));
        assert_eq!(
            (
                snapshot.attempts.prompt_tokens,
                snapshot.attempts.completion_tokens
            ),
            (40, 2),
            "tokens are the usage ledger's"
        );
        assert_eq!(
            snapshot
                .retry_rate
                .iter()
                .map(|bucket| (bucket.attempts, bucket.retries))
                .fold((0, 0), |sum, bucket| (sum.0 + bucket.0, sum.1 + bucket.1)),
            (1, 1)
        );
        assert_eq!(
            snapshot
                .latency
                .iter()
                .filter_map(|bucket| bucket.p95_ms)
                .collect::<Vec<_>>(),
            vec![90_000]
        );
    }
    let other = p
        .dashboard_snapshot(Some(Uuid::new_v4()), DashboardWindow::last_hour())
        .await
        .unwrap();
    assert_eq!(other.tasks.total(), 0);
    assert!(other.outstanding.is_empty());
    assert_eq!(
        other.attempts,
        crate::entities::dashboard::AttemptStats::default()
    );
    assert_eq!(other.queue_depth.last().unwrap().open, 0);

    sqlx::query("UPDATE background_tasks SET status = 'completed' WHERE company_id = $1")
        .bind(company)
        .execute(p.pool())
        .await
        .unwrap();
    for scope in [None, Some(company)] {
        let snapshot = p
            .dashboard_snapshot(scope, DashboardWindow::last_hour())
            .await
            .unwrap();
        assert_eq!(snapshot.throughput_total(), 3);
        assert_eq!(snapshot.tasks.count_of(TaskStatus::Completed), 3);
    }
}

/// Plan new-board-view 04: each outstanding job names the run it is a step of, through its
/// execution, so the Dashboard can link a job outside a conversation to its run.
#[tokio::test]
async fn workflow_job_projection_dashboard_outstanding_jobs_name_their_run() {
    let (f, _) = scoped_fixture().await;
    let p = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let expected: std::collections::HashMap<Uuid, Uuid> = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT job.id, execution.run_id FROM background_tasks AS job \
           JOIN workflow_executions AS execution \
             ON execution.company_id = job.company_id AND execution.id = job.workflow_execution_id \
          WHERE job.company_id = $1 AND job.queue_kind = 'workflow'",
    )
    .bind(company)
    .fetch_all(p.pool())
    .await
    .unwrap()
    .into_iter()
    .collect();
    assert_eq!(expected.len(), 3, "the fixture's three jobs");
    for scope in [None, Some(company)] {
        let snapshot = p
            .dashboard_snapshot(scope, DashboardWindow::last_hour())
            .await
            .unwrap();
        let named: std::collections::HashMap<Uuid, Uuid> = snapshot
            .outstanding
            .iter()
            .map(|job| {
                (
                    job.id,
                    job.run_id.expect("a job of an execution names its run"),
                )
            })
            .collect();
        assert_eq!(named, expected, "{scope:?}");
    }
}
