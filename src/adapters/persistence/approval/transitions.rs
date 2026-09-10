use super::*;
use crate::entities::correlation::CorrelationId;
use sqlx::{Postgres, Transaction};

type Tx<'a> = Transaction<'a, Postgres>;

#[derive(sqlx::FromRow)]
struct ExistingInvocationWait {
    run_id: Option<Uuid>,
    invocation_id: Option<Uuid>,
    checkpoint_revision: Option<i64>,
}

pub(super) async fn lock_subject(
    tx: &mut Tx<'_>,
    subject: &crate::entities::approval::ApprovalSubject,
) -> AppResult<()> {
    if let Some(suspension) = subject.suspension {
        let locked = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM background_tasks WHERE company_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(subject.company_id)
        .bind(suspension.task_id())
        .fetch_optional(&mut **tx)
        .await?;
        if locked.is_none() {
            return Err(AppError::NotFound("Approval task".into()));
        }
    }
    Ok(())
}

pub(super) async fn link_wait(
    tx: &mut Tx<'_>,
    subject: &crate::entities::approval::ApprovalSubject,
    approval: &HumanApprovalDb,
    invocation: Option<crate::services::harness::runs::InvocationRef>,
) -> AppResult<()> {
    let Some(suspension) = subject.suspension else {
        return Ok(());
    };
    let ownership = suspension.ownership();
    if let Some(reference) = invocation {
        let existing = sqlx::query_as::<_, ExistingInvocationWait>(
            r#"SELECT wait.run_id, wait.invocation_id, wait.checkpoint_revision
                FROM task_approval_waits wait JOIN background_tasks task ON task.id = wait.task_id AND task.company_id = wait.company_id
                WHERE wait.company_id = $1 AND wait.task_id = $2 AND wait.approval_id = $3 AND wait.state = 'waiting'
                  AND task.status = 'pending_approval' AND wait.ownership_version = task.ownership_version
                  AND wait.owner_principal_id IS NOT DISTINCT FROM task.owner_principal_id"#,
        ).bind(subject.company_id).bind(suspension.task_id()).bind(approval.id).fetch_optional(&mut **tx).await?;
        if let Some(existing) = existing {
            if existing.run_id != Some(reference.run_id.0)
                || existing.invocation_id != Some(reference.invocation_id.0)
                || existing.checkpoint_revision
                    != reference
                        .expected_revision
                        .0
                        .checked_add(1)
                        .and_then(|n| i64::try_from(n).ok())
            {
                return Err(AppError::Conflict(
                    "Approval is linked to another invocation or revision".into(),
                ));
            }
            return Ok(());
        }
    }
    let inserted = sqlx::query(
        r#"INSERT INTO task_approval_waits (task_id, company_id, approval_id, cycle_id, owner_principal_id, ownership_version, state)
           VALUES ($1,$2,$3,$4,$5,$6,'waiting')
           ON CONFLICT (task_id) DO UPDATE SET approval_id = EXCLUDED.approval_id,
             cycle_id = CASE WHEN task_approval_waits.state = 'waiting' THEN task_approval_waits.cycle_id ELSE EXCLUDED.cycle_id END, owner_principal_id = EXCLUDED.owner_principal_id,
             ownership_version = EXCLUDED.ownership_version, state = 'waiting',
             run_id = CASE WHEN task_approval_waits.state = 'waiting' THEN task_approval_waits.run_id ELSE NULL END,
             invocation_id = CASE WHEN task_approval_waits.state = 'waiting' THEN task_approval_waits.invocation_id ELSE NULL END,
             checkpoint_revision = CASE WHEN task_approval_waits.state = 'waiting' THEN task_approval_waits.checkpoint_revision ELSE NULL END
           WHERE task_approval_waits.state <> 'waiting'
              OR (task_approval_waits.approval_id = EXCLUDED.approval_id
                  AND task_approval_waits.ownership_version = EXCLUDED.ownership_version
                  AND task_approval_waits.owner_principal_id IS NOT DISTINCT FROM EXCLUDED.owner_principal_id)"#,
    ).bind(suspension.task_id()).bind(subject.company_id).bind(approval.id).bind(Uuid::new_v4())
    .bind(ownership.owner.principal_id().map(|id| id.as_uuid()))
    .bind(i64::try_from(ownership.version).map_err(|_| AppError::Conflict("Ownership version exhausted".into()))?)
    .execute(&mut **tx).await?;
    if inserted.rows_affected() != 1 {
        return Err(AppError::Conflict(
            "Task is awaiting another approval".into(),
        ));
    }
    if let Some(invocation) = invocation {
        let lease = suspension
            .lease()
            .ok_or_else(|| AppError::BadRequest("Rig approval requires a worker lease".into()))?;
        let cycle_id = sqlx::query_scalar::<_, Uuid>("SELECT cycle_id FROM task_approval_waits WHERE company_id = $1 AND task_id = $2 AND approval_id = $3")
            .bind(subject.company_id).bind(suspension.task_id()).bind(approval.id).fetch_one(&mut **tx).await?;
        let revision = super::super::task::park_harness_approval_on(
            tx,
            &crate::services::harness::runs::RunWrite {
                company_id: subject.company_id,
                run_id: invocation.run_id,
                lease,
                expected_revision: invocation.expected_revision,
            },
            crate::services::harness::runs::ApprovalWait {
                approval_id: approval.id,
                cycle_id,
                invocation_id: invocation.invocation_id,
            },
        )
        .await?;
        sqlx::query("UPDATE task_approval_waits SET run_id = $3, invocation_id = $4, checkpoint_revision = $5 WHERE company_id = $1 AND task_id = $2")
            .bind(subject.company_id).bind(suspension.task_id()).bind(invocation.run_id.0).bind(invocation.invocation_id.0)
            .bind(revision.0 as i64).execute(&mut **tx).await?;
    }
    Ok(())
}

