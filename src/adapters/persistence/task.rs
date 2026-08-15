use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde_json::Value;
use sqlx::{Postgres, QueryBuilder};
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::task::{BackgroundTask, TaskStatus},
};

pub const TASK_LEASE_SECONDS: i64 = 15 * 60;

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
    pub worker_id: Option<Uuid>,
    pub locked_at: Option<NaiveDateTime>,
    pub lock_expires_at: Option<NaiveDateTime>,
    pub run_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(sqlx::FromRow, Debug)]
pub struct OutboxEmail {
    pub id: Uuid,
    pub payload: Value,
}

impl TryFrom<BackgroundTaskDb> for BackgroundTask {
    type Error = AppError;

    fn try_from(db: BackgroundTaskDb) -> AppResult<Self> {
        let status =
            TaskStatus::from_str(&db.status).map_err(|e| AppError::Internal(e.to_string()))?;

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
            worker_id: db.worker_id,
            locked_at: db.locked_at,
            lock_expires_at: db.lock_expires_at,
            run_at: db.run_at,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

#[async_trait]
pub trait TaskPersistence: Send + Sync {
    async fn claim_outbox_emails(
        &self,
        _worker_id: Uuid,
        _lock_expires_at: NaiveDateTime,
        _limit: i64,
    ) -> AppResult<Vec<OutboxEmail>> {
        Ok(Vec::new())
    }

    async fn mark_outbox_email_sent(
        &self,
        _id: Uuid,
        _worker_id: Uuid,
        _provider_message_id: &str,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn mark_outbox_email_failed(
        &self,
        _id: Uuid,
        _worker_id: Uuid,
        _error: &str,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn enqueue_task(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        task_type: &str,
        payload: Value,
    ) -> AppResult<BackgroundTask>;

    async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>>;

    async fn get_task_by_source_message_id(
        &self,
        _company_id: Uuid,
        _message_id: &str,
    ) -> AppResult<Option<BackgroundTask>> {
        Ok(None)
    }

    async fn update_task_payload(&self, id: Uuid, payload: Value) -> AppResult<()>;

    async fn update_claimed_task_payload(
        &self,
        id: Uuid,
        worker_id: Uuid,
        payload: Value,
    ) -> AppResult<bool> {
        self.update_task_payload(id, payload).await?;
        let _ = worker_id;
        Ok(true)
    }

    async fn renew_task_lease(
        &self,
        _id: Uuid,
        _worker_id: Uuid,
        _lock_expires_at: NaiveDateTime,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn claim_pending_tasks(
        &self,
        worker_id: Uuid,
        lock_expires_at: NaiveDateTime,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>>;

    async fn claim_task(
        &self,
        id: Uuid,
        worker_id: Uuid,
        lock_expires_at: NaiveDateTime,
    ) -> AppResult<bool>;

    async fn mark_task_completed(&self, id: Uuid, worker_id: Uuid) -> AppResult<bool>;

    async fn mark_task_failed(
        &self,
        id: Uuid,
        worker_id: Uuid,
        error_msg: &str,
        next_run_at: NaiveDateTime,
        is_dead_letter: bool,
    ) -> AppResult<bool>;

    async fn stop_task(&self, id: Uuid) -> AppResult<BackgroundTask>;

    async fn resume_task(&self, id: Uuid) -> AppResult<BackgroundTask>;

    async fn update_task_status(&self, id: Uuid, status: TaskStatus) -> AppResult<BackgroundTask>;

    async fn list_due_waiting_tasks(
        &self,
        due_at: NaiveDateTime,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>>;

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>>;
}

#[async_trait]
impl TaskPersistence for PostgresPersistence {
    async fn claim_outbox_emails(
        &self,
        worker_id: Uuid,
        lock_expires_at: NaiveDateTime,
        limit: i64,
    ) -> AppResult<Vec<OutboxEmail>> {
        sqlx::query_as::<_, OutboxEmail>(
            r#"WITH claimable AS (
                   SELECT id FROM email_outbox
                   WHERE (status = 'pending' AND available_at <= CURRENT_TIMESTAMP)
                      OR (status = 'sending' AND lock_expires_at <= CURRENT_TIMESTAMP)
                   ORDER BY available_at, id
                   FOR UPDATE SKIP LOCKED
                   LIMIT $1
               )
               UPDATE email_outbox outbox
               SET status = 'sending', worker_id = $2, locked_at = CURRENT_TIMESTAMP,
                   lock_expires_at = $3, updated_at = CURRENT_TIMESTAMP
               FROM claimable
               WHERE outbox.id = claimable.id
               RETURNING outbox.id, outbox.payload"#,
        )
        .bind(limit)
        .bind(worker_id)
        .bind(lock_expires_at)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)
    }

    async fn mark_outbox_email_sent(
        &self,
        id: Uuid,
        worker_id: Uuid,
        provider_message_id: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE email_outbox
               SET status = 'sent', provider_message_id = $3, sent_at = CURRENT_TIMESTAMP,
                   worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'sending' AND worker_id = $2"#,
        )
        .bind(id)
        .bind(worker_id)
        .bind(provider_message_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn mark_outbox_email_failed(
        &self,
        id: Uuid,
        worker_id: Uuid,
        error: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE email_outbox
               SET status = CASE WHEN retry_count + 1 >= 5 THEN 'failed' ELSE 'pending' END,
                   retry_count = retry_count + 1, last_error = $3,
                   available_at = CURRENT_TIMESTAMP
                       + make_interval(secs => power(2, LEAST(retry_count + 1, 8))::double precision),
                   worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'sending' AND worker_id = $2"#,
        )
        .bind(id)
        .bind(worker_id)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn enqueue_task(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        task_type: &str,
        payload: Value,
    ) -> AppResult<BackgroundTask> {
        let id = Uuid::new_v4();
        let wait_expires_at = quorum_expiry(&payload);
        let source_message_id = payload
            .pointer("/inbound_message/message_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let targets = task_targets(&payload, company_id, channel_id, thread_id)?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"INSERT INTO background_tasks (
                    id, company_id, channel_id, thread_id, source_message_id,
                    task_type, status, payload, wait_expires_at
               )
               VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7, $8)
               ON CONFLICT (company_id, source_message_id)
               DO UPDATE SET source_message_id = EXCLUDED.source_message_id
               RETURNING id, company_id, channel_id, thread_id, task_type, status, payload,
                          retry_count, max_retries, last_error, worker_id, locked_at, lock_expires_at,
                          run_at, created_at, updated_at"#,
        )
        .bind(id)
        .bind(company_id)
        .bind(channel_id)
        .bind(thread_id)
        .bind(source_message_id)
        .bind(task_type)
        .bind(payload)
        .bind(wait_expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;

        for (position, target) in targets.into_iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO task_channel_targets (
                        task_id, company_id, channel_id, thread_id, recipient_role, position
                   ) VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (task_id, channel_id) DO NOTHING"#,
            )
            .bind(db.id)
            .bind(company_id)
            .bind(target.channel_id)
            .bind(target.thread_id)
            .bind(target.recipient_role)
            .bind(position as i32)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }

        tx.commit().await.map_err(AppError::from)?;
        db.try_into()
    }

    async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>> {
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, task_type, status, payload,
                       retry_count, max_retries, last_error, worker_id, locked_at, lock_expires_at,
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

    async fn get_task_by_source_message_id(
        &self,
        company_id: Uuid,
        message_id: &str,
    ) -> AppResult<Option<BackgroundTask>> {
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, task_type, status, payload,
                      retry_count, max_retries, last_error, worker_id, locked_at, lock_expires_at,
                      run_at, created_at, updated_at
               FROM background_tasks
               WHERE company_id = $1 AND source_message_id = $2"#,
        )
        .bind(company_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        db.map(TryInto::try_into).transpose()
    }

    async fn update_task_payload(&self, id: Uuid, payload: Value) -> AppResult<()> {
        let wait_expires_at = quorum_expiry(&payload);
        sqlx::query(
            r#"UPDATE background_tasks
               SET payload = $1, wait_expires_at = $2, updated_at = CURRENT_TIMESTAMP
               WHERE id = $3"#,
        )
        .bind(payload)
        .bind(wait_expires_at)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
    }

    async fn update_claimed_task_payload(
        &self,
        id: Uuid,
        worker_id: Uuid,
        payload: Value,
    ) -> AppResult<bool> {
        let wait_expires_at = quorum_expiry(&payload);
        let result = sqlx::query(
            r#"UPDATE background_tasks
               SET payload = $1, wait_expires_at = $2, updated_at = CURRENT_TIMESTAMP
               WHERE id = $3 AND status = 'processing' AND worker_id = $4
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(payload)
        .bind(wait_expires_at)
        .bind(id)
        .bind(worker_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn renew_task_lease(
        &self,
        id: Uuid,
        worker_id: Uuid,
        lock_expires_at: NaiveDateTime,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE background_tasks
               SET lock_expires_at = $3, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'processing' AND worker_id = $2
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(id)
        .bind(worker_id)
        .bind(lock_expires_at)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn claim_pending_tasks(
        &self,
        worker_id: Uuid,
        lock_expires_at: NaiveDateTime,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let db_list = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"WITH claimable AS (
                   SELECT id
                   FROM background_tasks
                   WHERE (status = 'pending' AND run_at <= CURRENT_TIMESTAMP)
                      OR (status = 'processing' AND (lock_expires_at IS NULL OR lock_expires_at <= CURRENT_TIMESTAMP))
                   ORDER BY run_at ASC, created_at ASC, id ASC
                   FOR UPDATE SKIP LOCKED
                   LIMIT $1
               )
               UPDATE background_tasks AS task
               SET status = 'processing',
                   worker_id = $2,
                   locked_at = CURRENT_TIMESTAMP,
                   lock_expires_at = $3,
                   updated_at = CURRENT_TIMESTAMP
               FROM claimable
               WHERE task.id = claimable.id
               RETURNING task.id, task.company_id, task.channel_id, task.thread_id,
                         task.task_type, task.status, task.payload, task.retry_count,
                         task.max_retries, task.last_error, task.worker_id, task.locked_at,
                         task.lock_expires_at, task.run_at, task.created_at, task.updated_at"#,
        )
        .bind(limit)
        .bind(worker_id)
        .bind(lock_expires_at)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        let mut tasks = Vec::new();
        for db in db_list {
            tasks.push(db.try_into()?);
        }
        Ok(tasks)
    }

    async fn claim_task(
        &self,
        id: Uuid,
        worker_id: Uuid,
        lock_expires_at: NaiveDateTime,
    ) -> AppResult<bool> {
        let res = sqlx::query(
            r#"UPDATE background_tasks
               SET status = 'processing', worker_id = $2, locked_at = CURRENT_TIMESTAMP,
                   lock_expires_at = $3, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'pending' AND run_at <= CURRENT_TIMESTAMP"#,
        )
        .bind(id)
        .bind(worker_id)
        .bind(lock_expires_at)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(res.rows_affected() > 0)
    }

    async fn mark_task_completed(&self, id: Uuid, worker_id: Uuid) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE background_tasks
               SET status = 'completed', worker_id = NULL, locked_at = NULL,
                   lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'processing' AND worker_id = $2
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(id)
        .bind(worker_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.rows_affected() > 0)
    }

    async fn mark_task_failed(
        &self,
        id: Uuid,
        worker_id: Uuid,
        error_msg: &str,
        next_run_at: NaiveDateTime,
        is_dead_letter: bool,
    ) -> AppResult<bool> {
        let new_status = if is_dead_letter {
            "dead_letter"
        } else {
            "pending"
        };

        let result = sqlx::query(
            r#"UPDATE background_tasks
               SET status = $1, retry_count = retry_count + 1, last_error = $2,
                   run_at = $3, worker_id = NULL, locked_at = NULL,
                   lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE id = $4 AND status = 'processing' AND worker_id = $5
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(new_status)
        .bind(error_msg)
        .bind(next_run_at)
        .bind(id)
        .bind(worker_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.rows_affected() > 0)
    }

    async fn stop_task(&self, id: Uuid) -> AppResult<BackgroundTask> {
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"UPDATE background_tasks
               SET status = 'stopped', worker_id = NULL, locked_at = NULL,
                   lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1
                 AND status IN ('pending', 'processing', 'pending_approval',
                                'waiting_for_third_party_reply', 'failed')
               RETURNING id, company_id, channel_id, thread_id, task_type, status, payload,
                          retry_count, max_retries, last_error, worker_id, locked_at, lock_expires_at,
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
               SET status = 'pending', run_at = CURRENT_TIMESTAMP, worker_id = NULL,
                   locked_at = NULL, lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1
                 AND status IN ('stopped', 'pending_approval',
                                'waiting_for_third_party_reply', 'failed')
               RETURNING id, company_id, channel_id, thread_id, task_type, status, payload,
                          retry_count, max_retries, last_error, worker_id, locked_at, lock_expires_at,
                          run_at, created_at, updated_at"#,
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.try_into()
    }

    async fn update_task_status(&self, id: Uuid, status: TaskStatus) -> AppResult<BackgroundTask> {
        let db = match status {
            TaskStatus::PendingApproval => {
                sqlx::query_as::<_, BackgroundTaskDb>(
                    r#"UPDATE background_tasks
                   SET status = 'pending_approval', worker_id = NULL, locked_at = NULL,
                       lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
                   WHERE id = $1
                     AND status IN ('processing', 'waiting_for_third_party_reply')
                   RETURNING id, company_id, channel_id, thread_id, task_type, status, payload,
                             retry_count, max_retries, last_error, worker_id, locked_at,
                             lock_expires_at, run_at, created_at, updated_at"#,
                )
                .bind(id)
                .fetch_one(&self.pool)
                .await
            }
            TaskStatus::WaitingForThirdPartyReply => {
                sqlx::query_as::<_, BackgroundTaskDb>(
                    r#"UPDATE background_tasks
                   SET status = 'waiting_for_third_party_reply', worker_id = NULL,
                       locked_at = NULL, lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
                   WHERE id = $1 AND status IN ('processing', 'pending_approval')
                   RETURNING id, company_id, channel_id, thread_id, task_type, status, payload,
                             retry_count, max_retries, last_error, worker_id, locked_at,
                             lock_expires_at, run_at, created_at, updated_at"#,
                )
                .bind(id)
                .fetch_one(&self.pool)
                .await
            }
            _ => {
                return Err(AppError::Internal(format!(
                    "Unsupported task status transition target: {}",
                    status.as_str()
                )));
            }
        }
        .map_err(AppError::from)?;

        db.try_into()
    }

    async fn list_due_waiting_tasks(
        &self,
        due_at: NaiveDateTime,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let db_list = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, task_type, status, payload,
                      retry_count, max_retries, last_error, worker_id, locked_at, lock_expires_at,
                      run_at, created_at, updated_at
                FROM background_tasks
                WHERE status = 'waiting_for_third_party_reply'
                  AND payload #>> '{quorum_outreach,status}' = 'awaiting_quorum'
                  AND wait_expires_at <= $1
               ORDER BY wait_expires_at ASC, id ASC
               LIMIT $2"#,
        )
        .bind(due_at)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list
            .into_iter()
            .map(TryInto::try_into)
            .collect::<AppResult<Vec<_>>>()
    }

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>> {
        let mut query = QueryBuilder::<Postgres>::new(
            r#"SELECT id, company_id, channel_id, thread_id, task_type, status, payload,
                      retry_count, max_retries, last_error, worker_id, locked_at, lock_expires_at,
                      run_at, created_at, updated_at
               FROM background_tasks WHERE company_id = "#,
        );
        query.push_bind(company_id);
        if let Some(channel_id) = channel_id {
            query.push(" AND channel_id = ").push_bind(channel_id);
        }
        if let Some(status) = status {
            query.push(" AND status = ").push_bind(status.as_str());
        }
        if sort_asc {
            query.push(" ORDER BY created_at ASC, id ASC");
        } else {
            query.push(" ORDER BY created_at DESC, id DESC");
        }
        query.push(" LIMIT 200");

        let db_list = query
            .build_query_as::<BackgroundTaskDb>()
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

fn quorum_expiry(payload: &Value) -> Option<NaiveDateTime> {
    let value = payload.pointer("/quorum_outreach/expires_at")?.as_str()?;

    chrono::DateTime::parse_from_rfc3339(value)
        .map(|value| value.naive_utc())
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f"))
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f"))
        .ok()
}

struct TaskTarget {
    channel_id: Uuid,
    thread_id: Uuid,
    recipient_role: String,
}

fn task_targets(
    payload: &Value,
    company_id: Uuid,
    primary_channel_id: Uuid,
    primary_thread_id: Option<Uuid>,
) -> AppResult<Vec<TaskTarget>> {
    let Some(matches) = payload.get("channel_matches").and_then(Value::as_array) else {
        return Ok(primary_thread_id
            .map(|thread_id| {
                vec![TaskTarget {
                    channel_id: primary_channel_id,
                    thread_id,
                    recipient_role: "to".to_string(),
                }]
            })
            .unwrap_or_default());
    };

    if matches.is_empty() {
        return Ok(primary_thread_id
            .map(|thread_id| {
                vec![TaskTarget {
                    channel_id: primary_channel_id,
                    thread_id,
                    recipient_role: "to".to_string(),
                }]
            })
            .unwrap_or_default());
    }

    matches
        .iter()
        .map(|entry| {
            let target_company = json_uuid(entry, "/company/id")?;
            if target_company != company_id {
                return Err(AppError::Internal(
                    "A task cannot target channels from multiple companies".into(),
                ));
            }

            Ok(TaskTarget {
                channel_id: json_uuid(entry, "/channel/id")?,
                thread_id: json_uuid(entry, "/thread/id")?,
                recipient_role: entry
                    .get("recipient_role")
                    .and_then(Value::as_str)
                    .unwrap_or("to")
                    .to_string(),
            })
        })
        .collect()
}

fn json_uuid(value: &Value, pointer: &str) -> AppResult<Uuid> {
    let raw = value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal(format!("Missing task target field {pointer}")))?;
    Uuid::parse_str(raw)
        .map_err(|_| AppError::Internal(format!("Invalid task target UUID at {pointer}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::use_cases::{
        channel::ChannelPersistence, company::CompanyPersistence, thread::ThreadPersistence,
        user::UserPersistence,
    };

    #[tokio::test]
    async fn concurrent_workers_claim_a_task_once() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let Ok(pool) = sqlx::PgPool::connect(&database_url).await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("queue_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            "Queue Test",
            &format!("queue-test-{suffix}"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            "Queue",
            "queue",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let thread = persistence
            .create_thread(channel.id, "Queue", std::slice::from_ref(&email))
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(
                company.id,
                channel.id,
                Some(thread.id),
                "test",
                serde_json::json!({}),
            )
            .await
            .unwrap();

        let first_worker = Uuid::new_v4();
        let second_worker = Uuid::new_v4();
        let expires_at = chrono::Utc::now().naive_utc() + chrono::Duration::minutes(5);
        let (first, second) = tokio::join!(
            persistence.claim_pending_tasks(first_worker, expires_at, 1),
            persistence.claim_pending_tasks(second_worker, expires_at, 1)
        );
        let claimed: Vec<_> = first.unwrap().into_iter().chain(second.unwrap()).collect();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, task.id);

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }
}
