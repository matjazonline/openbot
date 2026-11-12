//! Discovery is lock-free; mutation owns accounts, company, source, then the event fence.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProjectionPurpose {
    Project,
    Open { user_id: Uuid },
}

pub(super) enum LockedProjection {
    Current(Box<NotificationProjection>),
    Stale,
}

pub(super) async fn lock_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
    purpose: ProjectionPurpose,
) -> AppResult<LockedProjection> {
    let preview = resolve_projection_on(db, event).await?;
    let mut accounts = Vec::with_capacity(2);
    if let ProjectionPurpose::Open { user_id } = purpose {
        accounts.push(user_id);
    }
    if let NotificationDisposition::Active(recipient) = &preview.disposition {
        accounts.push(recipient.user_id);
    }
    accounts.sort_unstable();
    accounts.dedup();
    let locked: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR SHARE")
            .bind(&accounts)
            .fetch_all(&mut *db)
            .await?;
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(event.company_id)
        .execute(&mut *db)
        .await?;
    if locked != accounts {
        return Ok(LockedProjection::Stale);
    }
    match event.source_kind {
        NotificationSourceKind::Delivery => {
            sqlx::query(
                "SELECT id FROM message_deliveries WHERE company_id=$1 AND id=$2 FOR SHARE",
            )
            .bind(event.company_id)
            .bind(event.source_id)
            .execute(&mut *db)
            .await?;
        }
        NotificationSourceKind::DecisionReviewer => {
            super::reviewer_notice::lock_source_on(db, event).await?
        }
        NotificationSourceKind::EscalationControl => {
            sqlx::query(
                "SELECT id FROM workflow_escalation_cases WHERE company_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(event.company_id)
            .bind(event.source_id)
            .execute(&mut *db)
            .await?;
        }
        NotificationSourceKind::EscalationNotice => {
            super::escalation_notice::lock_source_on(db, event).await?
        }
    }
    sqlx::query("SELECT user_id FROM user_notification_preferences WHERE user_id=ANY($1) ORDER BY user_id FOR SHARE")
        .bind(&accounts).execute(&mut *db).await?;
    let current = resolve_projection_on(db, event).await?;
    if current != preview {
        return Ok(LockedProjection::Stale);
    }
    Ok(LockedProjection::Current(Box::new(current)))
}
