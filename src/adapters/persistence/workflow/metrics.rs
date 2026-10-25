//! One bounded, read-only snapshot. All values are aggregate numbers; no payloads or identifiers.
use super::*;
use crate::application::workflow::metrics::{WorkflowMetricPersistence, WorkflowMetrics};
use std::time::Duration;

#[async_trait]
impl WorkflowMetricPersistence for PostgresPersistence {
    async fn workflow_metrics(&self) -> AppResult<WorkflowMetrics> {
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut tx = self.pool.begin().await?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
                .execute(&mut *tx)
                .await?;
            sqlx::query("SET LOCAL statement_timeout = '2s'")
                .execute(&mut *tx)
                .await?;
            sqlx::query("SET LOCAL lock_timeout = '250ms'")
                .execute(&mut *tx)
                .await?;
            let sqlx::types::Json(snapshot) = sqlx::query_scalar::<
                _,
                sqlx::types::Json<WorkflowMetrics>,
            >(include_str!("metrics.sql"))
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(snapshot)
        })
        .await
        .map_err(|_| AppError::Timeout("Workflow metrics collection".into()))?
    }
}
