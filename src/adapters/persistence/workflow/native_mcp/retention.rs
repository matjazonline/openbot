//! The real run collector owns run -> call retirement; no native owner row is locked.
use super::*;

pub(in crate::adapters::persistence::workflow) async fn retire_on(
    db: &mut PgConnection,
    company: Uuid,
    root: Uuid,
) -> AppResult<()> {
    sqlx::query("SELECT call.id FROM native_mcp_calls AS call JOIN workflow_run_budgets AS budget
        ON budget.company_id=call.company_id AND budget.run_id=call.run_id
        WHERE budget.company_id=$1 AND budget.root_run_id=$2 ORDER BY call.id FOR UPDATE OF call NOWAIT")
        .bind(company).bind(root).execute(&mut *db).await?;
    // Deferred FKs/witnesses require that these actual runs disappear in this same
    // transaction. The invocation key, original owner, digest and outcome remain.
    sqlx::query("UPDATE native_mcp_calls AS call SET run_id=NULL,input=NULL,source_operation_id=NULL,source_session_id=NULL,
        retained_content_level=run.content_level,terminal_outcome=run.state,retired_at=clock_timestamp()
        FROM workflow_runs AS run JOIN workflow_run_budgets AS budget ON budget.company_id=run.company_id AND budget.run_id=run.id
        WHERE budget.company_id=$1 AND budget.root_run_id=$2 AND call.company_id=run.company_id AND call.run_id=run.id")
        .bind(company).bind(root).execute(db).await?;
    Ok(())
}
