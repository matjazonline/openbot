use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::AppResult,
    application::thread_activity::ThreadActivityReader,
    entities::thread_activity::{ThreadActivity, ThreadWorkSummary},
};

/// One run per thread: the newest unfinished one, else the newest of all. A failure is the
/// thread's last word only until a later run answers, so finished runs compete by time alone.
/// Served by `workflow_runs_by_thread`.
pub(crate) const THREAD_ACTIVITY_SQL: &str = r#"
SELECT DISTINCT ON (run.thread_id) run.thread_id, run.id AS run_id, run.state,
       run.waiting_reason
  FROM workflow_runs AS run
 WHERE run.company_id = $1 AND run.thread_id = ANY($2)
 ORDER BY run.thread_id, run.state IN ('succeeded', 'failed', 'cancelled'),
          run.created_at DESC, run.id DESC
"#;

#[derive(sqlx::FromRow)]
struct ThreadActivityRow {
    thread_id: Uuid,
    run_id: Uuid,
    state: String,
    waiting_reason: Option<String>,
}

#[async_trait::async_trait]
impl ThreadActivityReader for PostgresPersistence {
    async fn thread_activity(
        &self,
        company_id: Uuid,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadWorkSummary>> {
        if thread_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query_as::<_, ThreadActivityRow>(THREAD_ACTIVITY_SQL)
            .bind(company_id)
            .bind(thread_ids)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.thread_id,
                    ThreadWorkSummary {
                        run_id: row.run_id,
                        activity: ThreadActivity::from_run(
                            &row.state,
                            row.waiting_reason.as_deref(),
                        ),
                    },
                )
            })
            .collect())
    }
}
