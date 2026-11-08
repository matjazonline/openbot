use super::*;

pub(super) async fn retry_on(
    db: &mut PgConnection,
    command: &CancelCommand,
    head: &RunHead,
) -> AppResult<bool> {
    // A runtime child is never retried: its terminal parent fact is immutable and may already be
    // consumed. Recovery is a new revision round or a rerun (07-D1 §D1.7). A parent failed by a
    // child outcome closed its parked job without a failed attempt, so it is never retry-safe.
    if head.state != RunState::Failed
        || matches!(
            head.causality.trigger().source(),
            TriggerSource::Child { .. }
        )
    {
        return Ok(false);
    }
    let execution: Option<Uuid> = sqlx::query_scalar("SELECT execution.id FROM workflow_executions AS execution JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.terminal_execution_id=execution.id WHERE run.company_id=$1 AND run.id=$2 AND execution.run_id=run.id FOR UPDATE OF execution")
        .bind(command.company_id.as_uuid()).bind(command.run_id.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(execution) = execution else {
        return Ok(false);
    };
    let job: Option<Uuid> = sqlx::query_scalar("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2  AND status='failed' FOR UPDATE")
        .bind(command.company_id.as_uuid()).bind(execution).fetch_optional(&mut *db).await?;
    let Some(job) = job else {
        return Ok(false);
    };
    let eligible: bool = sqlx::query_scalar("SELECT workflow_control_retry_safe($1,$2,$3)")
        .bind(command.company_id.as_uuid())
        .bind(command.run_id.as_uuid())
        .bind(job)
        .fetch_one(&mut *db)
        .await?;
    if !eligible {
        return Ok(false);
    }
    let conversation: Option<Uuid> = sqlx::query_scalar(
        "SELECT conversation_id FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2 \
         AND execution_id=$3 AND state='failed' FOR UPDATE",
    )
    .bind(command.company_id.as_uuid())
    .bind(command.run_id.as_uuid())
    .bind(execution)
    .fetch_optional(&mut *db)
    .await?;
    if let Some(conversation) = conversation {
        let available: bool = sqlx::query_scalar("SELECT agent_history_admit($1,$2)")
            .bind(command.company_id.as_uuid())
            .bind(conversation)
            .fetch_one(&mut *db)
            .await?;
        if !available {
            return Err(AppError::Conflict("Agent history queue is full".into()));
        }
    }
    sqlx::query("UPDATE background_tasks SET status='pending',run_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(command.company_id.as_uuid()).bind(job).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL,terminal_execution_id=NULL WHERE company_id=$1 AND id=$2")
        .bind(command.company_id.as_uuid()).bind(command.run_id.as_uuid()).execute(db).await?;
    Ok(true)
}
