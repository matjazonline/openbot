//! Case creation participates in the authoritative source transaction; controls use the existing queue.
use super::admission_binding::SourceKey;
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
#[path = "escalation_follow.rs"]
mod follow;
pub(super) use follow::assignment_changed_on;
#[path = "escalation_decisions.rs"]
mod decisions;
pub(super) use decisions::{
    CommandTarget, ReadPurpose, read_prelocked_on, submit_prelocked_on, target_hint_on,
};

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
    let source: Option<String> = sqlx::query_scalar(
        "SELECT source_key FROM workflow_admission_refusals WHERE company_id=$1 AND id=$2",
    )
    .bind(company.as_uuid())
    .bind(refusal.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    if let Some(message) = source.as_deref().and_then(SourceKey::message)
        && super::escalation_handling::has_message_on(db, company, message).await?
    {
        // The original intent owner records its handling refusal. Never create
        // a fresh escalation case while trying to deliver an escalation notice.
        return Ok(None);
    }
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

/// The authoritative terminal writer already owns company and run. A child leaves its failure
/// to parent propagation, where an authored handler can still consume it. Existing cases retain
/// their first cause, including a previously withdrawn empty-reviewer escalation.
pub(super) async fn run_failed_on(
    db: &mut PgConnection,
    scope: crate::application::workflow::activation::ActivationRequest,
    reason: &FailureCode,
) -> AppResult<()> {
    if super::escalation_handling::run_failed_on(db, scope).await? {
        return Ok(());
    }
    let suppressed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_run_notification_origins WHERE company_id=$1 AND run_id=$2)",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    if suppressed {
        return Ok(());
    }
    let case: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO workflow_escalation_cases \
         (company_id,id,source_kind,run_id,execution_id,binding_id,binding_revision, \
          origin_department_id,policy,reason_code,step_id) \
         SELECT run.company_id,gen_random_uuid(),'run',run.id,execution.id,run.binding_id,run.binding_revision, \
                manifest.role_context_department_id,run.escalation,$4,execution.step_id \
         FROM workflow_runs AS run \
         JOIN workflow_executions AS execution ON execution.company_id=run.company_id \
           AND execution.run_id=run.id AND execution.id=$3 \
         JOIN run_dependency_manifests AS ownership ON ownership.company_id=run.company_id AND ownership.run_id=run.id \
         JOIN agent_dependency_manifests AS manifest ON manifest.company_id=ownership.company_id AND manifest.id=ownership.manifest_id \
         WHERE run.company_id=$1 AND run.id=$2 AND run.state='failed' AND run.escalation IS NOT NULL \
           AND run.terminal_execution_id=execution.id AND run.parent_run_id IS NULL \
         ON CONFLICT (company_id,run_id) WHERE run_id IS NOT NULL DO NOTHING RETURNING id",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(reason.as_str())
    .fetch_optional(&mut *db)
    .await?;
    if let Some(case) = case {
        enqueue_control_on(
            db,
            scope.company,
            EscalationId::new(case),
            1,
            chrono::Utc::now(),
        )
        .await?;
    }
    Ok(())
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
