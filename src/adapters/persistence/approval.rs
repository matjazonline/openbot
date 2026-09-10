mod transitions;

use crate::entities::task::{TaskSuspension, TaskTransitionReason, TransitionActor};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::{
        PostgresPersistence, delivery::enqueue::insert_delivery_on, task::TransitionAttribution,
        thread::insert_message_on,
    },
    app_error::{AppError, AppResult},
    entities::{
        approval::{ApprovalStatus, HumanApproval, QuorumTimeoutAction},
        transport::PrincipalId,
    },
    use_cases::approval::{ApprovalPersistence, NewApproval},
};

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct HumanApprovalDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub task_id: Option<Uuid>,
    pub step_key: String,
    pub approver_email: String,
    pub approver_principal_id: Option<Uuid>,
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
    status, expires_at, created_at, updated_at, approver_principal_id"#;

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
            approver_principal_id: db.approver_principal_id.map(PrincipalId::new),
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

async fn approver_principal_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    approver_email: &str,
) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar::<_, Uuid>(
        r#"SELECT principal.id
           FROM participant_identities AS identity
           JOIN principals AS principal
             ON principal.company_id = identity.company_id
            AND principal.id = identity.principal_id
           WHERE identity.company_id = $1
             AND identity.transport = 'email'
             AND identity.status = 'verified'
             AND LOWER(identity.subject) = LOWER($2)
             AND principal.kind = 'person'
           ORDER BY identity.created_at, identity.id
           LIMIT 1"#,
    )
    .bind(company_id)
    .bind(approver_email)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)
}

