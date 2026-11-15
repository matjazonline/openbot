use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    application::run_counts::RunCountsReader,
    entities::run_counts::{RunCountRow, RunCountSnapshot},
};
use uuid::Uuid;

/// Served by `workflow_runs_open_by_channel` (p08 D4.4).
pub(crate) const RUN_COUNTS_QUERY: &str = r#"
SELECT run.channel_id, run.state, COUNT(*)::bigint AS count
  FROM workflow_runs AS run
 WHERE run.company_id = $1 AND run.state IN ('queued', 'running', 'waiting')
   AND (run.channel_id = ANY($2)
        OR (run.channel_id IS NULL AND run.principal_id IS NULL)
        OR EXISTS (SELECT 1 FROM principals AS viewer
                   WHERE viewer.company_id=run.company_id AND viewer.user_id=$3
                     AND principal_direct_can_view(run.company_id,run.principal_id,viewer.id)))
 GROUP BY run.channel_id, run.state
"#;

#[derive(sqlx::FromRow)]
struct RunCountDb {
    channel_id: Option<Uuid>,
    state: String,
    count: i64,
}

#[async_trait::async_trait]
impl RunCountsReader for PostgresPersistence {
    async fn run_counts(
        &self,
        company_id: Uuid,
        visible_channel_ids: &[Uuid],
        viewer_user_id: Uuid,
    ) -> AppResult<RunCountSnapshot> {
        let rows = sqlx::query_as::<_, RunCountDb>(RUN_COUNTS_QUERY)
            .bind(company_id)
            .bind(visible_channel_ids)
            .bind(viewer_user_id)
            .fetch_all(self.pool())
            .await?;
        let rows = rows
            .into_iter()
            .map(|row| {
                Ok(RunCountRow {
                    channel_id: row.channel_id,
                    state: row.state,
                    count: row
                        .count
                        .try_into()
                        .map_err(|_| AppError::Internal("Negative run count".into()))?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok(RunCountSnapshot::from_rows(rows))
    }
}
