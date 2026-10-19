//! Usage is a projection of the durable request ledger, never an independent execution ledger.
use crate::{
    app_error::{AppError, AppResult},
    services::harness::runs::{ModelReservation, RunCheckpoint, SavedModelTurn, TokenUsageSource},
};
use sqlx::PgConnection;

pub(crate) async fn persist_checkpoint_usage_on(
    tx: &mut PgConnection,
    previous: &RunCheckpoint,
    run: &RunCheckpoint,
) -> AppResult<()> {
    for reservation in &run.reservations {
        if !previous
            .reservations
            .iter()
            .any(|old| old.request_id == reservation.request_id)
        {
            reserve_on(tx, run, reservation).await?;
        }
    }
    for turn in &run.turns {
        if !previous
            .turns
            .iter()
            .any(|old| old.request_id == turn.request_id)
        {
            settle_on(tx, run, turn).await?;
        }
    }
    Ok(())
}

async fn reserve_on(
    tx: &mut PgConnection,
    run: &RunCheckpoint,
    reservation: &ModelReservation,
) -> AppResult<()> {
    let changed = sqlx::query(
        "INSERT INTO model_usage_calls
         (company_id,request_id,task_id,agent_id,provider,model,reserved_input,reserved_output,
          workflow_id,version_id,run_id,execution_id,root_run_id)
         SELECT task.company_id,$2,task.id,$4,$5,$6,$7,$8,
                owner.workflow_id,owner.version_id,owner.id,execution.id,budget.root_run_id
         FROM background_tasks AS task
         LEFT JOIN workflow_executions AS execution ON execution.company_id=task.company_id
            AND execution.id=task.workflow_execution_id AND task.queue_kind='workflow'
         LEFT JOIN workflow_runs AS owner ON owner.company_id=execution.company_id AND owner.id=execution.run_id
         LEFT JOIN workflow_run_budgets AS budget ON budget.company_id=owner.company_id AND budget.run_id=owner.id
         WHERE task.company_id=$1 AND task.id=$3",
    )
    .bind(run.identity.company_id)
    .bind(reservation.request_id.0)
    .bind(run.identity.task_id)
    .bind(run.identity.agent_id)
    .bind(run.identity.provider.as_str())
    .bind(run.identity.model.as_str())
    .bind(counter(reservation.input_tokens)?)
    .bind(counter(reservation.output_tokens)?)
    .execute(tx)
    .await?.rows_affected();
    if changed != 1 {
        return Err(AppError::Conflict("Model usage owner is missing".into()));
    }
    Ok(())
}

async fn settle_on(
    tx: &mut PgConnection,
    run: &RunCheckpoint,
    turn: &SavedModelTurn,
) -> AppResult<()> {
    let source = match turn.token_usage_source {
        TokenUsageSource::Reported => "reported",
        TokenUsageSource::Estimated => "estimated",
        TokenUsageSource::Mixed => "mixed",
    };
    let changed = sqlx::query(
        "UPDATE model_usage_calls SET input_tokens=$3,output_tokens=$4,usage_source=$5,
         settled_at=statement_timestamp() WHERE company_id=$1 AND request_id=$2 AND settled_at IS NULL",
    )
    .bind(run.identity.company_id)
    .bind(turn.request_id.0)
    .bind(counter(turn.input_tokens)?)
    .bind(counter(turn.output_tokens)?)
    .bind(source)
    .execute(tx)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(AppError::Conflict(
            "Model usage settlement missing or already committed".into(),
        ));
    }
    Ok(())
}

fn counter(value: u64) -> AppResult<i64> {
    if value > i32::MAX as u64 {
        return Err(AppError::BadRequest(
            "Model token counter exceeds supported bound".into(),
        ));
    }
    Ok(value as i64)
}

mod report;
