use super::*;
use crate::application::workflow::capacity::{WorkflowCapacity, WorkflowCapacityPolicy};
use std::time::Duration;

#[derive(sqlx::FromRow)]
struct CapacityRow {
    global_limit: i32,
    company_limit: i32,
}

#[async_trait]
impl WorkflowCapacity for PostgresPersistence {
    async fn configure_capacity(&self, policy: WorkflowCapacityPolicy) -> AppResult<()> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
                .execute(&mut *tx).await?;
            insert(&mut tx, policy).await?;
            if lock(&mut tx).await? != policy {
                return Err(AppError::Conflict("Workflow capacity policy is already installed".into()));
            }
            tx.commit().await?;
            Ok(())
        }).await.map_err(|_| lease::timed_out())?
    }
}

async fn insert(db: &mut PgConnection, policy: WorkflowCapacityPolicy) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_capacity_policy (singleton,global_limit,company_limit) VALUES (true,$1,$2) ON CONFLICT (singleton) DO NOTHING")
        .bind(i32::from(policy.global())).bind(i32::from(policy.company())).execute(db).await?;
    Ok(())
}

/// Call only after the caller owns its run, execution and job. Never acquire a
/// different run or a root budget after this lock. There are no ancestor FKs.
pub(super) async fn lock(db: &mut PgConnection) -> AppResult<WorkflowCapacityPolicy> {
    insert(db, WorkflowCapacityPolicy::default()).await?;
    let row = sqlx::query_as::<_, CapacityRow>("SELECT global_limit,company_limit FROM workflow_capacity_policy WHERE singleton FOR UPDATE")
        .fetch_one(db).await?;
    WorkflowCapacityPolicy::new(
        u16::try_from(row.global_limit).map_err(|_| invalid())?,
        u16::try_from(row.company_limit).map_err(|_| invalid())?,
    )
}

/// Read only after holding the singleton; bounded by the immutable global ceiling.
pub(super) async fn live_companies(db: &mut PgConnection, limit: u16) -> AppResult<Vec<Uuid>> {
    Ok(sqlx::query_scalar(LIVE_COMPANIES_SQL)
        .bind(i64::from(limit))
        .fetch_all(db)
        .await?)
}

pub(super) const LIVE_COMPANIES_SQL: &str = r#"
    SELECT job.company_id FROM background_tasks AS job
    JOIN workflow_executions AS execution
      ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id
    JOIN workflow_runs AS run
      ON run.company_id=execution.company_id AND run.id=execution.run_id
    WHERE job.queue_kind='workflow' AND job.status='processing'
      AND job.lock_expires_at > clock_timestamp() AND execution.completed_at IS NULL
      AND run.state IN ('queued','running') AND run.deadline > clock_timestamp()
    LIMIT $1
"#;
