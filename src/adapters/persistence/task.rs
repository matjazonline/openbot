use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde_json::Value;
use sqlx::{Postgres, QueryBuilder};
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        outreach::{
            CreateOutreachRequest, DueOutreach, OutreachProgress, OutreachReplyMatch,
            OutreachStatus,
        },
        task::{BackgroundTask, TaskStatus},
    },
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

#[derive(sqlx::FromRow, Debug)]
struct OutreachDb {
    id: Uuid,
    task_id: Uuid,
    status: String,
    required_threshold_percent: f64,
    expires_at: NaiveDateTime,
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
    async fn create_outreach_and_pause(
        &self,
        _request: CreateOutreachRequest,
    ) -> AppResult<OutreachProgress> {
        Err(AppError::Internal(
            "Outreach persistence is not configured".into(),
        ))
    }

    async fn find_correlated_outreach_reply(
        &self,
        _company_id: Uuid,
        _channel_id: Uuid,
        _thread_id: Uuid,
        _sender: &str,
        _references: &[String],
    ) -> AppResult<Option<OutreachReplyMatch>> {
        Ok(None)
    }

    async fn record_outreach_reply(
        &self,
        _matched: &OutreachReplyMatch,
        _response_message_id: Uuid,
    ) -> AppResult<OutreachProgress> {
        Err(AppError::Internal(
            "Outreach persistence is not configured".into(),
        ))
    }

    async fn list_due_outreaches(
        &self,
        _due_at: NaiveDateTime,
        _limit: i64,
    ) -> AppResult<Vec<DueOutreach>> {
        Ok(Vec::new())
    }

    async fn mark_outreach_timeout_pending(&self, _outreach_id: Uuid) -> AppResult<bool> {
        Ok(false)
    }

    async fn restore_outreach_waiting(&self, _outreach_id: Uuid) -> AppResult<()> {
        Ok(())
    }

    async fn get_outreach_context(&self, _task_id: Uuid) -> AppResult<Option<String>> {
        Ok(None)
    }

    async fn complete_outreach(&self, _task_id: Uuid) -> AppResult<()> {
        Ok(())
    }

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

    async fn get_outreach_thread_for_outbox(&self, _outbox_id: Uuid) -> AppResult<Option<Uuid>> {
        Ok(None)
    }

    async fn is_outbox_delivery_active(&self, _outbox_id: Uuid) -> AppResult<bool> {
        Ok(true)
    }

    async fn cancel_claimed_outbox(&self, _outbox_id: Uuid, _worker_id: Uuid) -> AppResult<bool> {
        Ok(false)
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

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>>;

    async fn list_company_tasks_page(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let tasks = self
            .list_company_tasks(company_id, channel_id, status, sort_asc)
            .await?;
        Ok(tasks
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
            .collect())
    }
}

#[async_trait]
impl TaskPersistence for PostgresPersistence {
    async fn create_outreach_and_pause(
        &self,
        request: CreateOutreachRequest,
    ) -> AppResult<OutreachProgress> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let outreach = sqlx::query_as::<_, OutreachDb>(
            r#"INSERT INTO task_outreaches (
                    id, task_id, outreach_key, status, required_threshold_percent,
                    expires_at, subject, body
               )
               SELECT $1, id, $2, 'waiting', $3, $4, $5, $6
               FROM background_tasks
               WHERE id = $7 AND company_id = $8
                 AND status = 'processing' AND worker_id = $9
                 AND lock_expires_at > CURRENT_TIMESTAMP
               ON CONFLICT (task_id, outreach_key) DO UPDATE
                   SET outreach_key = EXCLUDED.outreach_key
               RETURNING id, task_id, status,
                         required_threshold_percent::double precision AS required_threshold_percent,
                         expires_at"#,
        )
        .bind(request.id)
        .bind(&request.outreach_key)
        .bind(request.required_threshold_percent)
        .bind(request.expires_at)
        .bind(&request.subject)
        .bind(&request.body)
        .bind(request.task_id)
        .bind(request.company_id)
        .bind(request.worker_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Internal("Outreach task lease was lost before creation".into()))?;

