use super::*;
use crate::application::workflow::storage::{RetentionRoot, RetiredTree};

pub(super) async fn retire_on(
    db: &mut PgConnection,
    root: RetentionRoot,
) -> AppResult<Option<RetiredTree>> {
    let company = root.company.as_uuid();
    let root_id = root.run.as_uuid();
    if sqlx::query_scalar::<_, Uuid>("SELECT id FROM companies WHERE id=$1 FOR UPDATE SKIP LOCKED")
        .bind(company)
        .fetch_optional(&mut *db)
        .await?
        .is_none()
    {
        return Ok(None);
    }
    let owned: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2
        AND parent_run_id IS NULL AND state IN ('succeeded','failed','cancelled')
        AND finished_at<clock_timestamp()-interval '90 days' FOR UPDATE SKIP LOCKED",
    )
    .bind(company)
    .bind(root_id)
    .fetch_optional(&mut *db)
    .await?;
    if owned.is_none() {
        return Ok(None);
    }
    let runs = lock_runs(db, company, root_id).await?;
    if runs == 0 {
        return Err(AppError::Internal(
            "Retained workflow is missing budget links".into(),
        ));
    }
    let jobs = lock_dependents(db, company, root_id).await?;
    let eligible: bool = sqlx::query_scalar(include_str!("storage_retention_eligible.sql"))
        .bind(company)
        .bind(root_id)
        .fetch_one(&mut *db)
        .await?;
    if !eligible {
        return Ok(None);
    }
    super::native_mcp::retire_on(db, company, root_id).await?;
    let calls = archive_usage(db, company, root_id).await?;
    let removed = sqlx::query("WITH retained AS MATERIALIZED (
        SELECT company_id,run_id FROM workflow_run_budgets WHERE company_id=$1 AND root_run_id=$2)
        DELETE FROM workflow_runs AS run USING retained WHERE run.company_id=retained.company_id AND run.id=retained.run_id")
        .bind(company).bind(root_id).execute(&mut *db).await?.rows_affected();
    // Owned members cannot disappear or change root. An extra member in the newer
    // snapshot must roll the whole candidate back rather than delete an unowned run.
    if removed != runs {
        return Err(AppError::Conflict(
            "Workflow tree changed during retention; rediscover it".into(),
        ));
    }
    // Runs must be absent before job-owned append-only checkpoint/effect cascades.
    // The transaction-local capture survives the run/execution/link cascades.
    let removed_jobs = sqlx::query(
        "DELETE FROM background_tasks AS job USING pg_temp.workflow_retention_jobs AS saved
        WHERE job.company_id=saved.company_id AND job.id=saved.job_id
        AND job.workflow_execution_id=saved.execution_id ",
    )
    .execute(db)
    .await?
    .rows_affected();
    if removed_jobs != jobs {
        return Err(AppError::Internal(
            "Workflow retention lost a captured job".into(),
        ));
    }
    Ok(Some(RetiredTree {
        runs: removed,
        jobs: removed_jobs,
        calls,
    }))
}

async fn lock_runs(db: &mut PgConnection, company: Uuid, root: Uuid) -> AppResult<u64> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM (
        SELECT run.id FROM workflow_runs AS run JOIN workflow_run_budgets AS link
            ON link.company_id=run.company_id AND link.run_id=run.id
        WHERE link.company_id=$1 AND link.root_run_id=$2 ORDER BY run.id FOR UPDATE OF run NOWAIT) AS owned")
        .bind(company).bind(root).fetch_one(db).await?;
    u64::try_from(count)
        .map_err(|_| AppError::Internal("Invalid retained workflow row count".into()))
}

async fn lock_dependents(db: &mut PgConnection, company: Uuid, root: Uuid) -> AppResult<u64> {
    // Scalar aggregates consume locking subqueries without transferring a tree-sized ID collection.
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM (
        SELECT step.id FROM workflow_executions AS step JOIN workflow_run_budgets AS link
            ON link.company_id=step.company_id AND link.run_id=step.run_id
        WHERE link.company_id=$1 AND link.root_run_id=$2 ORDER BY step.id FOR UPDATE OF step NOWAIT) AS owned")
        .bind(company).bind(root).fetch_one(&mut *db).await?;
    sqlx::query(
        "CREATE TEMP TABLE workflow_retention_jobs(company_id uuid NOT NULL,job_id uuid NOT NULL,
        execution_id uuid NOT NULL,PRIMARY KEY(company_id,job_id)) ON COMMIT DROP",
    )
    .execute(&mut *db)
    .await?;
    let jobs = sqlx::query("INSERT INTO pg_temp.workflow_retention_jobs(company_id,job_id,execution_id)
        SELECT job.company_id,job.id,job.workflow_execution_id FROM background_tasks AS job
        JOIN workflow_executions AS step ON step.company_id=job.company_id AND step.id=job.workflow_execution_id
        JOIN workflow_run_budgets AS link ON link.company_id=step.company_id AND link.run_id=step.run_id
        WHERE link.company_id=$1 AND link.root_run_id=$2 ORDER BY job.id FOR UPDATE OF job NOWAIT")
        .bind(company).bind(root).execute(&mut *db).await?.rows_affected();
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM (
        SELECT attempt.id FROM task_attempts AS attempt JOIN pg_temp.workflow_retention_jobs AS saved ON saved.job_id=attempt.task_id
        WHERE saved.company_id=$1 ORDER BY attempt.id FOR UPDATE OF attempt NOWAIT) AS owned")
        .bind(company).fetch_one(&mut *db).await?;
    sqlx::query_scalar::<_, Uuid>("SELECT root_run_id FROM workflow_root_budget_usage WHERE company_id=$1 AND root_run_id=$2 FOR UPDATE NOWAIT")
        .bind(company).bind(root).fetch_one(&mut *db).await?;
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM (
        SELECT call.request_id FROM model_usage_calls AS call JOIN workflow_run_budgets AS link
            ON link.company_id=call.company_id AND link.run_id=call.run_id
        WHERE link.company_id=$1 AND link.root_run_id=$2 ORDER BY call.request_id FOR UPDATE OF call NOWAIT) AS owned")
        .bind(company).bind(root).fetch_one(db).await?;
    Ok(jobs)
}

async fn archive_usage(db: &mut PgConnection, company: Uuid, root: Uuid) -> AppResult<u64> {
    Ok(sqlx::query("INSERT INTO model_usage_history(company_id,request_id,agent_id,workflow_id,version_id,provider,model,
        requested_at,reserved_input,reserved_output,input_tokens,output_tokens,usage_source,settled_at)
        SELECT call.company_id,call.request_id,call.agent_id,call.workflow_id,call.version_id,call.provider,call.model,
        call.requested_at,call.reserved_input,call.reserved_output,call.input_tokens,call.output_tokens,call.usage_source,call.settled_at
        FROM model_usage_calls AS call JOIN workflow_run_budgets AS link ON link.company_id=call.company_id AND link.run_id=call.run_id
        WHERE link.company_id=$1 AND link.root_run_id=$2")
        .bind(company).bind(root).execute(db).await?.rows_affected())
}
