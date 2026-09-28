use super::*;
use crate::application::workflow::{activation::*, polling::*};
use std::time::Duration;

#[derive(sqlx::FromRow)]
struct CandidateRow {
    company_id: Uuid,
    run_id: Uuid,
    execution_id: Uuid,
    job_id: Uuid,
    work: String,
}
#[async_trait]
impl WorkflowPolling for PostgresPersistence {
    async fn poll_work(&self, after: Option<PollCursor>, limit: u16) -> AppResult<PollPage> {
        if !(1..=128).contains(&limit) {
            return Err(invalid());
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
                .execute(&mut *tx).await?;
            let rows = sqlx::query_as::<_, CandidateRow>(r#"
                SELECT job.company_id, execution.run_id, execution.id AS execution_id,
                       job.id AS job_id,
                       CASE WHEN wait.id IS NOT NULL THEN 'wait'
                            WHEN job.status = 'processing' THEN 'expired' ELSE 'job' END AS work
                FROM background_tasks AS job
                JOIN workflow_executions AS execution
                  ON execution.company_id = job.company_id AND execution.id = job.workflow_execution_id
                JOIN workflow_runs AS run
                  ON run.company_id = execution.company_id AND run.id = execution.run_id
                LEFT JOIN workflow_waits AS wait
                  ON wait.company_id = execution.company_id AND wait.execution_id = execution.id
                 AND wait.run_id = run.id AND wait.state = 'waiting'
                WHERE job.queue_kind = 'workflow' AND execution.completed_at IS NULL
                  AND ($1::uuid IS NULL OR job.id > $1)
                  AND (
                    (wait.id IS NULL AND run.state IN ('queued','running')
                     AND run.deadline > clock_timestamp()
                     AND ((job.status = 'pending' AND job.run_at <= clock_timestamp()
                           AND job.retry_count < job.max_retries)
                          OR (job.status = 'processing' AND job.lock_expires_at <= clock_timestamp())))
                    OR (wait.id IS NOT NULL AND (
                        wait.deadline <= clock_timestamp() OR run.deadline <= clock_timestamp()
                        OR run.state IN ('cancelled','failed','succeeded')
                        OR EXISTS (SELECT 1 FROM workflow_wait_events AS event
                            WHERE event.company_id = wait.company_id AND event.run_id = wait.run_id
                              AND event.execution_id = wait.execution_id
                              AND event.event_name = wait.event_name AND event.correlation = wait.correlation)))
                  )
                ORDER BY job.id LIMIT $2
            "#).bind(after.map(|cursor| cursor.0)).bind(i64::from(limit))
                .fetch_all(&mut *tx).await?;
            tx.commit().await?;
            let next = rows.last().map(|row| PollCursor(row.job_id));
            let candidates = rows.into_iter().map(candidate).collect::<AppResult<Vec<_>>>()?;
            Ok(PollPage { candidates, next })
        }).await.map_err(|_| lease::timed_out())?
    }
    async fn step_kind(&self, scope: ActivationRequest) -> AppResult<WorkflowStepKind> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
                .execute(&mut *tx).await?;
            let row: Option<(Uuid, String)> = sqlx::query_as(
                "SELECT run.binding_id, execution.step_id FROM workflow_runs AS run \
                 JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id \
                 JOIN background_tasks AS job ON job.company_id=run.company_id AND job.workflow_execution_id=execution.id \
                 WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3 AND job.id=$4 AND job.queue_kind='workflow'")
                .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0)
                .fetch_optional(&mut *tx).await?;
            let (binding, step) = row.ok_or_else(missing)?;
            let binding = admission_binding::read_saved_binding(&mut tx, scope.company, WorkflowBindingId::new(binding), scope.run.as_uuid()).await?;
            let step = StepId::parse(step).map_err(|_| invalid())?;
            let kind = binding.bundle().compiled().graph().definition().steps.get(&step).ok_or_else(invalid)?.step_type.as_str().to_owned();
            tx.commit().await?;
            Ok(WorkflowStepKind(kind))
        }).await.map_err(|_| lease::timed_out())?
    }
}
fn candidate(row: CandidateRow) -> AppResult<PollCandidate> {
    Ok(PollCandidate {
        scope: ActivationRequest {
            company: CompanyId::new(row.company_id),
            run: RunId::new(row.run_id),
            execution: ExecutionId::new(row.execution_id),
            job: WorkflowJobId(row.job_id),
        },
        work: match row.work.as_str() {
            "job" => PollWork::Job,
            "expired" => PollWork::ExpiredLease,
            "wait" => PollWork::Wait,
            _ => return Err(invalid()),
        },
    })
}