        let created = outreach.id == request.id;
        if created {
            for (position, target) in request.targets.iter().enumerate() {
                sqlx::query(
                    r#"INSERT INTO email_outbox (
                            id, company_id, task_id, idempotency_key, payload
                       ) VALUES ($1, $2, $3, $4, $5)"#,
                )
                .bind(target.outbox_id)
                .bind(request.company_id)
                .bind(request.task_id)
                .bind(format!("outreach:{}:target:{}", outreach.id, position))
                .bind(&target.outbox_payload)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;

                sqlx::query(
                    r#"INSERT INTO task_outreach_targets (outreach_id, email, outbox_id)
                       VALUES ($1, $2, $3)"#,
                )
                .bind(outreach.id)
                .bind(&target.email)
                .bind(target.outbox_id)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
            }
        }

        let status = OutreachStatus::from_str(&outreach.status)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let suspended = status == OutreachStatus::Waiting;
        if suspended {
            let paused = sqlx::query(
                r#"UPDATE background_tasks
                   SET status = 'waiting_for_third_party_reply', wait_expires_at = $1,
                       worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                       updated_at = CURRENT_TIMESTAMP
                   WHERE id = $2 AND company_id = $3
                     AND status = 'processing' AND worker_id = $4
                     AND lock_expires_at > CURRENT_TIMESTAMP"#,
            )
            .bind(outreach.expires_at)
            .bind(request.task_id)
            .bind(request.company_id)
            .bind(request.worker_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            if paused.rows_affected() != 1 {
                return Err(AppError::Internal(
                    "Outreach task could not be paused".into(),
                ));
            }
        }

        let (target_count, response_count): (i64, i64) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint,
                      COUNT(*) FILTER (WHERE responded_at IS NOT NULL)::bigint
               FROM task_outreach_targets WHERE outreach_id = $1"#,
        )
        .bind(outreach.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;

        Ok(outreach_progress(
            &outreach,
            status,
            target_count,
            response_count,
            suspended,
        ))
    }

    async fn find_correlated_outreach_reply(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Uuid,
        sender: &str,
        references: &[String],
    ) -> AppResult<Option<OutreachReplyMatch>> {
        if references.is_empty() {
            return Ok(None);
        }
        let row = sqlx::query_as::<_, (Uuid, Uuid, String)>(
            r#"SELECT outreach.id, task.id, target.email::text
               FROM task_outreaches outreach
               JOIN background_tasks task ON task.id = outreach.task_id
               JOIN task_outreach_targets target ON target.outreach_id = outreach.id
               JOIN email_outbox outbox ON outbox.id = target.outbox_id
               WHERE task.company_id = $1 AND task.channel_id = $2 AND task.thread_id = $3
                 AND target.email = $4
                  AND outreach.status IN (
                      'waiting', 'timeout_pending_approval', 'threshold_met', 'completed'
                  )
                 AND outbox.status = 'sent'
                 AND outbox.provider_message_id = ANY($5)
               ORDER BY outreach.created_at DESC
               LIMIT 1"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(thread_id)
        .bind(sender.trim())
        .bind(references)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(
            row.map(|(outreach_id, task_id, target_email)| OutreachReplyMatch {
                outreach_id,
                task_id,
                target_email,
            }),
        )
    }

    async fn record_outreach_reply(
        &self,
        matched: &OutreachReplyMatch,
        response_message_id: Uuid,
    ) -> AppResult<OutreachProgress> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let mut outreach = sqlx::query_as::<_, OutreachDb>(
            r#"SELECT id, task_id, status,
                      required_threshold_percent::double precision AS required_threshold_percent,
                      expires_at
               FROM task_outreaches WHERE id = $1 FOR UPDATE"#,
        )
        .bind(matched.outreach_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;

        sqlx::query(
            r#"UPDATE task_outreach_targets
               SET responded_at = CURRENT_TIMESTAMP, response_message_id = $3
               WHERE outreach_id = $1 AND email = $2 AND responded_at IS NULL"#,
        )
        .bind(matched.outreach_id)
        .bind(&matched.target_email)
        .bind(response_message_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        let (target_count, response_count): (i64, i64) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint,
                      COUNT(*) FILTER (WHERE responded_at IS NOT NULL)::bigint
               FROM task_outreach_targets WHERE outreach_id = $1"#,
        )
        .bind(matched.outreach_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let required = required_response_count(target_count, outreach.required_threshold_percent);
        let current_status = OutreachStatus::from_str(&outreach.status)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let reached = response_count >= required as i64;

        if reached
            && matches!(
                current_status,
                OutreachStatus::Waiting | OutreachStatus::TimeoutPendingApproval
            )
        {
            sqlx::query(
                r#"UPDATE task_outreaches SET status = 'threshold_met',
                       updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            )
            .bind(matched.outreach_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            sqlx::query(
                r#"UPDATE background_tasks SET status = 'pending', run_at = CURRENT_TIMESTAMP,
                       wait_expires_at = NULL, worker_id = NULL, locked_at = NULL,
                       lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
                   WHERE id = $1 AND status IN (
                       'waiting_for_third_party_reply', 'pending_approval'
                   )"#,
            )
            .bind(outreach.task_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            sqlx::query(
                r#"UPDATE human_approvals SET status = 'expired', updated_at = CURRENT_TIMESTAMP
                   WHERE task_id = $1 AND action_type = 'quorum_timeout'
                     AND status = 'pending'"#,
            )
            .bind(outreach.task_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            outreach.status = OutreachStatus::ThresholdMet.as_str().to_string();
        }
        tx.commit().await.map_err(AppError::from)?;

        let status = OutreachStatus::from_str(&outreach.status)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        Ok(outreach_progress(
            &outreach,
            status,
            target_count,
            response_count,
            false,
        ))
    }

    async fn list_due_outreaches(
        &self,
        due_at: NaiveDateTime,
        limit: i64,
    ) -> AppResult<Vec<DueOutreach>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                Uuid,
                Uuid,
                Option<Uuid>,
                f64,
                i64,
                i64,
                NaiveDateTime,
            ),
        >(
            r#"SELECT outreach.id, task.id, task.company_id, task.channel_id, task.thread_id,
                      outreach.required_threshold_percent::double precision,
                      COUNT(target.*)::bigint,
                      COUNT(target.*) FILTER (WHERE target.responded_at IS NOT NULL)::bigint,
                      outreach.expires_at
               FROM task_outreaches outreach
               JOIN background_tasks task ON task.id = outreach.task_id
               JOIN task_outreach_targets target ON target.outreach_id = outreach.id
               WHERE outreach.status = 'waiting' AND outreach.expires_at <= $1
                 AND task.status = 'waiting_for_third_party_reply'
               GROUP BY outreach.id, task.id
               ORDER BY outreach.expires_at, outreach.id
               LIMIT $2"#,
        )
        .bind(due_at)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    outreach_id,
                    task_id,
                    company_id,
                    channel_id,
                    thread_id,
                    required_threshold_percent,
                    target_count,
                    response_count,
                    expires_at,
                )| DueOutreach {
                    outreach_id,
                    task_id,
                    company_id,
                    channel_id,
                    thread_id,
                    required_threshold_percent,
                    target_count: target_count as usize,
                    response_count: response_count as usize,
                    expires_at,
                },
            )
            .collect())
    }

    async fn mark_outreach_timeout_pending(&self, outreach_id: Uuid) -> AppResult<bool> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let task_id = sqlx::query_scalar::<_, Uuid>(
            r#"UPDATE task_outreaches
               SET status = 'timeout_pending_approval', updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'waiting' AND expires_at <= CURRENT_TIMESTAMP
               RETURNING task_id"#,
        )
        .bind(outreach_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let Some(task_id) = task_id else {
            tx.rollback().await.map_err(AppError::from)?;
            return Ok(false);
        };
        let updated = sqlx::query(
            r#"UPDATE background_tasks
               SET status = 'pending_approval', wait_expires_at = NULL,
                   worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'waiting_for_third_party_reply'"#,
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(updated.rows_affected() == 1)
    }

    async fn restore_outreach_waiting(&self, outreach_id: Uuid) -> AppResult<()> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let row = sqlx::query_as::<_, (Uuid, NaiveDateTime)>(
            r#"UPDATE task_outreaches SET status = 'waiting', updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'timeout_pending_approval'
               RETURNING task_id, expires_at"#,
        )
        .bind(outreach_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if let Some((task_id, expires_at)) = row {
            sqlx::query(
                r#"UPDATE background_tasks
                   SET status = 'waiting_for_third_party_reply', wait_expires_at = $2,
                       updated_at = CURRENT_TIMESTAMP
                   WHERE id = $1 AND status = 'pending_approval'"#,
            )
            .bind(task_id)
            .bind(expires_at)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok(())
    }

    async fn get_outreach_context(&self, task_id: Uuid) -> AppResult<Option<String>> {
        let rows = sqlx::query_as::<_, (String, String, String, f64, String, bool)>(
            r#"SELECT outreach.subject, outreach.body, outreach.status,
                      outreach.required_threshold_percent::double precision,
                      target.email::text, target.responded_at IS NOT NULL
               FROM task_outreaches outreach
               JOIN task_outreach_targets target ON target.outreach_id = outreach.id
               WHERE outreach.id = (
                   SELECT id FROM task_outreaches
                   WHERE task_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1
               )
               ORDER BY outreach.created_at DESC, target.email"#,
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        let Some((subject, body, status, threshold, _, _)) = rows.first() else {
            return Ok(None);
        };
        let responded = rows
            .iter()
            .filter(|row| row.5)
            .map(|row| row.4.clone())
            .collect::<Vec<_>>();
        let outstanding = rows
            .iter()
            .filter(|row| !row.5)
            .map(|row| row.4.clone())
            .collect::<Vec<_>>();
        Ok(Some(format!(
            "Outreach status: {status}\nSubject: {subject}\nRequest: {body}\nRequired threshold: {threshold:.1}%\nResponses: {}/{}\nRespondents: {}\nOutstanding: {}",
            responded.len(),
            rows.len(),
            if responded.is_empty() {
                "none".to_string()
            } else {
                responded.join(", ")
            },
            if outstanding.is_empty() {
                "none".to_string()
            } else {
                outstanding.join(", ")
            },
        )))
    }

    async fn complete_outreach(&self, task_id: Uuid) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE task_outreaches SET status = 'completed', updated_at = CURRENT_TIMESTAMP
               WHERE task_id = $1 AND status IN ('threshold_met', 'proceed_partial')"#,
        )
        .bind(task_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(())
    }

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

    async fn get_outreach_thread_for_outbox(&self, outbox_id: Uuid) -> AppResult<Option<Uuid>> {
        sqlx::query_scalar(
            r#"SELECT task.thread_id
               FROM task_outreach_targets target
               JOIN task_outreaches outreach ON outreach.id = target.outreach_id
               JOIN background_tasks task ON task.id = outreach.task_id
               WHERE target.outbox_id = $1"#,
        )
        .bind(outbox_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)
        .map(Option::flatten)
    }

    async fn is_outbox_delivery_active(&self, outbox_id: Uuid) -> AppResult<bool> {
        sqlx::query_scalar::<_, bool>(
            r#"SELECT CASE
                   WHEN target.outbox_id IS NULL THEN true
                   ELSE outreach.status = 'waiting'
               END
               FROM email_outbox outbox
               LEFT JOIN task_outreach_targets target ON target.outbox_id = outbox.id
               LEFT JOIN task_outreaches outreach ON outreach.id = target.outreach_id
               WHERE outbox.id = $1"#,
        )
        .bind(outbox_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)
        .map(|active| active.unwrap_or(false))
    }

    async fn cancel_claimed_outbox(&self, outbox_id: Uuid, worker_id: Uuid) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE email_outbox SET status = 'failed', last_error = 'Outreach closed',
                   worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'sending' AND worker_id = $2"#,
        )
        .bind(outbox_id)
        .bind(worker_id)
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
        let source_message_id = payload
            .pointer("/inbound_message/message_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let targets = task_targets(&payload, company_id, channel_id, thread_id)?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"INSERT INTO background_tasks (
                    id, company_id, channel_id, thread_id, source_message_id,
                    task_type, status, payload
               )
               VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7)
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
        sqlx::query(
            r#"UPDATE background_tasks
               SET payload = $1, updated_at = CURRENT_TIMESTAMP
               WHERE id = $2"#,
        )
        .bind(payload)
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
        let result = sqlx::query(
            r#"UPDATE background_tasks
               SET payload = $1, updated_at = CURRENT_TIMESTAMP
               WHERE id = $2 AND status = 'processing' AND worker_id = $3
                  AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(payload)
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
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
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
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"UPDATE task_outreaches SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP
               WHERE task_id = $1 AND status IN ('waiting', 'timeout_pending_approval')"#,
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"UPDATE email_outbox SET status = 'failed', last_error = 'Task stopped',
                   updated_at = CURRENT_TIMESTAMP
               WHERE task_id = $1 AND status = 'pending'"#,
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
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

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>> {
        self.list_company_tasks_page(company_id, channel_id, status, sort_asc, 0, 200)
            .await
    }

    async fn list_company_tasks_page(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
        offset: i64,
        limit: i64,
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
        query
            .push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

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

fn required_response_count(target_count: i64, threshold_percent: f64) -> usize {
    ((target_count as f64 * threshold_percent / 100.0).ceil() as usize).max(1)
}

fn outreach_progress(
    outreach: &OutreachDb,
    status: OutreachStatus,
    target_count: i64,
    response_count: i64,
    suspended: bool,
) -> OutreachProgress {
    OutreachProgress {
        id: outreach.id,
        task_id: outreach.task_id,
        status,
        required_threshold_percent: outreach.required_threshold_percent,
        target_count: target_count as usize,
        response_count: response_count as usize,
        required_response_count: required_response_count(
            target_count,
            outreach.required_threshold_percent,
        ),
        expires_at: outreach.expires_at,
        suspended,
    }
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
    use crate::entities::message::{Message, MessageDirection, MessageRole};
    use crate::services::outbound_dispatcher::OutboundEmail;
    use crate::use_cases::{
        channel::ChannelPersistence, company::CompanyPersistence, thread::ThreadPersistence,
        user::UserPersistence,
    };

    #[test]
    fn quorum_threshold_rounds_up() {
        assert_eq!(required_response_count(1, 100.0), 1);
        assert_eq!(required_response_count(3, 50.0), 2);
        assert_eq!(required_response_count(4, 50.0), 2);
        assert_eq!(required_response_count(10, 20.0), 2);
    }

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

    #[tokio::test]
    async fn outreach_reply_reaches_quorum_and_resumes_task() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let Ok(pool) = sqlx::PgPool::connect(&database_url).await else {
            return;
        };
        sqlx::migrate!().run(&pool).await.unwrap();
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let owner_email = format!("outreach_owner_{suffix}@example.com");
        persistence
            .create_user(&format!("outreach_owner_{suffix}"), &owner_email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &owner_email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            "Outreach Test",
            &format!("outreach-test-{suffix}"),
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
            "Outreach",
            "outreach",
            None,
            None,
            None,
            Some(vec![owner_email.clone()]),
            None,
            None,
        )
        .await
        .unwrap();
        let thread = persistence
            .create_thread(
                channel.id,
                "Need response",
                std::slice::from_ref(&owner_email),
            )
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(
                company.id,
                channel.id,
                Some(thread.id),
                "email_agent_dispatch",
                serde_json::json!({}),
            )
            .await
            .unwrap();
        let worker_id = Uuid::new_v4();
        assert!(
            persistence
                .claim_task(
                    task.id,
                    worker_id,
                    chrono::Utc::now().naive_utc() + chrono::Duration::minutes(5),
                )
                .await
                .unwrap()
        );
        let outreach_id = Uuid::new_v4();
        let outbox_id = Uuid::new_v4();
        let target_email = "vendor@supplier.example";
        let outbox_payload = serde_json::to_value(OutboundEmail {
            channel_id: channel.id,
            channel_name: channel.name.clone(),
            channel_slug: channel.slug.clone(),
            company_slug: company.slug.clone(),
            trigger_message_id: "<request@example.com>".into(),
            thread_references: Vec::new(),
            recipient_to: target_email.into(),
            recipients_cc: Vec::new(),
            subject: "Question".into(),
            body_text: "Please respond".into(),
            hop_count: 0,
            trace_channels: Vec::new(),
        })
        .unwrap();
        let progress = persistence
            .create_outreach_and_pause(CreateOutreachRequest {
                id: outreach_id,
                task_id: task.id,
                company_id: company.id,
                worker_id,
                outreach_key: "integration-outreach".into(),
                required_threshold_percent: 100.0,
                expires_at: chrono::Utc::now().naive_utc() + chrono::Duration::hours(24),
                subject: "Question".into(),
                body: "Please respond".into(),
                targets: vec![crate::entities::outreach::OutreachTargetRequest {
                    email: target_email.into(),
                    outbox_id,
                    outbox_payload,
                }],
            })
            .await
            .unwrap();
        assert!(progress.suspended);
        assert_eq!(
            persistence
                .get_task_by_id(task.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::WaitingForThirdPartyReply
        );

        let outbound_message_id = "<outreach-vendor@mailagents.test>";
        sqlx::query(
            "UPDATE email_outbox SET status = 'sent', provider_message_id = $2 WHERE id = $1",
        )
        .bind(outbox_id)
        .bind(outbound_message_id)
        .execute(&persistence.pool)
        .await
        .unwrap();
        let matched = persistence
            .find_correlated_outreach_reply(
                company.id,
                channel.id,
                thread.id,
                target_email,
                &[outbound_message_id.into()],
            )
            .await
            .unwrap()
            .unwrap();
        let response = persistence
            .create_message(&Message {
                id: Uuid::new_v4(),
                thread_id: thread.id,
                message_id: "<vendor-response@supplier.example>".into(),
                in_reply_to: Some(outbound_message_id.into()),
                references_list: vec![outbound_message_id.into()],
                sender: target_email.into(),
                recipients_to: vec![owner_email.clone()],
                recipients_cc: Vec::new(),
                subject: "Re: Question".into(),
                clean_text_body: "Confirmed".into(),
                raw_text_body: None,
                raw_html_body: None,
                attachments: None,
                direction: MessageDirection::Inbound,
                role: MessageRole::Human,
                thread_index: None,
                created_at: chrono::Utc::now().naive_utc(),
            })
            .await
            .unwrap();
        let progress = persistence
            .record_outreach_reply(&matched, response.id)
            .await
            .unwrap();
        assert_eq!(progress.status, OutreachStatus::ThresholdMet);
        assert_eq!(progress.response_count, 1);
        assert_eq!(
            persistence
                .get_task_by_id(task.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::Pending
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }
}