#[async_trait]
impl ApprovalPersistence for PostgresPersistence {
    async fn create_approval(
        &self,
        new_approval: NewApproval<'_>,
    ) -> AppResult<(HumanApproval, bool)> {
        let NewApproval {
            invocation,
            subject,
            action,
            message,
            delivery,
            token,
            expires_at,
        } = new_approval;
        let task_id = subject.suspension.map(TaskSuspension::task_id);
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        transitions::lock_subject(&mut tx, subject).await?;
        let approver_principal_id =
            approver_principal_id(&mut tx, subject.company_id, subject.approver_email.as_str())
                .await?;
        let id = Uuid::new_v4();
        let db = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"
            INSERT INTO human_approvals (
                id, company_id, channel_id, thread_id, task_id,
                step_key, approver_email, action_type, action_title,
                action_summary, payload, token, status, expires_at, approver_principal_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'pending', $13, $14)
            ON CONFLICT ON CONSTRAINT human_approvals_thread_step_key
            DO UPDATE SET updated_at = human_approvals.updated_at
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
        .bind(approver_principal_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create human approval: {}", e)))?;

        let created = db.token == token;
        if db.status == "pending" {
            transitions::link_wait(&mut tx, subject, &db, invocation).await?;
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
                let ownership = suspension.ownership();
                let attribution = TransitionAttribution::new(
                    TaskTransitionReason::ApprovalRequested,
                    TransitionActor::Approval(db.id),
                );
                let paused = sqlx::query(&format!(
                    r#"UPDATE background_tasks
                       SET status = 'pending_approval', worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                           lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP, {attribution}
                       WHERE id = $1 AND company_id = $2
                         AND owner_principal_id IS NOT DISTINCT FROM $5
                         AND ownership_version = $6
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
                .bind(ownership.owner.principal_id().map(|id| id.as_uuid()))
                .bind(i64::try_from(ownership.version).map_err(|_| {
                    AppError::Conflict("Ownership version exhausted.".into())
                })?)
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
        }
        if created {
            // The note in the thread, then the mail that carries it. Both inside the same
            // transaction as the approval and the task it parked: an approval that parked a run
            // and then failed to tell anyone is a run nobody can un-park.
            insert_message_on(&mut tx, message).await?;
            insert_delivery_on(&mut tx, &delivery).await?;
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok((db.try_into()?, created))
    }

    async fn find_approval_by_step_key(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Uuid,
        step_key: &str,
    ) -> AppResult<Option<HumanApproval>> {
        let db = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"
            SELECT {APPROVAL_COLUMNS} FROM human_approvals
            WHERE company_id = $1 AND channel_id = $2 AND thread_id = $3
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

    async fn get_approval_by_id(
        &self,
        company_id: Uuid,
        approval_id: Uuid,
    ) -> AppResult<Option<HumanApproval>> {
        let db = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"SELECT {APPROVAL_COLUMNS} FROM human_approvals
               WHERE company_id = $1 AND id = $2"#
        ))
        .bind(company_id)
        .bind(approval_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        db.map(TryInto::try_into).transpose()
    }

    async fn decide_approval_and_transition(
        &self,
        token: &str,
        status: ApprovalStatus,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>> {
        let Ok(token) = Uuid::parse_str(token) else {
            return Ok(None);
        };
        self.transition_approval(token, status, now).await
    }

    async fn consume_quorum_timeout_action(
        &self,
        token: &str,
        action: QuorumTimeoutAction,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>> {
        let Ok(token) = Uuid::parse_str(token) else {
            return Ok(None);
        };
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let candidate = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"SELECT {APPROVAL_COLUMNS} FROM human_approvals
               WHERE token = $1 AND status = 'pending' AND expires_at >= $2
                 AND action_type = 'quorum_timeout'"#
        ))
        .bind(token)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let Some(candidate) = candidate else {
            tx.rollback().await.map_err(AppError::from)?;
            return Ok(None);
        };
        let task_id = candidate.task_id.ok_or_else(|| {
            AppError::Internal("Quorum timeout approval is missing its task".into())
        })?;
        let outreach_id = candidate
            .payload
            .get("outreach_id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| {
                AppError::Internal("Quorum timeout approval is missing its outreach".into())
            })?;
        // Every task/outreach writer uses this order. Read the candidate first only to discover
        // its ids, then prove it is still pending after the shared workflow rows are locked.
        let locked_outreach = sqlx::query_scalar::<_, Uuid>(
            r#"SELECT id FROM task_outreaches
               WHERE id = $1 AND task_id = $2 FOR UPDATE"#,
        )
        .bind(outreach_id)
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if locked_outreach.is_none() {
            tx.rollback().await.map_err(AppError::from)?;
            return Ok(None);
        }
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM background_tasks WHERE id = $1 FOR UPDATE")
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::from)?;
        let current_wait = sqlx::query_scalar::<_, bool>(
            r#"SELECT EXISTS (SELECT 1 FROM background_tasks task
                JOIN task_outreaches outreach ON outreach.id = task.awaited_outreach_id AND outreach.task_id = task.id AND outreach.company_id = task.company_id
                JOIN task_approval_waits wait ON wait.task_id = task.id AND wait.company_id = task.company_id
                WHERE task.id = $1 AND task.company_id = $2 AND task.status = 'pending_approval'
                    AND outreach.id = $3 AND outreach.ownership_version = task.ownership_version
                    AND wait.approval_id = $4 AND wait.state = 'waiting'
                    AND wait.ownership_version = task.ownership_version
                    AND wait.owner_principal_id IS NOT DISTINCT FROM task.owner_principal_id)"#,
        ).bind(task_id).bind(candidate.company_id).bind(outreach_id).bind(candidate.id).fetch_one(&mut *tx).await?;
        if !current_wait {
            return Ok(None);
        }
        let approval = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"SELECT {APPROVAL_COLUMNS} FROM human_approvals
               WHERE id = $1 AND status = 'pending' AND expires_at >= $2
                 AND action_type = 'quorum_timeout' FOR UPDATE"#
        ))
        .bind(candidate.id)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let Some(approval) = approval else {
            tx.rollback().await.map_err(AppError::from)?;
            return Ok(None);
        };
        // Every arm names the decision the human actually made. `reject` used to borrow
        // `operator_stopped`, which contradicted its own `approval` actor kind, and the `_` arm
        // filed anything it did not recognise as consent.
        let transition_reason = match action {
            QuorumTimeoutAction::ProceedPartial => TaskTransitionReason::ApprovalAccepted,
            QuorumTimeoutAction::Extend { .. } => TaskTransitionReason::OutreachExtended,
            QuorumTimeoutAction::Reject => TaskTransitionReason::ApprovalRejected,
        };
        let attribution =
            TransitionAttribution::new(transition_reason, TransitionActor::Approval(approval.id));

