use crate::entities::task::{TaskSuspension, TaskTransitionReason, TransitionActor};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::{PostgresPersistence, task::TransitionAttribution},
    app_error::{AppError, AppResult},
    entities::approval::{ApprovalAction, ApprovalStatus, ApprovalSubject, HumanApproval},
};

/// One approval to write, and the notification to queue alongside it.
///
/// Borrows the two halves of the request rather than copying them: the caller has just built the
/// notification body out of both and still owns them.
pub struct NewApproval<'a> {
    pub subject: &'a ApprovalSubject,
    pub action: &'a ApprovalAction,
    /// The serialized [`OutboundEmail`](crate::services::outbound_dispatcher::OutboundEmail) that
    /// carries the decision links. Written in the same transaction as the approval row, so a
    /// crash cannot leave an approval nobody was ever told about.
    pub notification: Value,
    /// The secret in the decision URLs. A `Uuid` end to end -- the column is one, so taking a
    /// `&str` here only added a parse that could fail on a value this code had just generated.
    pub token: Uuid,
    pub expires_at: DateTime<Utc>,
}

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

const APPROVAL_COLUMNS: &str = r#"id, company_id, channel_id, thread_id, task_id,
    step_key, approver_email, action_type, action_title, action_summary, payload, token,
    status, expires_at, created_at, updated_at"#;

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
    /// Writes the approval and queues its notification in one transaction.
    ///
    /// Returns the approval and whether *this* call created it: asking twice about the same
    /// `step_key` returns the standing one rather than mailing a second link.
    async fn create_approval(
        &self,
        new_approval: NewApproval<'_>,
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
        new_approval: NewApproval<'_>,
    ) -> AppResult<(HumanApproval, bool)> {
        let NewApproval {
            subject,
            action,
            notification,
            token,
            expires_at,
        } = new_approval;
        let task_id = subject.suspension.map(TaskSuspension::task_id);
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let db = sqlx::query_as::<_, HumanApprovalDb>(&format!(
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
            RETURNING {APPROVAL_COLUMNS}
            "#,
        ))
        .bind(id)
        .bind(subject.company_id)
        .bind(subject.channel_id)
        .bind(subject.thread_id)
        .bind(task_id)
        .bind(&action.step_key)
        .bind(subject.approver_email.as_str())
        .bind(&action.action_type)
        .bind(&action.title)
        .bind(&action.summary)
        .bind(&action.payload)
        .bind(token)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create human approval: {}", e)))?;

        let created = db.token == token;
        if created {
            if let Some(suspension) = subject.suspension {
                // Parking a task is a write against a possibly-leased row. A run that has been
                // superseded must not be able to make it, or it would park work the run that
                // now owns the task is actively doing.
                //
                // The lease branch is guarded on the generation; the second branch covers a row
                // that is already parked and so, by `background_tasks_lease_check`, holds no
                // lease for anyone to match. A caller with no lease gets NULL binds, which makes
                // the first branch unsatisfiable -- so it can only ever act on the second.
                let lease = suspension.lease();
                let attribution = TransitionAttribution::new(
                    TaskTransitionReason::ApprovalRequested,
                    TransitionActor::Approval(db.id),
                );
                let paused = sqlx::query(&format!(
                    r#"UPDATE background_tasks
                       SET status = 'pending_approval', worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                           lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP, {attribution}
                       WHERE id = $1 AND company_id = $2
                         AND (
                             ($3::uuid IS NOT NULL
                              AND status = 'processing'
                              AND worker_id = $3
                              AND execution_generation = $4
                              AND lock_expires_at > CURRENT_TIMESTAMP)
                             OR status IN ('waiting_for_third_party_reply', 'pending_approval')
                         )"#,
                    attribution = attribution.set_clause(),
                ))
                .bind(suspension.task_id())
                .bind(subject.company_id)
                .bind(lease.map(|lease| lease.worker_id))
                .bind(lease.map(|lease| lease.execution_generation))
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                if paused.rows_affected() != 1 {
                    return Err(AppError::Internal(
                        "Approval task could not be paused: it is not suspendable by this caller"
                            .into(),
                    ));
                }
            }

            sqlx::query(
                r#"INSERT INTO email_outbox (
                        id, company_id, channel_id, task_id, correlation_id,
                        idempotency_key, payload
                   ) VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            )
            .bind(Uuid::new_v4())
            .bind(subject.company_id)
            .bind(subject.channel_id)
            .bind(task_id)
            .bind(subject.correlation_id.as_uuid())
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
        let db = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"
            SELECT {APPROVAL_COLUMNS} FROM human_approvals
            WHERE company_id = $1 AND channel_id = $2
              AND (thread_id = $3 OR ($3 IS NULL AND thread_id IS NULL))
              AND step_key = $4
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        ))
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
        let db = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"
            SELECT {APPROVAL_COLUMNS} FROM human_approvals
            WHERE token = $1
            "#,
        ))
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
        let db = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"
            UPDATE human_approvals
            SET status = $2, updated_at = CURRENT_TIMESTAMP
            WHERE token = $1
              AND status = 'pending'
              AND expires_at >= $3
            RETURNING {APPROVAL_COLUMNS}
            "#,
        ))
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
        let approval = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"SELECT {APPROVAL_COLUMNS} FROM human_approvals
               WHERE token = $1 AND status = 'pending' AND expires_at >= $2
                 AND action_type = 'quorum_timeout'
               FOR UPDATE"#
        ))
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
        let transition_reason = match action {
            "proceed_partial" => TaskTransitionReason::ApprovalAccepted,
            "extend_24h" | "extend_48h" | "extend" => TaskTransitionReason::OutreachExtended,
            "reject" => TaskTransitionReason::OperatorStopped,
            _ => TaskTransitionReason::ApprovalAccepted,
        };
        let attribution =
            TransitionAttribution::new(transition_reason, TransitionActor::Approval(approval.id));

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
                let task = sqlx::query(&format!(
                    r#"UPDATE background_tasks SET status = 'pending', run_at = CURRENT_TIMESTAMP,
                           wait_expires_at = NULL, updated_at = CURRENT_TIMESTAMP, {attribution}
                       WHERE id = $1 AND status = 'pending_approval'"#,
                    attribution = attribution.set_clause(),
                ))
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
                let task = sqlx::query(&format!(
                    r#"UPDATE background_tasks SET status = 'waiting_for_third_party_reply',
                           wait_expires_at = $2, updated_at = CURRENT_TIMESTAMP, {attribution}
                       WHERE id = $1 AND status = 'pending_approval'"#,
                    attribution = attribution.set_clause(),
                ))
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
                let task = sqlx::query(&format!(
                    r#"UPDATE background_tasks SET status = 'stopped', wait_expires_at = NULL,
                           worker_id = NULL, execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL,
                           updated_at = CURRENT_TIMESTAMP, {attribution}
                       WHERE id = $1 AND status = 'pending_approval'"#,
                    attribution = attribution.set_clause(),
                ))
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
        let updated = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"UPDATE human_approvals SET status = $2, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'pending' RETURNING {APPROVAL_COLUMNS}"#
        ))
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
        let db = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"
            UPDATE human_approvals
            SET status = 'expired', updated_at = CURRENT_TIMESTAMP
            WHERE token = $1
              AND status = 'pending'
              AND expires_at < $2
            RETURNING {APPROVAL_COLUMNS}
            "#,
        ))
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
        let list = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"
            SELECT {APPROVAL_COLUMNS} FROM human_approvals
            WHERE company_id = $1 AND channel_id = $2
            ORDER BY created_at DESC, id DESC
            LIMIT 200
            "#,
        ))
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
    use crate::adapters::persistence::task::TaskPersistence;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::entities::correlation::CorrelationId;
    use crate::entities::task::{NewTask, TaskTransitionReason};
    use crate::use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        thread::ThreadPersistence,
        user::UserPersistence,
    };

    /// Parking a task is a leased write, and the two callers that do it are not equivalent.
    ///
    /// Regression for a guard that checked only `id`, `company_id` and `status`. Under it a run
    /// whose lease had already been reaped could still park the task, suspending work the run
    /// that now owns it was actively doing -- and the quorum-timeout sweep, which legitimately
    /// holds no lease, was what made that guard look sufficient.
    /// The "who and where" half of an approval, which every test here shares.
    fn approval_subject(
        company: &crate::entities::company::Company,
        channel: &crate::entities::channel::Channel,
        thread_id: Uuid,
        approver: &str,
    ) -> ApprovalSubject {
        ApprovalSubject {
            company_id: company.id,
            channel_id: channel.id,
            channel_name: channel.name.clone(),
            channel_slug: channel.slug.clone(),
            company_slug: company.slug.clone(),
            thread_id: Some(thread_id),
            suspension: None,
            correlation_id: CorrelationId::new(),
            approver_email: approver.into(),
        }
    }

    /// The "what" half. Only `step_key` distinguishes one of these tests' requests from another,
    /// which is exactly the asymmetry the two structs exist to express.
    fn deploy_action(step_key: &str) -> ApprovalAction {
        ApprovalAction {
            step_key: step_key.to_string(),
            action_type: "tool".to_string(),
            title: "Deploy".to_string(),
            summary: "Deploy application".to_string(),
            payload: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn only_the_run_that_owns_a_task_may_park_it() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("park_owner_{suffix}");
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
            CompanyWrite {
                name: "Park Test".to_string(),
                slug: format!("park-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Park".into(),
                slug: "park".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
        let thread = persistence
            .create_thread(channel.id, "Park", std::slice::from_ref(&email_addr))
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread.id),
                "test",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        let worker = Uuid::new_v4();
        assert!(
            persistence
                .claim_task(task.id, worker, Utc::now() + chrono::Duration::minutes(5))
                .await
                .unwrap()
        );
        let claimed = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        let lease = crate::entities::task::TaskLeaseRef::of(&claimed).expect("claim records lease");

        let park = async |suspension, step: &str| {
            persistence
                .create_approval(NewApproval {
                    subject: &ApprovalSubject {
                        suspension,
                        ..approval_subject(&company, &channel, thread.id, &email)
                    },
                    action: &deploy_action(step),
                    notification: serde_json::json!({}),
                    token: Uuid::new_v4(),
                    expires_at: Utc::now() + chrono::Duration::hours(1),
                })
                .await
        };

        // A superseded run: same task, same worker, a generation that is no longer current.
        let stale = crate::entities::task::TaskLeaseRef {
            execution_generation: Uuid::new_v4(),
            ..lease
        };
        assert!(
            park(Some(TaskSuspension::Leased(stale)), "stale-step")
                .await
                .is_err(),
            "a superseded run must not be able to park the task"
        );

        // Nor may a caller holding no lease at all park a task that is still running.
        assert!(
            park(
                Some(TaskSuspension::AlreadySuspended { task_id: task.id }),
                "unleased-step"
            )
            .await
            .is_err(),
            "an unleased caller must not be able to park a running task"
        );

        assert_eq!(
            persistence
                .get_task_by_id(task.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            crate::entities::task::TaskStatus::Processing,
            "the task is still running and still owned"
        );

        // The run that actually owns the lease parks it.
        let (_, created) = park(Some(TaskSuspension::Leased(lease)), "live-step")
            .await
            .unwrap();
        assert!(created);
        let parked = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        assert_eq!(
            parked.status,
            crate::entities::task::TaskStatus::PendingApproval
        );
        assert!(
            parked.worker_id.is_none() && parked.execution_generation.is_none(),
            "a parked task releases its lease"
        );
        let events = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 20)
            .await
            .unwrap();
        let requested = events
            .iter()
            .find(|event| event.reason == TaskTransitionReason::ApprovalRequested)
            .expect("parking records the exact approval transition");
        assert!(requested.related_approval_id.is_some());

        // Once parked there is no owner left to fence against, so the unleased sweep may act --
        // this is the quorum-timeout path.
        assert!(
            park(
                Some(TaskSuspension::AlreadySuspended { task_id: task.id }),
                "sweep-step"
            )
            .await
            .is_ok(),
            "the timeout sweep must still be able to move an already-parked task"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

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
            CompanyWrite {
                name: "Approval Test".to_string(),
                slug: format!("approval-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Approval".into(),
                slug: "approval".into(),
                enabled: false,
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
        let token = Uuid::new_v4();
        let (approval, created) = persistence
            .create_approval(NewApproval {
                subject: &approval_subject(&company, &channel, thread.id, &email),
                action: &deploy_action("deploy-step"),
                notification: serde_json::json!({}),
                token,
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            })
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
        let token_str = token.to_string();
        let (first, second) = tokio::join!(
            persistence.consume_pending_approval(&token_str, ApprovalStatus::Approved, now),
            persistence.consume_pending_approval(&token_str, ApprovalStatus::Approved, now)
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
