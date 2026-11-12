//! First projection outcomes are immutable; a later revocation remains a separate failure fact.
use super::*;

pub(in crate::adapters::persistence::notification) async fn record_receipt_on(
    db: &mut PgConnection,
    projection: &NotificationProjection,
) -> AppResult<()> {
    if projection.event.source_kind != NotificationSourceKind::EscalationNotice {
        return Ok(());
    }
    let event = &projection.event;
    let Some(source) = notice_on(db, event).await? else {
        return Ok(());
    };
    if !matches!(projection.disposition, NotificationDisposition::Active(_)) {
        cancel_email_on(db, event).await?;
    }
    let outcome = match &projection.disposition {
        NotificationDisposition::Active(_) if projection.should_email() => "email_queued",
        NotificationDisposition::Active(recipient)
            if event.actor_principal_id == Some(recipient.principal_id) =>
        {
            "self_suppressed"
        }
        NotificationDisposition::Active(_) => "opted_out",
        NotificationDisposition::Resolved => "resolved",
        NotificationDisposition::Withdrawn if source.active => {
            if !source.failed {
                record_failure_on(db, event, &source).await?;
            }
            "failed"
        }
        NotificationDisposition::Withdrawn => "withdrawn",
        NotificationDisposition::Control => {
            return Err(AppError::Internal(
                "An escalation notice cannot be a control".into(),
            ));
        }
    };
    if !source.is_person {
        return Ok(());
    }
    sqlx::query("INSERT INTO workflow_escalation_person_receipts(company_id,notice_id,event_id,outcome,email_delivery_id) \
        SELECT notice.company_id,notice.id,notice.event_id,$4,CASE WHEN $4='email_queued' THEN notification.email_delivery_id ELSE NULL END \
        FROM workflow_escalation_notices AS notice LEFT JOIN notifications AS notification \
          ON notification.company_id=notice.company_id AND notification.event_id=notice.event_id \
        WHERE notice.company_id=$1 AND notice.id=$2 AND notice.event_id=$3 ON CONFLICT(company_id,notice_id) DO NOTHING")
        .bind(event.company_id).bind(event.source_id).bind(event.id.as_uuid()).bind(outcome).execute(db).await?;
    Ok(())
}

async fn cancel_email_on(db: &mut PgConnection, event: &NotificationEvent) -> AppResult<()> {
    let deliveries: Vec<Uuid> = sqlx::query_scalar("SELECT email_delivery_id FROM notifications \
        WHERE company_id=$1 AND event_id=$2 AND email_delivery_id IS NOT NULL ORDER BY id FOR UPDATE")
        .bind(event.company_id).bind(event.id.as_uuid()).fetch_all(&mut *db).await?;
    Box::pin(
        crate::adapters::persistence::delivery::cancellation::cancel_deliveries_on(
            db,
            event.company_id,
            &deliveries,
            crate::entities::transport::DeliveryCancellationReason::RecipientRevoked,
        ),
    )
    .await?;
    Ok(())
}

async fn record_failure_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
    source: &NoticeSource,
) -> AppResult<()> {
    let failure = if !source.is_person {
        RecipientFailure::Kind
    } else if source.user_id.is_none() {
        RecipientFailure::Retired
    } else {
        RecipientFailure::Source
    };
    sqlx::query("INSERT INTO workflow_escalation_recipient_failures(company_id,notice_id,failure_kind,reason_code) \
        VALUES ($1,$2,$3,$4) ON CONFLICT(company_id,notice_id,failure_kind) DO NOTHING")
        .bind(event.company_id).bind(event.source_id).bind(failure.kind()).bind(failure.code()).execute(&mut *db).await?;
    super::super::escalation_failures::wake_failed_assignments_on(
        db,
        CompanyId::new(event.company_id),
        Some(EscalationId::new(source.case_id)),
    )
    .await
}

enum RecipientFailure {
    Kind,
    Retired,
    Source,
}
impl RecipientFailure {
    fn kind(&self) -> &'static str {
        match self {
            Self::Retired => "retired",
            Self::Kind | Self::Source => "route",
        }
    }
    fn code(&self) -> &'static str {
        match self {
            Self::Kind => "escalation.recipient_kind",
            Self::Retired => "escalation.recipient_retired",
            Self::Source => "escalation.source_unavailable",
        }
    }
}
