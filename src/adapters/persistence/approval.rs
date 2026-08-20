use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::approval::{ApprovalStatus, HumanApproval},
};

#[derive(sqlx::FromRow, Debug)]
pub struct HumanApprovalDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub step_key: String,
    pub approver_email: String,
    pub action_type: String,
    pub action_title: String,
    pub action_summary: String,
    pub payload: Value,
    pub token: Uuid,
    pub status: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<HumanApprovalDb> for HumanApproval {
    type Error = AppError;

    fn try_from(db: HumanApprovalDb) -> AppResult<Self> {
        let status =
            ApprovalStatus::from_str(&db.status).map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(HumanApproval {
            id: db.id,
            company_id: db.company_id,
            channel_id: db.channel_id,
            thread_id: db.thread_id,
            task_id: db.task_id,
            step_key: db.step_key,
            approver_email: db.approver_email,
            action_type: db.action_type,
            action_title: db.action_title,
            action_summary: db.action_summary,
            payload: db.payload,
            token: db.token.to_string(),
            status,
            expires_at: db.expires_at,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

#[async_trait]
pub trait ApprovalPersistence: Send + Sync {
    async fn create_approval(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        task_id: Option<Uuid>,
        step_key: &str,
        approver_email: &str,
        action_type: &str,
        action_title: &str,
        action_summary: &str,
        payload: Value,
        notification: Value,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> AppResult<(HumanApproval, bool)>;

    async fn find_approval_by_step_key(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        step_key: &str,
    ) -> AppResult<Option<HumanApproval>>;

    async fn get_approval_by_token(&self, token: &str) -> AppResult<Option<HumanApproval>>;

    async fn consume_pending_approval(
        &self,
        token: &str,
        status: ApprovalStatus,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>>;

    async fn consume_quorum_timeout_action(
        &self,
        _token: &str,
        _action: &str,
        _now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>> {
        Err(AppError::Internal(
            "Atomic quorum approval persistence is not configured".into(),
        ))
    }

    async fn expire_pending_approval(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>>;

    async fn list_approvals_by_channel(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<HumanApproval>>;
}

#[async_trait]
impl ApprovalPersistence for PostgresPersistence {
    async fn create_approval(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        task_id: Option<Uuid>,
        step_key: &str,
        approver_email: &str,
        action_type: &str,
        action_title: &str,
        action_summary: &str,
        payload: Value,
        notification: Value,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> AppResult<(HumanApproval, bool)> {
        let id = Uuid::new_v4();
        let token = Uuid::parse_str(token)
            .map_err(|e| AppError::Internal(format!("Invalid approval token: {}", e)))?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let db = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            INSERT INTO human_approvals (
                id, company_id, channel_id, thread_id, task_id,
                step_key, approver_email, action_type, action_title,
                action_summary, payload, token, status, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'pending', $13)
            ON CONFLICT ON CONSTRAINT human_approvals_thread_step_key
            DO UPDATE SET
                approver_email = CASE WHEN human_approvals.status = 'expired'
                    THEN EXCLUDED.approver_email ELSE human_approvals.approver_email END,
                action_type = CASE WHEN human_approvals.status = 'expired'
                    THEN EXCLUDED.action_type ELSE human_approvals.action_type END,
                action_title = CASE WHEN human_approvals.status = 'expired'
                    THEN EXCLUDED.action_title ELSE human_approvals.action_title END,
                action_summary = CASE WHEN human_approvals.status = 'expired'
                    THEN EXCLUDED.action_summary ELSE human_approvals.action_summary END,
                task_id = CASE WHEN human_approvals.status = 'expired'
                    THEN EXCLUDED.task_id ELSE human_approvals.task_id END,
                payload = CASE WHEN human_approvals.status = 'expired'
                    THEN EXCLUDED.payload ELSE human_approvals.payload END,
                token = CASE WHEN human_approvals.status = 'expired'
                    THEN EXCLUDED.token ELSE human_approvals.token END,
                status = CASE WHEN human_approvals.status = 'expired'
                    THEN 'pending' ELSE human_approvals.status END,
                expires_at = CASE WHEN human_approvals.status = 'expired'
                    THEN EXCLUDED.expires_at ELSE human_approvals.expires_at END,
                updated_at = CASE WHEN human_approvals.status = 'expired'
                    THEN CURRENT_TIMESTAMP ELSE human_approvals.updated_at END
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(company_id)
        .bind(channel_id)
        .bind(thread_id)
        .bind(task_id)
        .bind(step_key)
        .bind(approver_email)
        .bind(action_type)
        .bind(action_title)
        .bind(action_summary)
        .bind(payload)
        .bind(token)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create human approval: {}", e)))?;

        let created = db.token == token;
        if created {
            if let Some(task_id) = task_id {
                let paused = sqlx::query(
                    r#"UPDATE background_tasks
                       SET status = 'pending_approval', worker_id = NULL, locked_at = NULL,
                           lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
                       WHERE id = $1 AND company_id = $2
                         AND status IN ('processing', 'waiting_for_third_party_reply',
                                        'pending_approval')"#,
                )
                .bind(task_id)
                .bind(company_id)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                if paused.rows_affected() != 1 {
                    return Err(AppError::Internal(
                        "Approval task could not be paused".into(),
                    ));
                }
            }

            sqlx::query(
                r#"INSERT INTO email_outbox (
                        id, company_id, channel_id, task_id, idempotency_key, payload
                   ) VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(Uuid::new_v4())
            .bind(company_id)
            .bind(channel_id)
            .bind(task_id)
            .bind(format!("approval:{}", db.id))
            .bind(notification)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok((db.try_into()?, created))
    }

    async fn find_approval_by_step_key(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        step_key: &str,
    ) -> AppResult<Option<HumanApproval>> {
        let db = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            SELECT * FROM human_approvals
            WHERE company_id = $1 AND channel_id = $2
              AND (thread_id = $3 OR ($3 IS NULL AND thread_id IS NULL))
              AND step_key = $4
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(thread_id)
        .bind(step_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to query approval by step key: {}", e)))?;

        db.map(|d| d.try_into()).transpose()
    }

    async fn get_approval_by_token(&self, token: &str) -> AppResult<Option<HumanApproval>> {
        let Ok(token) = Uuid::parse_str(token) else {
            return Ok(None);
        };
        let db = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            SELECT * FROM human_approvals
            WHERE token = $1
            "#,
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to query approval by token: {}", e)))?;

        db.map(|d| d.try_into()).transpose()
    }

    async fn consume_pending_approval(
        &self,
        token: &str,
        status: ApprovalStatus,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>> {
        let Ok(token) = Uuid::parse_str(token) else {
            return Ok(None);
        };
        let db = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            UPDATE human_approvals
            SET status = $2, updated_at = CURRENT_TIMESTAMP
            WHERE token = $1
              AND status = 'pending'
              AND expires_at >= $3
            RETURNING *
            "#,
        )
        .bind(token)
        .bind(status.as_str())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to consume approval token: {}", e)))?;

        db.map(TryInto::try_into).transpose()
    }

    async fn consume_quorum_timeout_action(
        &self,
        token: &str,
        action: &str,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>> {
        let Ok(token) = Uuid::parse_str(token) else {
            return Ok(None);
        };
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let approval = sqlx::query_as::<_, HumanApprovalDb>(
            r#"SELECT * FROM human_approvals
               WHERE token = $1 AND status = 'pending' AND expires_at >= $2
                 AND action_type = 'quorum_timeout'
               FOR UPDATE"#,
        )
        .bind(token)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let Some(approval) = approval else {
            tx.rollback().await.map_err(AppError::from)?;
            return Ok(None);
        };
        let task_id = approval.task_id.ok_or_else(|| {
            AppError::Internal("Quorum timeout approval is missing its task".into())
        })?;

        let task_updated = match action {
            "proceed_partial" => {
                let outreach = sqlx::query(
                    r#"UPDATE task_outreaches SET status = 'proceed_partial',
                           updated_at = CURRENT_TIMESTAMP
                       WHERE task_id = $1 AND status = 'timeout_pending_approval'"#,
                )
                .bind(task_id)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                let task = sqlx::query(
                    r#"UPDATE background_tasks SET status = 'pending', run_at = CURRENT_TIMESTAMP,
                           wait_expires_at = NULL, updated_at = CURRENT_TIMESTAMP
                       WHERE id = $1 AND status = 'pending_approval'"#,
                )
                .bind(task_id)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                outreach.rows_affected() == 1 && task.rows_affected() == 1
            }
            "extend_24h" | "extend_48h" | "extend" => {
                let hours = if action == "extend_48h" { 48 } else { 24 };
                let expires_at = now + chrono::Duration::hours(hours);
                let outreach = sqlx::query(
                    r#"UPDATE task_outreaches SET status = 'waiting', expires_at = $2,
                           updated_at = CURRENT_TIMESTAMP
                       WHERE task_id = $1 AND status = 'timeout_pending_approval'"#,
                )
                .bind(task_id)
                .bind(expires_at)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                let task = sqlx::query(
                    r#"UPDATE background_tasks SET status = 'waiting_for_third_party_reply',
                           wait_expires_at = $2, updated_at = CURRENT_TIMESTAMP
                       WHERE id = $1 AND status = 'pending_approval'"#,
                )
                .bind(task_id)
                .bind(expires_at)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                outreach.rows_affected() == 1 && task.rows_affected() == 1
            }
            "reject" => {
                let outreach = sqlx::query(
                    r#"UPDATE task_outreaches SET status = 'cancelled',
                           updated_at = CURRENT_TIMESTAMP
                       WHERE task_id = $1 AND status = 'timeout_pending_approval'"#,
                )
                .bind(task_id)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                sqlx::query(
                    r#"UPDATE email_outbox SET status = 'failed', last_error = 'Outreach rejected',
                           updated_at = CURRENT_TIMESTAMP
                       WHERE task_id = $1 AND status = 'pending'"#,
                )
                .bind(task_id)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                let task = sqlx::query(
                    r#"UPDATE background_tasks SET status = 'stopped', wait_expires_at = NULL,
                           worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                           updated_at = CURRENT_TIMESTAMP
                       WHERE id = $1 AND status = 'pending_approval'"#,
                )
                .bind(task_id)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                outreach.rows_affected() == 1 && task.rows_affected() == 1
            }
            _ => false,
        };
        if !task_updated {
            tx.rollback().await.map_err(AppError::from)?;
            return Err(AppError::Internal(
                "Outreach is no longer awaiting this timeout decision".into(),
            ));
        }
        let status = if action == "reject" {
            ApprovalStatus::Rejected
        } else {
            ApprovalStatus::Approved
        };
        let updated = sqlx::query_as::<_, HumanApprovalDb>(
            r#"UPDATE human_approvals SET status = $2, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'pending' RETURNING *"#,
        )
        .bind(approval.id)
        .bind(status.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(Some(updated.try_into()?))
    }

    async fn expire_pending_approval(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>> {
        let Ok(token) = Uuid::parse_str(token) else {
            return Ok(None);
        };
        let db = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            UPDATE human_approvals
            SET status = 'expired', updated_at = CURRENT_TIMESTAMP
            WHERE token = $1
              AND status = 'pending'
              AND expires_at < $2
            RETURNING *
            "#,
        )
        .bind(token)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to expire approval token: {}", e)))?;

        db.map(TryInto::try_into).transpose()
    }

    async fn list_approvals_by_channel(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<HumanApproval>> {
        let list = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            SELECT * FROM human_approvals
            WHERE company_id = $1 AND channel_id = $2
            ORDER BY created_at DESC, id DESC
            LIMIT 200
            "#,
        )
        .bind(company_id)
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to list channel approvals: {}", e)))?;

        list.into_iter().map(|d| d.try_into()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::CompanyPersistence,
        thread::ThreadPersistence,
        user::UserPersistence,
    };

    #[tokio::test]
    async fn approval_lookup_is_scoped_and_token_is_consumed_once() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("approval_owner_{suffix}");
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
            "Approval Test",
            &format!("approval-test-{suffix}"),
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
            ChannelWrite {
                name: "Approval".into(),
                slug: "approval".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
        let thread = persistence
            .create_thread(channel.id, "Approval", std::slice::from_ref(&email_addr))
            .await
            .unwrap();
        let token = Uuid::new_v4().to_string();
        let (approval, created) = persistence
            .create_approval(
                company.id,
                channel.id,
                Some(thread.id),
                None,
                "deploy-step",
                &email,
                "tool",
                "Deploy",
                "Deploy application",
                serde_json::json!({}),
                serde_json::json!({}),
                &token,
                chrono::Utc::now() + chrono::Duration::hours(1),
            )
            .await
            .unwrap();
        assert!(created);

        // Counted by key alone, with no `status` filter. What is under test is that creating an
        // approval queues exactly one notification; whether a poller has since claimed it is the
        // poller's business, and asserting it is still 'pending' would be asserting that no
        // unscoped claim ran in between — which is not a property this code has.
        let queued_notifications: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM email_outbox WHERE idempotency_key = $1")
                .bind(format!("approval:{}", approval.id))
                .fetch_one(&persistence.pool)
                .await
                .unwrap();
        assert_eq!(queued_notifications, 1);
        assert_eq!(
            persistence
                .find_approval_by_step_key(company.id, channel.id, Some(thread.id), "deploy-step",)
                .await
                .unwrap()
                .unwrap()
                .id,
            approval.id
        );

        let now = chrono::Utc::now();
        let (first, second) = tokio::join!(
            persistence.consume_pending_approval(&token, ApprovalStatus::Approved, now),
            persistence.consume_pending_approval(&token, ApprovalStatus::Approved, now)
        );
        assert_eq!(
            [first.unwrap(), second.unwrap()]
                .into_iter()
                .filter(Option::is_some)
                .count(),
            1
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }
}