impl PostgresPersistence {
    pub(super) async fn transition_approval(
        &self,
        token: Uuid,
        decision: ApprovalStatus,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HumanApproval>> {
        if !matches!(
            decision,
            ApprovalStatus::Approved | ApprovalStatus::Rejected | ApprovalStatus::Expired
        ) {
            return Err(AppError::BadRequest("Invalid approval decision".into()));
        }
        let mut tx = self.pool.begin().await?;
        let candidate = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            "SELECT {APPROVAL_COLUMNS} FROM human_approvals WHERE token = $1 AND status = 'pending'",
        )).bind(token).fetch_optional(&mut *tx).await?;
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        // Outreach transactions acquire their outreach lock before the task lock.
        if candidate.action_type == "quorum_timeout" {
            if decision != ApprovalStatus::Expired {
                return Err(AppError::BadRequest(
                    "Use the quorum decision operation".into(),
                ));
            }
            lock_outreach(&mut tx, &candidate).await?;
        }
        if let Some(task_id) = candidate.task_id {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM background_tasks WHERE company_id = $1 AND id = $2 FOR UPDATE",
            )
            .bind(candidate.company_id)
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
        }
        let approval = sqlx::query_as::<_, HumanApprovalDb>(&format!(
            r#"UPDATE human_approvals SET status = $2, updated_at = CURRENT_TIMESTAMP
               WHERE token = $1 AND status = 'pending'
                 AND (($2 = 'expired' AND expires_at < $3) OR ($2 <> 'expired' AND expires_at >= $3))
               RETURNING {APPROVAL_COLUMNS}"#,
        )).bind(token).bind(decision.as_str()).bind(now).fetch_optional(&mut *tx).await?;
        let Some(approval) = approval else {
            return Ok(None);
        };
        settle_task(&mut tx, &approval, decision.clone()).await?;
        decision_note(&mut tx, &approval, decision).await?;
        tx.commit().await?;
        Ok(Some(approval.try_into()?))
    }
}

async fn lock_outreach(tx: &mut Tx<'_>, approval: &HumanApprovalDb) -> AppResult<()> {
    let outreach_id = approval
        .payload
        .get("outreach_id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| {
            AppError::BadRequest("Quorum approval has no valid outreach identity".into())
        })?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM task_outreaches WHERE id = $1 AND task_id = $2 FOR UPDATE",
    )
    .bind(outreach_id)
    .bind(approval.task_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(())
}