        let task_updated = match action {
            QuorumTimeoutAction::ProceedPartial => {
                let outreach_id = sqlx::query_scalar::<_, Uuid>(
                    r#"UPDATE task_outreaches SET status = 'proceed_partial', version = version + 1,
                           updated_at = CURRENT_TIMESTAMP
                       WHERE id = $1 AND task_id = $2 AND status = 'timeout_pending_approval'
                       RETURNING id"#,
                )
                .bind(outreach_id)
                .bind(task_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(AppError::from)?;
                if let Some(outreach_id) = outreach_id {
                    super::task::cancel_unsent_outreach_questions(&mut tx, outreach_id).await?;
                    sqlx::query(
                        "UPDATE task_outreach_targets SET status = 'expired' WHERE outreach_id = $1 AND status = 'active'",
                    )
                    .bind(outreach_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(AppError::from)?;
                }
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
                outreach_id.is_some() && task.rows_affected() == 1
            }
            QuorumTimeoutAction::Extend { hours } => {
                let expires_at = now + chrono::Duration::hours(hours);
                let outreach = sqlx::query(
                    r#"UPDATE task_outreaches SET status = 'waiting', expires_at = $3,
                           version = version + 1,
                           updated_at = CURRENT_TIMESTAMP
                       WHERE id = $1 AND task_id = $2 AND status = 'timeout_pending_approval'"#,
                )
                .bind(outreach_id)
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
            QuorumTimeoutAction::Reject => {
                let outreach_id = sqlx::query_scalar::<_, Uuid>(
                    r#"UPDATE task_outreaches SET status = 'cancelled', version = version + 1,
                           updated_at = CURRENT_TIMESTAMP
                       WHERE id = $1 AND task_id = $2 AND status = 'timeout_pending_approval'
                       RETURNING id"#,
                )
                .bind(outreach_id)
                .bind(task_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(AppError::from)?;
                // The questions this outreach had not yet sent are no longer wanted. Claimable
                // rows only: one already in flight is owned by a worker holding a live lease, and
                // writing past that fence would overwrite an outcome a provider had already given.
                if let Some(outreach_id) = outreach_id {
                    sqlx::query(
                        r#"UPDATE message_deliveries AS delivery
                          SET status = 'dead_letter', attempt_count = max_attempts,
                              last_error_class = 'superseded',
                              last_error_detail = 'The outreach this delivery belonged to was rejected',
                              updated_at = CURRENT_TIMESTAMP
                         FROM task_outreach_targets AS target
                        WHERE target.outreach_id = $1
                          AND target.delivery_id = delivery.id
                          AND delivery.status IN ('pending', 'retryable')"#,
                    )
                    .bind(outreach_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(AppError::from)?;
                    sqlx::query(
                        "UPDATE task_outreach_targets SET status = 'cancelled' WHERE outreach_id = $1 AND status = 'active'",
                    )
                    .bind(outreach_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(AppError::from)?;
                }
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
                outreach_id.is_some() && task.rows_affected() == 1
            }
        };
        if !task_updated {
            tx.rollback().await.map_err(AppError::from)?;
            return Err(AppError::Internal(
                "Outreach is no longer awaiting this timeout decision".into(),
            ));
        }
        let status = match action {
            QuorumTimeoutAction::Reject => ApprovalStatus::Rejected,
            QuorumTimeoutAction::ProceedPartial | QuorumTimeoutAction::Extend { .. } => {
                ApprovalStatus::Approved
            }
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
        super::task::resolve_harness_outreach_on(&mut tx, outreach_id).await?;
        transitions::decision_note(
            &mut tx,
            &approval,
            if action == QuorumTimeoutAction::Reject {
                ApprovalStatus::Rejected
            } else {
                ApprovalStatus::Approved
            },
        )
        .await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(Some(updated.try_into()?))
    }

    async fn expire_due_approvals(&self, limit: u32) -> AppResult<u32> {
        if !(1..=128).contains(&limit) {
            return Err(AppError::BadRequest(
                "Approval expiry batch must be 1–128".into(),
            ));
        }
        let tokens = sqlx::query_scalar::<_, Uuid>(
            r#"WITH due AS (
                   SELECT id FROM human_approvals
                   WHERE status = 'pending' AND expires_at < CURRENT_TIMESTAMP
                     AND (expiry_retry_at IS NULL OR expiry_retry_at <= CURRENT_TIMESTAMP)
                   ORDER BY expires_at, id FOR UPDATE SKIP LOCKED LIMIT $1
               ) UPDATE human_approvals AS approval
                 SET expiry_retry_at = CURRENT_TIMESTAMP + INTERVAL '5 minutes'
                 FROM due WHERE approval.id = due.id RETURNING approval.token"#,
        )
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        let mut settled = 0;
        for token in tokens {
            match self
                .transition_approval(token, ApprovalStatus::Expired, Utc::now())
                .await
            {
                Ok(Some(_)) => settled += 1,
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, "Approval expiry deferred after settlement failure")
                }
            }
        }
        Ok(settled)
    }

    async fn expire_pending_approval(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>> {
        let Ok(token) = Uuid::parse_str(token) else {
            return Ok(None);
        };
        self.transition_approval(token, ApprovalStatus::Expired, now)
            .await
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
    mod continuation_tests;
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::adapters::persistence::test_support::{DeliveryFixtureRequest, delivery_fixture};
    use crate::entities::approval::{ApprovalAction, ApprovalSubject, QUORUM_TIMEOUT_ACTION};
    use crate::entities::correlation::CorrelationId;
    use crate::entities::creation::CreationProvenance;
    use crate::entities::message::{MessageDirection, MessageRole};
    use crate::entities::task::{NewTask, TaskTransitionReason};
    use crate::entities::transport::DeliveryPurpose;
    use crate::task_queue::{CreateOutreachRequest, TaskPersistence};
    use crate::transport::NewDelivery;
    use crate::use_cases::thread::{MessageAuthorWrite, MessageWrite};
    use crate::use_cases::{
        agent::{AgentPersistence, AgentWrite},
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        thread::ThreadPersistence,
        user::UserPersistence,
    };

    async fn seed_channel_agent(
        persistence: &PostgresPersistence,
        company_id: Uuid,
        label: &str,
    ) -> Uuid {
        let suffix = Uuid::new_v4().simple().to_string();
        AgentPersistence::create(
            persistence,
            company_id,
            AgentWrite {
                name: format!("{label} agent"),
                slug: format!("{label}-agent-{suffix}"),
                created_by: Some(CreationProvenance::system()),
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
            },
        )
        .await
        .expect("test channel agent is created")
        .id
    }

    /// Parking a task is a leased write, and the two callers that do it are not equivalent.
    ///
    /// Regression for a guard that checked only `id`, `company_id` and `status`. Under it a run
    /// whose lease had already been reaped could still park the task, suspending work the run
    /// that now owns it was actively doing -- and the quorum-timeout sweep, which legitimately
    /// holds no lease, was what made that guard look sufficient.
    /// The request as a message in its thread, and the mail that carries it.
    ///
    /// Every approval writes these two rows now, in the transaction that parks the task -- so a
    /// fixture that skipped them would be exercising a path production does not have.
    async fn approval_notice(
        persistence: &PostgresPersistence,
        subject: &ApprovalSubject,
        step: &str,
    ) -> (MessageWrite, NewDelivery) {
        let notice = MessageWrite::internal(
            subject.thread_id,
            MessageAuthorWrite::Platform,
            "[APPROVAL REQUIRED] Deploy",
            "Please approve",
            MessageDirection::Outbound,
            MessageRole::System,
            subject.correlation_id,
        )
        .external_conversation()
        .with_entry_kind(crate::entities::message::ThreadEntryKind::SystemEvent);
        let queued = delivery_fixture(
            persistence,
            DeliveryFixtureRequest {
                recipient: subject.approver_email.as_str(),
                subject: "[APPROVAL REQUIRED] Deploy",
                body: "Please approve",
                purpose: DeliveryPurpose::Notification,
                ..DeliveryFixtureRequest::new(
                    subject.company_id,
                    subject.channel_id,
                    subject.thread_id,
                    step,
                )
            },
        )
        .await;
        let delivery = NewDelivery {
            message_id: notice.id,
            ..queued.delivery
        };
        (notice, delivery)
    }

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
            thread_id,
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
        let agent_id = seed_channel_agent(&persistence, company.id, "park").await;
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Park".into(),
                slug: "park".into(),
                agent_ids: Some(vec![agent_id]),
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
            let subject = ApprovalSubject {
                suspension,
                ..approval_subject(&company, &channel, thread.id, &email)
            };
            let (notice, delivery) = approval_notice(&persistence, &subject, step).await;
            persistence
                .create_approval(NewApproval {
                    invocation: None,
                    subject: &subject,
                    action: &deploy_action(step),
                    message: &notice,
                    delivery,
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
                Some(TaskSuspension::AlreadySuspended {
                    task_id: task.id,
                    ownership: task.ownership,
                }),
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

        // A sweep cannot replace a pending approval with an unrelated wait.
        // Quorum timeout transitions start from their own outreach wait.
        assert!(
            park(
                Some(TaskSuspension::AlreadySuspended {
                    task_id: task.id,
                    ownership: task.ownership,
                }),
                "sweep-step"
            )
            .await
            .is_err(),
            "an unrelated approval must not replace the current wait"
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
        let agent_id = seed_channel_agent(&persistence, company.id, "approval").await;
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Approval".into(),
                slug: "approval".into(),
                agent_ids: Some(vec![agent_id]),
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
        let subject = approval_subject(&company, &channel, thread.id, &email);
        let (notice, delivery) = approval_notice(&persistence, &subject, "deploy-step").await;
        let delivery_key = delivery.idempotency_key.clone();
        let (approval, created) = persistence
            .create_approval(NewApproval {
                invocation: None,
                subject: &subject,
                action: &deploy_action("deploy-step"),
                message: &notice,
                delivery,
                token,
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            })
            .await
            .unwrap();
        assert!(created);
        let owner_principal = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
        )
        .bind(company.id)
        .bind(owner.id)
        .fetch_one(&persistence.pool)
        .await
        .unwrap();
        assert_eq!(
            approval.approver_principal_id,
            Some(PrincipalId::new(owner_principal)),
            "a verified teammate address is snapshotted as the in-app approval assignee"
        );

        // Counted by key alone, with no `status` filter. What is under test is that creating an
        // approval queues exactly one notification; whether a worker has since claimed it is the
        // worker's business, and asserting it is still 'pending' would be asserting that no
        // unscoped claim ran in between — which is not a property this code has.
        let queued_notifications: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM message_deliveries WHERE idempotency_key = $1",
        )
        .bind(delivery_key.as_str())
        .fetch_one(&persistence.pool)
        .await
        .unwrap();
        assert_eq!(queued_notifications, 1);
        // And the request is in the thread, so the conversation shows that a human was asked.
        let noticed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM thread_messages WHERE thread_id = $1 AND message_id = $2",
        )
        .bind(thread.id)
        .bind(notice.id.as_uuid())
        .fetch_one(&persistence.pool)
        .await
        .unwrap();
        assert_eq!(noticed, 1);
        let reloaded = persistence
            .find_approval_by_step_key(company.id, channel.id, thread.id, "deploy-step")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.id, approval.id);
        assert_eq!(
            reloaded.approver_principal_id,
            approval.approver_principal_id
        );
        assert_eq!(
            persistence
                .get_approval_by_id(company.id, approval.id)
                .await
                .unwrap()
                .unwrap()
                .id,
            approval.id
        );
        assert!(
            persistence
                .get_approval_by_id(Uuid::new_v4(), approval.id)
                .await
                .unwrap()
                .is_none(),
            "an approval id from another company must not cross the tenant scope"
        );

        let now = chrono::Utc::now();
        let token_str = token.to_string();
        let (first, second) = tokio::join!(
            persistence.decide_approval_and_transition(&token_str, ApprovalStatus::Approved, now),
            persistence.decide_approval_and_transition(&token_str, ApprovalStatus::Approved, now)
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

    /// A rejected quorum timeout is an approval acting, and the ledger has to say so.
    ///
    /// It used to record `operator_stopped` against an `approval` actor kind -- a row that
    /// contradicted itself, and that the trigger's own reason-to-actor mapping disagreed with. The
    /// sibling arms are covered by the type: the match is exhaustive over
    /// [`QuorumTimeoutAction`], so there is no longer a `_` arm able to file an unrecognised verb
    /// as consent.
    #[tokio::test]
    async fn a_rejected_quorum_timeout_is_recorded_as_the_approval_that_rejected_it() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("quorum_owner_{suffix}");
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
                name: "Quorum Test".to_string(),
                slug: format!("quorum-test-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let agent_id = seed_channel_agent(&persistence, company.id, "quorum").await;
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Quorum".into(),
                slug: "quorum".into(),
                agent_ids: Some(vec![agent_id]),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = crate::entities::value_objects::EmailAddress::from(email.clone());
        let thread = persistence
            .create_thread(channel.id, "Quorum", std::slice::from_ref(&email_addr))
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread.id),
                "quorum_reject",
                serde_json::json!({}),
            ))
            .await
            .unwrap();

        // Park the task behind an outreach, then let that outreach time out unanswered -- the one
        // state a quorum-timeout decision is allowed to act on.
        let worker_id = Uuid::new_v4();
        assert!(
            persistence
                .claim_task(
                    task.id,
                    worker_id,
                    Utc::now() + chrono::Duration::minutes(5)
                )
                .await
                .unwrap()
        );
        let lease = crate::entities::task::TaskLeaseRef::of(
            &persistence.get_task_by_id(task.id).await.unwrap().unwrap(),
        )
        .unwrap();
        let outreach_id = Uuid::new_v4();
        persistence
            .create_outreach_and_pause(CreateOutreachRequest {
                invocation: None,
                correlation_id: task.correlation_id,
                id: outreach_id,
                lease,
                company_id: company.id,
                channel_id: channel.id,
                outreach_key: "quorum-reject".into(),
                required_threshold_percent: 100.0,
                expires_at: Utc::now() + chrono::Duration::hours(1),
                subject: "Question".into(),
                body: "Please respond".into(),
                targets: Vec::new(),
            })
            .await
            .unwrap();
        // The schema refuses an outreach that expires before it was created, so the whole row is
        // backdated rather than only its deadline.
        sqlx::query(
            "UPDATE task_outreaches
                SET created_at = CURRENT_TIMESTAMP - interval '2 hours',
                    expires_at = CURRENT_TIMESTAMP - interval '1 hour'
              WHERE id = $1",
        )
        .bind(outreach_id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            persistence
                .mark_outreach_timeout_pending(outreach_id)
                .await
                .unwrap()
        );

        let token = Uuid::new_v4();
        let quorum_subject = ApprovalSubject {
            suspension: Some(TaskSuspension::AlreadySuspended {
                task_id: task.id,
                ownership: task.ownership,
            }),
            ..approval_subject(&company, &channel, thread.id, &email)
        };
        let (notice, delivery) =
            approval_notice(&persistence, &quorum_subject, &format!("quorum-{suffix}")).await;
        let (approval, created) = persistence
            .create_approval(NewApproval {
                invocation: None,
                subject: &quorum_subject,
                action: &ApprovalAction {
                    step_key: format!("quorum-{suffix}"),
                    action_type: QUORUM_TIMEOUT_ACTION.to_string(),
                    title: "Outreach timed out".to_string(),
                    summary: "Received 1/4 responses.".to_string(),
                    payload: serde_json::json!({ "outreach_id": outreach_id }),
                },
                message: &notice,
                delivery,
                token,
                expires_at: Utc::now() + chrono::Duration::hours(1),
            })
            .await
            .unwrap();
        assert!(created);

        let consumed = persistence
            .consume_quorum_timeout_action(
                &token.to_string(),
                QuorumTimeoutAction::Reject,
                Utc::now(),
            )
            .await
            .unwrap()
            .expect("the pending quorum approval is the one consumed");
        assert_eq!(consumed.status, ApprovalStatus::Rejected);

        let events = persistence
            .list_task_status_events(company.id, task.correlation_id, None, 50)
            .await
            .unwrap();
        let rejection = events
            .iter()
            .find(|event| event.reason == TaskTransitionReason::ApprovalRejected)
            .expect("the rejection stopped the task");
        assert_eq!(
            rejection.actor_kind,
            crate::entities::task::TaskTransitionActorKind::Approval
        );
        assert_eq!(rejection.related_approval_id, Some(approval.id));
        assert_eq!(
            rejection.to_status,
            crate::entities::task::TaskStatus::Stopped
        );
        assert!(
            !events
                .iter()
                .any(|event| event.reason == TaskTransitionReason::OperatorStopped),
            "no operator was involved: {events:#?}"
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }
}
