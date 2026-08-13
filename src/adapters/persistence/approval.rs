use async_trait::async_trait;
use chrono::NaiveDateTime;
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
    pub token: String,
    pub status: String,
    pub expires_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl TryFrom<HumanApprovalDb> for HumanApproval {
    type Error = AppError;

    fn try_from(db: HumanApprovalDb) -> AppResult<Self> {
        let status = ApprovalStatus::from_str(&db.status)
            .map_err(|e| AppError::Internal(e.to_string()))?;

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
            token: db.token,
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
        workflow_id: Uuid,
        thread_id: Option<Uuid>,
        task_id: Option<Uuid>,
        step_key: &str,
        approver_email: &str,
        action_type: &str,
        action_title: &str,
        action_summary: &str,
        payload: Value,
        token: &str,
        expires_at: NaiveDateTime,
    ) -> AppResult<HumanApproval>;

    async fn find_approval_by_step_key(
        &self,
        thread_id: Option<Uuid>,
        step_key: &str,
    ) -> AppResult<Option<HumanApproval>>;

    async fn get_approval_by_token(&self, token: &str) -> AppResult<Option<HumanApproval>>;

    async fn update_approval_status(
        &self,
        id: Uuid,
        status: ApprovalStatus,
    ) -> AppResult<HumanApproval>;

    async fn list_approvals_by_workflow(
        &self,
        company_id: Uuid,
        workflow_id: Uuid,
    ) -> AppResult<Vec<HumanApproval>>;
}

#[async_trait]
impl ApprovalPersistence for PostgresPersistence {
    async fn create_approval(
        &self,
        company_id: Uuid,
        workflow_id: Uuid,
        thread_id: Option<Uuid>,
        task_id: Option<Uuid>,
        step_key: &str,
        approver_email: &str,
        action_type: &str,
        action_title: &str,
        action_summary: &str,
        payload: Value,
        token: &str,
        expires_at: NaiveDateTime,
    ) -> AppResult<HumanApproval> {
        let id = Uuid::new_v4();
        let db = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            INSERT INTO human_approvals (
                id, company_id, channel_id, thread_id, task_id,
                step_key, approver_email, action_type, action_title,
                action_summary, payload, token, status, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'pending', $13)
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(company_id)
        .bind(workflow_id)
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
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create human approval: {}", e)))?;

        db.try_into()
    }

    async fn find_approval_by_step_key(
        &self,
        thread_id: Option<Uuid>,
        step_key: &str,
    ) -> AppResult<Option<HumanApproval>> {
        let db = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            SELECT * FROM human_approvals
            WHERE (thread_id = $1 OR ($1 IS NULL AND thread_id IS NULL))
              AND step_key = $2
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(thread_id)
        .bind(step_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to query approval by step key: {}", e)))?;

        db.map(|d| d.try_into()).transpose()
    }

    async fn get_approval_by_token(&self, token: &str) -> AppResult<Option<HumanApproval>> {
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

    async fn update_approval_status(
        &self,
        id: Uuid,
        status: ApprovalStatus,
    ) -> AppResult<HumanApproval> {
        let db = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            UPDATE human_approvals
            SET status = $2, updated_at = CURRENT_TIMESTAMP
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(status.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to update approval status: {}", e)))?;

        db.try_into()
    }

    async fn list_approvals_by_workflow(
        &self,
        company_id: Uuid,
        workflow_id: Uuid,
    ) -> AppResult<Vec<HumanApproval>> {
        let list = sqlx::query_as::<_, HumanApprovalDb>(
            r#"
            SELECT * FROM human_approvals
            WHERE company_id = $1 AND channel_id = $2
            ORDER BY created_at DESC
            "#,
        )
        .bind(company_id)
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to list workflow approvals: {}", e)))?;

        list.into_iter().map(|d| d.try_into()).collect()
    }
}
