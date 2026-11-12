//! Case creation participates in the authoritative source transaction; controls use the existing queue.
use super::*;
use crate::application::workflow::escalation::{NotificationRecipients, NotificationRoleRefusal};
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::entities::workflow_escalation::{AdmissionRefusalId, EscalationId};

#[path = "escalation_recipient_facts.rs"]
mod recipient_facts;
pub(super) use recipient_facts::{HopRecipients, facts_on};
#[path = "escalation_controls.rs"]
mod controls;
pub(crate) use controls::{ControlProjection, control_on};
#[path = "escalation_access.rs"]
mod access;
pub(crate) use access::{CasePayloadAccess, case_scope_on, payload_access_on};

pub(super) async fn recipients_on(
    db: &mut PgConnection,
    company: CompanyId,
    origin: Option<DepartmentId>,
    role: DepartmentRole,
) -> AppResult<NotificationRecipients> {
    let Some(origin) = origin else {
        return Ok(NotificationRecipients::Refused {
            department: None,
            reason: NotificationRoleRefusal::MissingContext,
        });
    };
    agent_manifest::notification_recipients_on(db, company, origin, role).await
}

pub(super) async fn admission_refused_on(
    db: &mut PgConnection,
    company: CompanyId,
    refusal: AdmissionRefusalId,
) -> AppResult<Option<EscalationId>> {
    let case: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO workflow_escalation_cases \
         (company_id,id,source_kind,admission_refusal_id,binding_id,binding_revision, \
          origin_department_id,policy,reason_code,step_id,role) \
         SELECT refusal.company_id,gen_random_uuid(),'admission',refusal.id, \
                refusal.binding_id,refusal.binding_revision,channel.department_id,configuration.escalation, \
                workflow_role_refusal_code(refusal.role_refusal),refusal.role_refusal->>'step', \
                refusal.role_refusal->>'role' \
         FROM workflow_admission_refusals AS refusal \
         JOIN workflow_binding_revisions AS configuration ON configuration.company_id=refusal.company_id \
           AND configuration.binding_id=refusal.binding_id AND configuration.revision=refusal.binding_revision \
         LEFT JOIN channels AS channel ON channel.company_id=refusal.company_id AND channel.id=refusal.channel_id \
         WHERE refusal.company_id=$1 AND refusal.id=$2 AND configuration.escalation IS NOT NULL \
         ON CONFLICT (company_id,admission_refusal_id) WHERE admission_refusal_id IS NOT NULL \
         DO NOTHING RETURNING id",
    )
    .bind(company.as_uuid())
    .bind(refusal.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    let case = match case {
        Some(id) => Some(id),
        None => sqlx::query_scalar(
            "SELECT id FROM workflow_escalation_cases WHERE company_id=$1 AND admission_refusal_id=$2",
        )
        .bind(company.as_uuid())
        .bind(refusal.as_uuid())
        .fetch_optional(&mut *db)
        .await?,
    };
    if let Some(case) = case {
        enqueue_control_on(db, company, EscalationId::new(case), 1, chrono::Utc::now()).await?;
    }
    Ok(case.map(EscalationId::new))
}

async fn enqueue_control_on(
    db: &mut PgConnection,
    company: CompanyId,
    case: EscalationId,
    phase: i16,
    available: chrono::DateTime<chrono::Utc>,
) -> AppResult<()> {
    sqlx::query(
        "WITH event AS ( \
           INSERT INTO notification_events(company_id,source_kind,source_id,action_kind,source_generation,available_at) \
           VALUES ($1,'escalation_control',$2,'escalation_control',$3,$4) \
           ON CONFLICT (company_id,source_kind,source_id,action_kind,source_generation) DO NOTHING \
           RETURNING id \
         ) INSERT INTO workflow_escalation_control_events(company_id,case_id,phase,event_id) \
           SELECT $1,$2,$3,event.id FROM event ON CONFLICT (company_id,case_id,phase) DO NOTHING",
    )
    .bind(company.as_uuid())
    .bind(case.as_uuid())
    .bind(i64::from(phase))
    .bind(available)
    .execute(db)
    .await?;
    Ok(())
}
