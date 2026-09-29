use super::*;

pub(super) async fn retry_on(
    db: &mut PgConnection,
    command: &CancelCommand,
    head: &RunHead,
) -> AppResult<bool> {
    // A child has already published an immutable parent terminal fact. Phase07
    // must define how a retry reconciles that fact before children can reopen.
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
    let job: Option<Uuid> = sqlx::query_scalar("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2 AND queue_kind='workflow' AND status='failed' FOR UPDATE")
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
    sqlx::query("UPDATE background_tasks SET status='pending',run_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(command.company_id.as_uuid()).bind(job).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL,terminal_execution_id=NULL WHERE company_id=$1 AND id=$2")
        .bind(command.company_id.as_uuid()).bind(command.run_id.as_uuid()).execute(db).await?;
    Ok(true)
}
