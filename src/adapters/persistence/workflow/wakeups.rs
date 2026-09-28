use super::*;
use crate::application::workflow::wakeups::*;
use std::time::Duration;

#[derive(sqlx::FromRow)]
struct WakeupRow {
    sequence: i64,
    terminal_state: String,
    execution_id: Option<Uuid>,
}

#[async_trait]
impl WorkflowParentWakeups for PostgresPersistence {
    async fn parent_wakeups(&self, query: ParentWakeupQuery) -> AppResult<Vec<ParentWakeup>> {
        if !(1..=128).contains(&query.limit) || query.after_sequence > i64::MAX as u64 {
            return Err(AppError::BadRequest(
                "Invalid parent wakeup read bound".into(),
            ));
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
                .execute(&mut *tx).await?;
            // No row locks, parent writes or copied output. The child association
            // was checked at admission and cannot change underneath this read.
            let rows = sqlx::query_as::<_, WakeupRow>(
                "SELECT event.sequence,event.terminal_state,event.terminal_execution_id AS execution_id \
                 FROM workflow_run_events AS event \
                 JOIN workflow_runs AS child ON child.company_id=event.company_id AND child.id=event.run_id \
                 JOIN workflow_executions AS parent ON parent.company_id=child.company_id \
                   AND parent.run_id=child.parent_run_id AND parent.id=child.parent_execution_id \
                 WHERE child.company_id=$1 AND child.id=$2 AND parent.run_id=$3 \
                   AND parent.id=$4 AND parent.step_id=$5 AND event.event_kind='parent_wakeup' \
                   AND event.sequence>$6 ORDER BY event.sequence LIMIT $7",
            ).bind(query.parent.company_id().as_uuid()).bind(query.child.as_uuid())
                .bind(query.parent.run_id().as_uuid()).bind(query.parent.execution_id().as_uuid())
                .bind(query.parent.step_id().as_str()).bind(query.after_sequence as i64)
                .bind(i64::from(query.limit)).fetch_all(&mut *tx).await?;
            tx.commit().await?;
            rows.into_iter().map(|row| decode(query.child, row)).collect()
        }).await.map_err(|_| AppError::Conflict("Parent wakeup read timed out".into()))?
    }
}

fn decode(child: RunId, row: WakeupRow) -> AppResult<ParentWakeup> {
    let state = match row.terminal_state.as_str() {
        "succeeded" => ChildTerminalState::Succeeded,
        "failed" => ChildTerminalState::Failed,
        "cancelled" => ChildTerminalState::Cancelled,
        _ => return Err(invalid()),
    };
    Ok(ParentWakeup {
        child,
        sequence: u64::try_from(row.sequence).map_err(|_| invalid())?,
        state,
        terminal_execution: row.execution_id.map(ExecutionId::new),
    })
}
