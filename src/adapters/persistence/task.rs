use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde_json::Value;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::task::{BackgroundTask, TaskStatus},
};

#[derive(sqlx::FromRow, Debug)]
pub struct BackgroundTaskDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub task_type: String,
    pub status: String,
    pub payload: Value,
    pub retry_count: i32,
    pub max_retries: i32,
    pub last_error: Option<String>,
    pub run_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl TryFrom<BackgroundTaskDb> for BackgroundTask {
    type Error = AppError;

    fn try_from(db: BackgroundTaskDb) -> AppResult<Self> {
        let status = TaskStatus::from_str(&db.status)
            .map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(BackgroundTask {
            id: db.id,
            company_id: db.company_id,
            channel_id: db.channel_id,
            thread_id: db.thread_id,
            task_type: db.task_type,
            status,
            payload: db.payload,
            retry_count: db.retry_count,
            max_retries: db.max_retries,
            last_error: db.last_error,
            run_at: db.run_at,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

#[async_trait]
pub trait TaskPersistence: Send + Sync {
    async fn enqueue_task(
        &self,
        company_id: Uuid,
        workflow_id: Uuid,
        thread_id: Option<Uuid>,
        task_type: &str,
        payload: Value,
    ) -> AppResult<BackgroundTask>;

    async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>>;

    async fn update_task_payload(&self, id: Uuid, payload: Value) -> AppResult<()>;

    async fn poll_next_pending_tasks(&self, limit: i64) -> AppResult<Vec<BackgroundTask>>;

    async fn mark_task_processing(&self, id: Uuid) -> AppResult<bool>;

    async fn mark_task_completed(&self, id: Uuid) -> AppResult<()>;

    async fn mark_task_failed(
        &self,
        id: Uuid,
        error_msg: &str,
        next_run_at: NaiveDateTime,
        is_dead_letter: bool,
    ) -> AppResult<()>;

    async fn stop_task(&self, id: Uuid) -> AppResult<BackgroundTask>;

    async fn resume_task(&self, id: Uuid) -> AppResult<BackgroundTask>;

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        workflow_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>>;
}

#[async_trait]
impl TaskPersistence for PostgresPersistence {
    async fn enqueue_task(
        &self,
        company_id: Uuid,
        workflow_id: Uuid,
        thread_id: Option<Uuid>,
        task_type: &str,
        payload: Value,
    ) -> AppResult<BackgroundTask> {
        let id = Uuid::new_v4();
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"INSERT INTO background_tasks (id, company_id, channel_id, thread_id, task_type, status, payload)
               VALUES ($1, $2, $3, $4, $5, 'pending', $6)
               RETURNING id, company_id, channel_id, thread_id, task_type, status, payload,
                         retry_count, max_retries, last_error,
                         run_at, created_at, updated_at"#,
        )
        .bind(id)
        .bind(company_id)
        .bind(workflow_id)
        .bind(thread_id)
        .bind(task_type)
        .bind(payload)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.try_into()
    }

    async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>> {
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, task_type, status, payload,
                      retry_count, max_retries, last_error,
                      run_at, created_at, updated_at
               FROM background_tasks WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        match db {
            Some(d) => Ok(Some(d.try_into()?)),
            None => Ok(None),
        }
    }

    async fn update_task_payload(&self, id: Uuid, payload: Value) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE background_tasks SET payload = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2"#,
        )
        .bind(payload)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
    }

    async fn poll_next_pending_tasks(&self, limit: i64) -> AppResult<Vec<BackgroundTask>> {
        let db_list = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, task_type, status, payload,
                      retry_count, max_retries, last_error,
                      run_at, created_at, updated_at
               FROM background_tasks
               WHERE status = 'pending' AND run_at <= CURRENT_TIMESTAMP
               ORDER BY run_at ASC
               LIMIT $1"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        let mut tasks = Vec::new();
        for db in db_list {
            tasks.push(db.try_into()?);
        }
        Ok(tasks)
    }

    async fn mark_task_processing(&self, id: Uuid) -> AppResult<bool> {
        let res = sqlx::query(
            r#"UPDATE background_tasks SET status = 'processing', updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND status = 'pending'"#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(res.rows_affected() > 0)
    }

    async fn mark_task_completed(&self, id: Uuid) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE background_tasks SET status = 'completed', updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
    }

    async fn mark_task_failed(
        &self,
        id: Uuid,
        error_msg: &str,
        next_run_at: NaiveDateTime,
        is_dead_letter: bool,
    ) -> AppResult<()> {
        let new_status = if is_dead_letter { "dead_letter" } else { "pending" };

        sqlx::query(
            r#"UPDATE background_tasks
               SET status = $1, retry_count = retry_count + 1, last_error = $2, run_at = $3, updated_at = CURRENT_TIMESTAMP
               WHERE id = $4"#,
        )
        .bind(new_status)
        .bind(error_msg)
        .bind(next_run_at)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
    }

    async fn stop_task(&self, id: Uuid) -> AppResult<BackgroundTask> {
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"UPDATE background_tasks
               SET status = 'stopped', updated_at = CURRENT_TIMESTAMP
               WHERE id = $1
               RETURNING id, company_id, channel_id, thread_id, task_type, status, payload,
                         retry_count, max_retries, last_error,
                         run_at, created_at, updated_at"#,
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.try_into()
    }

    async fn resume_task(&self, id: Uuid) -> AppResult<BackgroundTask> {
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"UPDATE background_tasks
               SET status = 'pending', run_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1
               RETURNING id, company_id, channel_id, thread_id, task_type, status, payload,
                         retry_count, max_retries, last_error,
                         run_at, created_at, updated_at"#,
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.try_into()
    }

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        workflow_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>> {
        let status_str = status.as_ref().map(|s| s.as_str());

        let db_list = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, task_type, status, payload,
                      retry_count, max_retries, last_error,
                      run_at, created_at, updated_at
               FROM background_tasks
               WHERE company_id = $1
                 AND ($2::uuid IS NULL OR channel_id = $2)
                 AND ($3::text IS NULL OR status = $3)
               ORDER BY
                 CASE WHEN $4 THEN created_at END ASC,
                 CASE WHEN NOT $4 THEN created_at END DESC"#,
        )
        .bind(company_id)
        .bind(workflow_id)
        .bind(status_str)
        .bind(sort_asc)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        let mut tasks = Vec::new();
        for db in db_list {
            tasks.push(db.try_into()?);
        }
        Ok(tasks)
    }
}
