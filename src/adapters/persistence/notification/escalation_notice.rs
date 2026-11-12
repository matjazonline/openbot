//! Escalation people use the ordinary notification projection; agents have a canonical route.
use super::projection::{SourceLifecycle, SourceSnapshot};
use super::*;
use crate::adapters::persistence::workflow::escalations::{
    CasePayloadAccess, case_scope_on, payload_access_on,
};
use crate::application::workflow::{CompanyId, WorkflowActor};
use crate::entities::notification::NotificationTarget;
use crate::entities::workflow_escalation::EscalationId;

#[path = "escalation_receipts.rs"]
mod receipts;
pub(super) use receipts::record_receipt_on;

#[derive(sqlx::FromRow)]
struct NoticeSource {
    case_id: Uuid,
    decision_id: Uuid,
    principal_id: Uuid,
    is_person: bool,
    is_agent: bool,
    user_id: Option<Uuid>,
    active: bool,
    resolved: bool,
    failed: bool,
}

async fn notice_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<Option<NoticeSource>> {
    Ok(sqlx::query_as("SELECT notice.case_id,notice.decision_id,notice.principal_id, \
        notice.principal_kind='person' AS is_person,notice.principal_kind='agent' AS is_agent,account.id AS user_id, \
        progress.state IN ('waiting','owner_waiting','manual') AND progress.current_decision_id=notice.decision_id \
          AND decision.settled_at IS NULL AS active,progress.state='settled' AS resolved, \
        EXISTS (SELECT 1 FROM workflow_escalation_recipient_failures AS failure WHERE failure.company_id=notice.company_id AND failure.notice_id=notice.id) AS failed \
        FROM workflow_escalation_notices AS notice \
        JOIN workflow_escalation_progress AS progress ON progress.company_id=notice.company_id AND progress.case_id=notice.case_id \
        JOIN workflow_escalation_decisions AS decision ON decision.company_id=notice.company_id AND decision.id=notice.decision_id \
        LEFT JOIN principals AS principal ON principal.company_id=notice.company_id AND principal.id=notice.principal_id AND principal.kind='person' \
        LEFT JOIN users AS account ON account.id=principal.user_id \
        WHERE notice.company_id=$1 AND notice.id=$2 AND notice.event_id=$3 AND notice.source_generation=$4")
        .bind(event.company_id).bind(event.source_id).bind(event.id.as_uuid())
        .bind(i64::try_from(event.source_generation).map_err(|_|AppError::BadRequest("Invalid escalation notice generation".into()))?)
        .fetch_optional(db).await?)
}

pub(super) async fn source_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<Option<SourceSnapshot>> {
    let Some(source) = notice_on(db, event).await? else {
        return Ok(None);
    };
    let Some(scope) = case_scope_on(
        db,
        CompanyId::new(event.company_id),
        EscalationId::new(source.case_id),
    )
    .await?
    else {
        return Ok(None);
    };
    let lifecycle = if source.resolved {
        SourceLifecycle::Resolved
    } else if !source.active || source.failed {
        SourceLifecycle::Withdrawn
    } else if source.is_agent {
        return Err(AppError::Internal(
            "Agent escalation notice requires canonical routing".into(),
        ));
    } else if !source.is_person {
        SourceLifecycle::Withdrawn
    } else if let Some(user) = source.user_id {
        match payload_access_on(db, &scope, WorkflowActor::authenticated(user)?).await? {
            CasePayloadAccess::Visible => SourceLifecycle::Active,
            CasePayloadAccess::Restricted | CasePayloadAccess::Unavailable => {
                SourceLifecycle::Withdrawn
            }
        }
    } else {
        SourceLifecycle::Withdrawn
    };
    Ok(Some(SourceSnapshot {
        channel_id: scope.channel_id,
        thread_id: scope.thread_id,
        run_id: scope.run_id,
        correlation_id: scope.correlation_id,
        responsible_principal_id: Some(source.principal_id),
        lifecycle,
        target: NotificationTarget::WorkflowEscalation {
            decision_id: source.decision_id,
        },
    }))
}

/// Assignment settlement owns company/case; stale queued email must stop in that transaction.
pub(crate) async fn close_assignment_on(
    db: &mut PgConnection,
    company: Uuid,
    decision: Uuid,
) -> AppResult<()> {
    #[derive(sqlx::FromRow)]
    struct Notice {
        id: Uuid,
        email_delivery_id: Option<Uuid>,
    }
    let notices: Vec<Notice> = sqlx::query_as("SELECT notification.id,notification.email_delivery_id \
        FROM workflow_escalation_notices AS source JOIN notifications AS notification \
          ON notification.company_id=source.company_id AND notification.event_id=source.event_id \
        WHERE source.company_id=$1 AND source.decision_id=$2 ORDER BY notification.id FOR UPDATE OF notification")
        .bind(company).bind(decision).fetch_all(&mut *db).await?;
    let identities: Vec<_> = notices.iter().map(|notice| notice.id).collect();
    let deliveries: Vec<_> = notices
        .iter()
        .filter_map(|notice| notice.email_delivery_id)
        .collect();
    sqlx::query("UPDATE notifications SET state='withdrawn',state_changed_at=clock_timestamp(),updated_at=clock_timestamp() \
        WHERE company_id=$1 AND id=ANY($2) AND state='active'")
        .bind(company).bind(identities).execute(&mut *db).await?;
    Box::pin(
        crate::adapters::persistence::delivery::cancellation::cancel_deliveries_on(
            db,
            company,
            &deliveries,
            crate::entities::transport::DeliveryCancellationReason::QuestionClosed,
        ),
    )
    .await?;
    Ok(())
}

/// The caller already owns applicable accounts and company. Run precedes case and assignment.
pub(super) async fn lock_source_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<()> {
    let Some(source) = notice_on(db, event).await? else {
        return Ok(());
    };
    sqlx::query(
        "SELECT run.id FROM workflow_runs AS run JOIN workflow_escalation_cases AS escalation \
        ON escalation.company_id=run.company_id AND escalation.run_id=run.id \
        WHERE escalation.company_id=$1 AND escalation.id=$2 FOR UPDATE OF run",
    )
    .bind(event.company_id)
    .bind(source.case_id)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "SELECT id FROM workflow_escalation_cases WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(event.company_id)
    .bind(source.case_id)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "SELECT id FROM workflow_escalation_decisions WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(event.company_id)
    .bind(source.decision_id)
    .execute(db)
    .await?;
    Ok(())
}
