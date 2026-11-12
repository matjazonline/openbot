use super::projection::{SourceLifecycle, SourceSnapshot};
use super::*;
use crate::entities::notification::NotificationTarget;

#[derive(sqlx::FromRow)]
struct ReviewerSource {
    decision_id: Uuid,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    run_id: Uuid,
    correlation_id: Uuid,
    principal_id: Uuid,
    open: bool,
    current_reviewer: bool,
}

pub(super) async fn source_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<Option<SourceSnapshot>> {
    let source:Option<ReviewerSource>=sqlx::query_as(
        "SELECT notice.decision_id,run.channel_id,run.thread_id,run.id AS run_id,run.correlation_id,notice.principal_id, \
         workflow_follow_is_open(notice.company_id,notice.decision_id) AS open, \
         EXISTS (SELECT 1 FROM workflow_decision_reviewers AS current \
             WHERE current.company_id=notice.company_id AND current.decision_id=notice.decision_id \
               AND current.principal_id=notice.principal_id AND current.notice_event_id=notice.event_id \
               AND current.notice_assignment_revision=notice.assignment_revision) AND workflow_follow_reviewer_eligible(notice.company_id,run.id,notice.principal_id) AS current_reviewer \
         FROM workflow_reviewer_notices AS notice \
         JOIN workflow_decisions AS decision ON decision.company_id=notice.company_id AND decision.id=notice.decision_id \
         JOIN workflow_runs AS run ON run.company_id=decision.company_id AND run.id=decision.run_id \
         WHERE notice.company_id=$1 AND notice.id=$2 AND notice.event_id=$3 AND notice.assignment_revision=$4")
        .bind(event.company_id).bind(event.source_id).bind(event.id.as_uuid())
        .bind(i64::try_from(event.source_generation).map_err(|_|AppError::BadRequest("Invalid notice generation".into()))?)
        .fetch_optional(db).await?;
    Ok(source.map(|source| SourceSnapshot {
        channel_id: source.channel_id,
        thread_id: source.thread_id,
        run_id: Some(source.run_id),
        correlation_id: Some(source.correlation_id),
        responsible_principal_id: Some(source.principal_id),
        target: NotificationTarget::WorkflowDecision {
            decision_id: source.decision_id,
        },
        lifecycle: if !source.open {
            SourceLifecycle::Resolved
        } else if !source.current_reviewer {
            SourceLifecycle::Withdrawn
        } else {
            SourceLifecycle::Active
        },
    }))
}

pub(super) async fn lock_source_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<()> {
    let run:Option<Uuid>=sqlx::query_scalar(
        "SELECT decision.run_id FROM workflow_reviewer_notices AS notice \
         JOIN workflow_decisions AS decision ON decision.company_id=notice.company_id AND decision.id=notice.decision_id \
         WHERE notice.company_id=$1 AND notice.id=$2")
        .bind(event.company_id).bind(event.source_id).fetch_optional(&mut *db).await?;
    if let Some(run) = run {
        sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(event.company_id)
            .bind(run)
            .execute(&mut *db)
            .await?;
        sqlx::query("SELECT decision.id FROM workflow_decisions AS decision JOIN workflow_reviewer_notices AS notice \
            ON notice.company_id=decision.company_id AND notice.decision_id=decision.id \
            WHERE notice.company_id=$1 AND notice.id=$2 FOR UPDATE OF decision")
            .bind(event.company_id).bind(event.source_id).execute(db).await?;
    }
    Ok(())
}

/// The first committed outcome is proof of delivery or a real opt-out, retained as history.
pub(super) async fn record_receipt_on(
    db: &mut PgConnection,
    projection: &NotificationProjection,
) -> AppResult<()> {
    if projection.event.source_kind != NotificationSourceKind::DecisionReviewer {
        return Ok(());
    }
    let outcome = match &projection.disposition {
        NotificationDisposition::Active(_) if projection.should_email() => "delivered",
        NotificationDisposition::Active(_) => "opted_out",
        NotificationDisposition::Resolved => "resolved",
        NotificationDisposition::Withdrawn => "withdrawn",
        NotificationDisposition::Control => return Ok(()),
    };
    sqlx::query("INSERT INTO workflow_reviewer_notice_receipts(company_id,notice_id,decision_id,event_id,outcome,email_delivery_id) \
        SELECT source.company_id,source.id,source.decision_id,source.event_id,$4,CASE WHEN $4='delivered' THEN notification.email_delivery_id ELSE NULL END \
        FROM workflow_reviewer_notices AS source LEFT JOIN notifications AS notification \
          ON notification.company_id=source.company_id AND notification.event_id=source.event_id \
        WHERE source.company_id=$1 AND source.id=$2 AND source.event_id=$3 ON CONFLICT(company_id,notice_id) DO NOTHING")
        .bind(projection.event.company_id).bind(projection.event.source_id).bind(projection.event.id.as_uuid()).bind(outcome).execute(db).await?;
    Ok(())
}
