//! Final lifecycle state revokes capability; a repaired identity does not resend an old notice.
use super::*;
use crate::application::workflow::CompanyId;
use crate::entities::workflow_escalation::EscalationId;

pub(crate) async fn refresh_person_escalations_on(
    db: &mut PgConnection,
    company: CompanyId,
) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_escalation_recipient_failures(company_id,notice_id,failure_kind,reason_code) \
        SELECT notice.company_id,notice.id,CASE WHEN principal.kind='person' AND account.id IS NOT NULL AND member.user_id IS NOT NULL \
          THEN 'route' ELSE 'retired' END,CASE WHEN principal.kind='person' AND account.id IS NOT NULL AND member.user_id IS NOT NULL \
          THEN 'escalation.source_unavailable' ELSE 'escalation.recipient_retired' END \
        FROM workflow_escalation_progress AS progress \
        JOIN workflow_escalation_notices AS notice ON notice.company_id=progress.company_id AND notice.decision_id=progress.current_decision_id \
        LEFT JOIN principals AS principal ON principal.company_id=notice.company_id AND principal.id=notice.principal_id \
        LEFT JOIN users AS account ON account.id=principal.user_id \
        LEFT JOIN company_members AS member ON member.company_id=notice.company_id AND member.user_id=account.id \
        WHERE progress.company_id=$1 AND progress.state IN ('waiting','owner_waiting','manual') AND notice.principal_kind='person' \
          AND workflow_escalation_notice_active(notice.company_id,notice.id) \
          AND NOT workflow_escalation_person_eligible(notice.company_id,notice.id) \
        ON CONFLICT(company_id,notice_id,failure_kind) DO NOTHING")
        .bind(company.as_uuid()).execute(&mut *db).await?;
    sqlx::query("UPDATE notifications AS notification SET state='withdrawn',state_changed_at=clock_timestamp(),updated_at=clock_timestamp() \
        FROM workflow_escalation_notices AS notice JOIN workflow_escalation_progress AS progress \
          ON progress.company_id=notice.company_id AND progress.current_decision_id=notice.decision_id \
        WHERE notification.company_id=$1 AND notice.company_id=notification.company_id AND notice.event_id=notification.event_id \
          AND notification.state='active' AND progress.state IN ('waiting','owner_waiting','manual') \
          AND EXISTS (SELECT 1 FROM workflow_escalation_recipient_failures AS failure WHERE failure.company_id=notice.company_id AND failure.notice_id=notice.id)")
        .bind(company.as_uuid()).execute(&mut *db).await?;
    let deliveries: Vec<Uuid> = sqlx::query_scalar("SELECT receipt.email_delivery_id FROM workflow_escalation_progress AS progress \
        JOIN workflow_escalation_notices AS notice ON notice.company_id=progress.company_id AND notice.decision_id=progress.current_decision_id \
        JOIN workflow_escalation_person_receipts AS receipt ON receipt.company_id=notice.company_id AND receipt.notice_id=notice.id \
        JOIN message_deliveries AS delivery ON delivery.company_id=receipt.company_id AND delivery.id=receipt.email_delivery_id \
        WHERE progress.company_id=$1 AND progress.state IN ('waiting','owner_waiting','manual') AND delivery.cancellation_requested_at IS NULL \
          AND EXISTS (SELECT 1 FROM workflow_escalation_recipient_failures AS failure WHERE failure.company_id=notice.company_id AND failure.notice_id=notice.id) \
        ORDER BY delivery.id")
        .bind(company.as_uuid()).fetch_all(&mut *db).await?;
    Box::pin(
        crate::adapters::persistence::delivery::cancellation::cancel_deliveries_on(
            db,
            company.as_uuid(),
            &deliveries,
            crate::entities::transport::DeliveryCancellationReason::RecipientRevoked,
        ),
    )
    .await?;
    wake_failed_assignments_on(db, company, None).await
}

pub(super) async fn wake_failed_assignments_on(
    db: &mut PgConnection,
    company: CompanyId,
    case: Option<EscalationId>,
) -> AppResult<()> {
    sqlx::query("UPDATE notification_events AS event SET available_at=clock_timestamp() \
        FROM workflow_escalation_control_events AS control JOIN workflow_escalation_progress AS progress \
          ON progress.company_id=control.company_id AND progress.case_id=control.case_id \
        WHERE event.company_id=$1 AND ($2::uuid IS NULL OR control.case_id=$2) AND event.company_id=control.company_id AND event.id=control.event_id \
          AND event.status='pending' AND event.available_at>clock_timestamp() AND progress.state='waiting' \
          AND control.phase=CASE WHEN progress.current_hop=1 AND EXISTS (SELECT 1 FROM workflow_escalation_cases AS escalation \
            WHERE escalation.company_id=control.company_id AND escalation.id=control.case_id AND escalation.policy->>'hops'='2') THEN 2 ELSE 3 END \
          AND NOT EXISTS (SELECT 1 FROM workflow_escalation_notices AS notice \
            WHERE notice.company_id=progress.company_id AND notice.decision_id=progress.current_decision_id \
              AND NOT EXISTS (SELECT 1 FROM workflow_escalation_recipient_failures AS failure \
                WHERE failure.company_id=notice.company_id AND failure.notice_id=notice.id))")
        .bind(company.as_uuid()).bind(case.map(|case|case.as_uuid())).execute(db).await?;
    Ok(())
}