async fn settle_task(
    tx: &mut Tx<'_>,
    approval: &HumanApprovalDb,
    decision: ApprovalStatus,
) -> AppResult<()> {
    let Some(task_id) = approval.task_id else {
        return Ok(());
    };
    let approved = decision == ApprovalStatus::Approved;
    let reason = match decision {
        ApprovalStatus::Approved => TaskTransitionReason::ApprovalAccepted,
        ApprovalStatus::Rejected => TaskTransitionReason::ApprovalRejected,
        _ => TaskTransitionReason::TimedOut,
    };
    let attribution = TransitionAttribution::new(reason, TransitionActor::Approval(approval.id));
    let updated = sqlx::query(&format!(
        r#"UPDATE background_tasks AS task SET status = $4, run_at = CURRENT_TIMESTAMP,
               wait_expires_at = NULL, last_error = $5, updated_at = CURRENT_TIMESTAMP, {attribution}
           FROM task_approval_waits AS wait
           WHERE task.company_id = $1 AND task.id = $2 AND task.status = 'pending_approval'
             AND wait.company_id = task.company_id AND wait.task_id = task.id
             AND wait.approval_id = $3 AND wait.state = 'waiting'
             AND wait.owner_principal_id IS NOT DISTINCT FROM task.owner_principal_id
             AND wait.ownership_version = task.ownership_version"#,
        attribution = attribution.set_clause(),
    )).bind(approval.company_id).bind(task_id).bind(approval.id)
    .bind(if approved { "pending" } else { "stopped" })
    .bind(match decision { ApprovalStatus::Expired => Some("Human approval expired"), ApprovalStatus::Rejected => Some("Human approval rejected"), _ => None })
    .execute(&mut **tx).await?;
    if updated.rows_affected() == 1 {
        super::super::task::resolve_harness_approval_on(
            tx,
            approval.company_id,
            task_id,
            approval.id,
            approved,
        )
        .await?;
    }
    if updated.rows_affected() == 1 && approval.action_type == "quorum_timeout" && !approved {
        cancel_outreach(tx, approval).await?;
    }
    sqlx::query("UPDATE task_approval_waits SET state = $4 WHERE company_id = $1 AND task_id = $2 AND approval_id = $3 AND state = 'waiting'")
        .bind(approval.company_id).bind(task_id).bind(approval.id).bind(decision.as_str()).execute(&mut **tx).await?;
    Ok(())
}

async fn cancel_outreach(tx: &mut Tx<'_>, approval: &HumanApprovalDb) -> AppResult<()> {
    let id = approval
        .payload
        .get("outreach_id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| AppError::BadRequest("Invalid quorum approval".into()))?;
    let cancelled = sqlx::query("UPDATE task_outreaches SET status = 'cancelled', version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND task_id = $2 AND status = 'timeout_pending_approval'")
        .bind(id).bind(approval.task_id).execute(&mut **tx).await?;
    if cancelled.rows_affected() == 1 {
        super::super::task::cancel_unsent_outreach_questions(tx, id).await?;
        super::super::task::resolve_harness_outreach_on(tx, id).await?;
        sqlx::query("UPDATE task_outreach_targets SET status = 'expired' WHERE outreach_id = $1 AND status = 'active'")
            .bind(id).execute(&mut **tx).await?;
    }
    Ok(())
}

pub(super) async fn decision_note(
    tx: &mut Tx<'_>,
    approval: &HumanApprovalDb,
    decision: ApprovalStatus,
) -> AppResult<()> {
    let correlation = if let Some(task_id) = approval.task_id {
        let id = sqlx::query_scalar::<_, Uuid>(
            "SELECT correlation_id FROM background_tasks WHERE company_id = $1 AND id = $2",
        )
        .bind(approval.company_id)
        .bind(task_id)
        .fetch_one(&mut **tx)
        .await?;
        CorrelationId::from(id)
    } else {
        CorrelationId::new()
    };
    let note = crate::use_cases::approval::decision_note(
        &HumanApproval {
            status: decision,
            ..approval.clone().try_into()?
        },
        correlation,
    );
    insert_message_on(tx, &note).await?;
    Ok(())
}
