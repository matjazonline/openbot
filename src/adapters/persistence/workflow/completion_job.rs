use super::*;
use crate::application::workflow::{activation::ActivationRequest, lease::WorkflowFence};

#[derive(Clone, Copy)]
pub(super) enum CompletionOwner {
    Pure,
    Recovered,
    Parked,
    Fenced(WorkflowFence),
}
impl CompletionOwner {
    pub(super) fn event_kind(self) -> &'static str {
        match self {
            Self::Recovered => "step_final_error",
            Self::Parked => "wait_completed",
            Self::Pure => "pure_step_completed",
            Self::Fenced(_) => "io_step_completed",
        }
    }
}

pub(super) async fn finish(
    db: &mut PgConnection,
    scope: ActivationRequest,
    owner: CompletionOwner,
) -> AppResult<()> {
    match owner {
        CompletionOwner::Parked | CompletionOwner::Recovered => Ok(()),
        CompletionOwner::Pure => {
            let changed = sqlx::query("UPDATE background_tasks SET status='completed',updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='pending' AND worker_id IS NULL AND execution_generation IS NULL AND lock_expires_at IS NULL")
                .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).execute(db).await?;
            if changed.rows_affected() != 1 {
                return Err(invalid());
            }
            Ok(())
        }
        CompletionOwner::Fenced(fence) => finish_fenced(db, fence).await,
    }
}

async fn finish_fenced(db: &mut PgConnection, fence: WorkflowFence) -> AppResult<()> {
    let scope = fence.scope;
    let closed = sqlx::query("UPDATE task_attempts SET status='completed',stop_reason='completed',finished_at=clock_timestamp() WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status='processing'")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0).execute(&mut *db).await?;
    if closed.rows_affected() != 1 {
        return Err(invalid());
    }
    // Keep the expiry read before clearing it. A slow AFTER trigger must not let
    // the final job write commit after the lease or run deadline has elapsed.
    let expiry: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT lock_expires_at FROM background_tasks WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.job.0)
    .fetch_one(&mut *db)
    .await?;
    let closed = sqlx::query("UPDATE background_tasks SET status='completed',updated_at=clock_timestamp(),worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='processing' AND worker_id=$4 AND execution_generation=$5 AND retry_count+1=$6 AND lock_expires_at>clock_timestamp() AND EXISTS (SELECT 1 FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$7 AND run.deadline>clock_timestamp())")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).bind(fence.worker.0).bind(fence.generation.0).bind(fence.attempt.0).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    let timely: bool = sqlx::query_scalar("SELECT $3>clock_timestamp() AND deadline>clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(expiry).fetch_one(db).await?;
    if closed.rows_affected() != 1 || !timely {
        return Err(lease::timed_out());
    }
    Ok(())
}
