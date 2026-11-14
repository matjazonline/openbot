//! Leased notification controls advance a finite ladder; they never project a user notification.
use super::*;
use crate::application::workflow::binding::{EscalationAckHours, EscalationPolicy};
use crate::application::workflow::escalation::{
    EscalationHop, LadderAction, LadderState, RecipientAvailability, ladder_action,
};
use crate::entities::notification::NotificationEvent;
use chrono::{DateTime, Utc};

#[path = "escalation_hops.rs"]
mod hops;
pub(super) use hops::enqueue_notices_on;

pub(crate) enum ControlProjection {
    Complete,
    Deferred(DateTime<Utc>),
}

#[derive(sqlx::FromRow)]
struct ControlSource {
    case_id: Uuid,
    run_id: Option<Uuid>,
    origin_department_id: Option<Uuid>,
    reason_code: String,
    policy: serde_json::Value,
    state: String,
    current_hop: i16,
    current_decision_id: Option<Uuid>,
    ack_hours: Option<i16>,
    deadline: Option<DateTime<Utc>>,
    capable: bool,
    now: DateTime<Utc>,
}

impl ControlSource {
    fn policy(&self) -> AppResult<EscalationPolicy> {
        serde_json::from_value(self.policy.clone()).map_err(|_| invalid())
    }
    fn state(&self) -> AppResult<LadderState> {
        Ok(match self.state.as_str() {
            "pending" => LadderState::Pending,
            "waiting" => LadderState::Waiting {
                hop: EscalationHop::try_from(
                    u8::try_from(self.current_hop).map_err(|_| invalid())?,
                )
                .map_err(|_| invalid())?,
                deadline: self.deadline.ok_or_else(invalid)?,
                recipients: if self.capable {
                    RecipientAvailability::Capable
                } else {
                    RecipientAvailability::Unavailable
                },
            },
            "owner_waiting" => LadderState::OwnerWaiting,
            "manual" | "settled" | "withdrawn" => LadderState::Settled,
            _ => return Err(invalid()),
        })
    }
}

pub(crate) async fn control_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
    configured_hours: EscalationAckHours,
) -> AppResult<ControlProjection> {
    let Some(source) = source_on(db, event).await? else {
        return Ok(ControlProjection::Complete);
    };
    let policy = source.policy()?;
    let action = ladder_action(policy, source.state()?, source.now);
    let expected = match action {
        LadderAction::Start => 1,
        LadderAction::Advance => 2,
        LadderAction::WaitUntil(_) | LadderAction::OwnerAttention => {
            if source.current_hop == 1 && policy.hops.get() == 2 {
                2
            } else {
                3
            }
        }
        LadderAction::Stop => return Ok(ControlProjection::Complete),
    };
    if event.source_generation < expected {
        return Ok(ControlProjection::Complete);
    }
    if event.source_generation != expected {
        return Err(invalid());
    }
    match action {
        LadderAction::Start => hops::create_on(db, event, &source, configured_hours).await?,
        LadderAction::Advance => {
            sqlx::query("UPDATE workflow_escalation_decisions SET settled_at=clock_timestamp(),settlement='advanced' \
                WHERE company_id=$1 AND id=$2 AND settled_at IS NULL")
                .bind(event.company_id).bind(source.current_decision_id).execute(&mut *db).await?;
            if let Some(decision) = source.current_decision_id {
                crate::adapters::persistence::notification::close_escalation_assignment_on(
                    db,
                    event.company_id,
                    decision,
                )
                .await?;
            }
            hops::create_on(db, event, &source, configured_hours).await?;
        }
        LadderAction::OwnerAttention => {
            sqlx::query("UPDATE workflow_escalation_progress SET state='owner_waiting',updated_at=clock_timestamp() WHERE company_id=$1 AND case_id=$2")
                .bind(event.company_id).bind(source.case_id).execute(db).await?;
        }
        LadderAction::WaitUntil(deadline) => return Ok(ControlProjection::Deferred(deadline)),
        LadderAction::Stop => {}
    }
    Ok(ControlProjection::Complete)
}

async fn source_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<Option<ControlSource>> {
    sqlx::query_as::<_,ControlSource>("SELECT escalation.id AS case_id,escalation.run_id,escalation.origin_department_id, \
        escalation.reason_code,escalation.policy,progress.state,progress.current_hop,progress.current_decision_id,progress.ack_hours, \
        hop.deadline,EXISTS (SELECT 1 FROM workflow_escalation_notices AS notice WHERE notice.company_id=escalation.company_id \
          AND notice.decision_id=progress.current_decision_id AND NOT EXISTS (SELECT 1 FROM workflow_escalation_recipient_failures AS failure \
            WHERE failure.company_id=notice.company_id AND failure.notice_id=notice.id)) AS capable,clock_timestamp() AS now \
        FROM workflow_escalation_cases AS escalation JOIN workflow_escalation_progress AS progress \
          ON progress.company_id=escalation.company_id AND progress.case_id=escalation.id \
        JOIN workflow_escalation_control_events AS control ON control.company_id=escalation.company_id AND control.case_id=escalation.id \
        LEFT JOIN workflow_run_escalations AS hop ON hop.company_id=progress.company_id AND hop.case_id=progress.case_id AND hop.hop=progress.current_hop \
        WHERE escalation.company_id=$1 AND escalation.id=$2 AND control.event_id=$3 AND control.phase=$4 \
        FOR UPDATE OF progress")
        .bind(event.company_id).bind(event.source_id).bind(event.id.as_uuid())
        .bind(i64::try_from(event.source_generation).map_err(|_|invalid())?).fetch_optional(db).await.map_err(AppError::from)
}
