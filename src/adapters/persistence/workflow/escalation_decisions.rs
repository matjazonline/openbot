//! Administrative decisions use current operator authority, independent of historical run actors.
use super::*;
use crate::application::workflow::escalation::{
    EscalationChoice, EscalationDecisionId, EscalationHop, EscalationReason,
    EscalationRecipientState, EscalationRecipientView, EscalationSourceView, EscalationSubmission,
    EscalationView, SubmitEscalation, WorkflowEscalations,
};
use chrono::{DateTime, Utc};

#[path = "escalation_assignment.rs"]
mod assignment;
#[path = "escalation_commands.rs"]
mod commands;

pub(in crate::adapters::persistence::workflow) use assignment::Target as CommandTarget;
pub(in crate::adapters::persistence::workflow) use commands::{
    apply_on as submit_prelocked_on, target_hint_on,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::adapters::persistence::workflow) enum ReadPurpose {
    Interactive,
    Tool,
}

#[derive(sqlx::FromRow)]
struct Decision {
    case_id: Uuid,
    run_id: Option<Uuid>,
    binding_id: Uuid,
    reason_code: String,
    step_id: Option<String>,
    role: Option<String>,
    assignment_hop: i16,
    current_decision_id: Option<Uuid>,
    state: String,
    manual_assignments: i16,
    created_at: DateTime<Utc>,
    deadline: Option<DateTime<Utc>>,
    settlement: Option<String>,
    manual_assignment: bool,
    settled_at: Option<DateTime<Utc>>,
}

async fn decision_on(
    db: &mut PgConnection,
    company: CompanyId,
    decision: EscalationDecisionId,
) -> AppResult<Decision> {
    sqlx::query_as("SELECT assignment.case_id,escalation.run_id,escalation.binding_id, \
        escalation.reason_code,escalation.step_id,escalation.role, \
        progress.current_decision_id,progress.state,progress.manual_assignments,assignment.created_at, \
        COALESCE(assignment.hop,progress.current_hop) AS assignment_hop, \
        assignment.deadline,assignment.settled_at,assignment.settlement,assignment.assignment_kind='manual' AS manual_assignment \
        FROM workflow_escalation_decisions AS assignment \
        JOIN workflow_escalation_cases AS escalation ON escalation.company_id=assignment.company_id AND escalation.id=assignment.case_id \
        JOIN workflow_escalation_progress AS progress ON progress.company_id=assignment.company_id AND progress.case_id=assignment.case_id \
        JOIN workflow_run_escalations AS hop ON hop.company_id=assignment.company_id AND hop.case_id=assignment.case_id \
          AND hop.hop=COALESCE(assignment.hop,progress.current_hop) \
        WHERE assignment.company_id=$1 AND assignment.id=$2")
        .bind(company.as_uuid()).bind(decision.as_uuid()).fetch_optional(db).await?.ok_or_else(missing)
}

async fn authorize_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    source: &Decision,
) -> AppResult<authority::ReviewerAuthority> {
    let (viewer, access) = scope_access_on(db, company, actor, source).await?;
    if access != CasePayloadAccess::Visible {
        return Err(missing());
    }
    Ok(viewer)
}

async fn scope_access_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    source: &Decision,
) -> AppResult<(authority::ReviewerAuthority, CasePayloadAccess)> {
    let viewer = authority::authorize_reviewer_on(db, company, actor).await?;
    let scope = case_scope_on(db, company, EscalationId::new(source.case_id))
        .await?
        .ok_or_else(missing)?;
    let association = match (scope.channel_id, scope.thread_id) {
        (Some(channel), Some(thread)) => RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(channel),
            thread_id: RelatedThreadId::new(thread),
        },
        (Some(channel), None) => RelatedAssociation::Channel(RelatedChannelId::new(channel)),
        (None, _) if viewer.privileged => RelatedAssociation::Company,
        _ => return Err(missing()),
    };
    association::authorize_association(db, company, actor, association).await?;
    let access = payload_access_on(db, &scope, actor).await?;
    Ok((viewer, access))
}

async fn assigned_on(
    db: &mut PgConnection,
    command: &SubmitEscalation,
    viewer: &authority::ReviewerAuthority,
) -> AppResult<bool> {
    if viewer.privileged {
        return Ok(true);
    }
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM workflow_escalation_notices \
        WHERE company_id=$1 AND decision_id=$2 AND principal_id=$3 AND principal_kind='person')",
    )
    .bind(command.company.as_uuid())
    .bind(command.decision.as_uuid())
    .bind(viewer.principal)
    .fetch_one(db)
    .await?)
}

#[async_trait]
impl WorkflowEscalations for PostgresPersistence {
    async fn read_escalation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        decision: EscalationDecisionId,
    ) -> AppResult<EscalationView> {
        let mut tx = self.pool.begin().await?;
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE id=$1 FOR SHARE")
            .bind(actor.user_id())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM companies WHERE id=$1 FOR SHARE")
            .bind(company.as_uuid())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
        let view =
            read_prelocked_on(&mut tx, company, actor, decision, ReadPurpose::Interactive).await?;
        tx.commit().await?;
        Ok(view)
    }

    async fn submit_escalation(
        &self,
        command: SubmitEscalation,
    ) -> AppResult<EscalationSubmission> {
        // The transactional command driver is a substantive persistence seam; keep its SQL
        // future off callers' native stacks.
        Box::pin(commands::submit(self, command)).await
    }
}

#[derive(sqlx::FromRow)]
struct Recipient {
    principal_id: Uuid,
    principal_label: String,
    state: String,
}

async fn recipients_on(
    db: &mut PgConnection,
    company: CompanyId,
    decision: EscalationDecisionId,
) -> AppResult<Vec<EscalationRecipientView>> {
    let rows: Vec<Recipient> = sqlx::query_as("SELECT notice.principal_id,notice.principal_label, \
        CASE WHEN assignment.settled_at IS NOT NULL OR progress.current_decision_id<>notice.decision_id \
          OR progress.state IN ('settled','withdrawn') THEN 'withdrawn' \
        WHEN EXISTS (SELECT 1 FROM workflow_escalation_recipient_failures AS failure \
          WHERE failure.company_id=notice.company_id AND failure.notice_id=notice.id) THEN 'failed' \
        WHEN EXISTS (SELECT 1 FROM workflow_escalation_notice_targets AS receipt \
          WHERE receipt.company_id=notice.company_id AND receipt.notice_id=notice.id) \
          OR EXISTS (SELECT 1 FROM workflow_escalation_person_receipts AS receipt \
          WHERE receipt.company_id=notice.company_id AND receipt.notice_id=notice.id AND receipt.outcome IN ('email_queued','opted_out')) \
          THEN 'delivered' ELSE 'pending' END AS state \
        FROM workflow_escalation_notices AS notice \
        JOIN workflow_escalation_decisions AS assignment ON assignment.company_id=notice.company_id AND assignment.id=notice.decision_id \
        JOIN workflow_escalation_progress AS progress ON progress.company_id=notice.company_id AND progress.case_id=notice.case_id \
        WHERE notice.company_id=$1 AND notice.decision_id=$2 ORDER BY notice.principal_id LIMIT 17")
        .bind(company.as_uuid()).bind(decision.as_uuid()).fetch_all(db).await?;
    if rows.len() > 16 {
        return Err(invalid());
    }
    rows.into_iter()
        .map(|row| {
            Ok(EscalationRecipientView {
                principal: crate::entities::transport::PrincipalId::new(row.principal_id),
                label: row.principal_label,
                delivery: match row.state.as_str() {
                    "withdrawn" => EscalationRecipientState::Withdrawn,
                    "failed" => EscalationRecipientState::Failed,
                    "delivered" => EscalationRecipientState::Delivered,
                    "pending" => EscalationRecipientState::Pending,
                    _ => return Err(invalid()),
                },
            })
        })
        .collect()
}

async fn options_on(
    db: &mut PgConnection,
    company: CompanyId,
) -> AppResult<(
    Vec<crate::application::workflow::escalation::EscalationAssignee>,
    bool,
)> {
    let mut rows: Vec<(Uuid,String)> = sqlx::query_as("SELECT principal.id,principal.display_label FROM principals AS principal \
        WHERE principal.company_id=$1 AND (principal.kind='agent' OR (principal.kind='person' AND EXISTS ( \
        SELECT 1 FROM company_members AS member WHERE member.company_id=principal.company_id AND member.user_id=principal.user_id))) \
        ORDER BY principal.display_label,principal.id LIMIT 201")
        .bind(company.as_uuid()).fetch_all(db).await?;
    let truncated = rows.len() > 200;
    rows.truncate(200);
    Ok((
        rows.into_iter()
            .map(
                |(id, label)| crate::application::workflow::escalation::EscalationAssignee {
                    principal: crate::entities::transport::PrincipalId::new(id),
                    label,
                },
            )
            .collect(),
        truncated,
    ))
}

fn restore_view(
    company: CompanyId,
    decision: EscalationDecisionId,
    source: Decision,
) -> AppResult<EscalationView> {
    Ok(EscalationView {
        company,
        escalation: EscalationId::new(source.case_id),
        decision,
        source: match source.run_id {
            Some(run) => EscalationSourceView::Run {
                run: RunId::new(run),
            },
            None => EscalationSourceView::Admission {
                binding: WorkflowBindingId::new(source.binding_id),
            },
        },
        reason: EscalationReason {
            code: FailureCode::parse(&source.reason_code).map_err(|_| invalid())?,
            step: source
                .step_id
                .map(StepId::parse)
                .transpose()
                .map_err(|_| invalid())?,
            role: source
                .role
                .map(|role| role.parse::<DepartmentRole>())
                .transpose()
                .map_err(|_| invalid())?,
        },
        hop: EscalationHop::try_from(u8::try_from(source.assignment_hop).map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
        state: serde_json::from_value(serde_json::Value::String(source.state))
            .map_err(|_| invalid())?,
        settlement: source
            .settlement
            .map(|s| serde_json::from_value(serde_json::Value::String(s)))
            .transpose()
            .map_err(|_| invalid())?,
        current_decision: EscalationDecisionId::new(
            source.current_decision_id.ok_or_else(invalid)?,
        ),
        manual_assignment: source.manual_assignment,
        created_at: source.created_at,
        deadline: source.deadline,
        recipients: Vec::new(),
        settled_at: source.settled_at,
        command: None,
        payload_restricted: false,
        inspect_run: false,
        can_acknowledge: false,
        can_reassign: false,
        assignment_options: Vec::new(),
        assignment_options_truncated: false,
        cancel_revision: None,
    })
}

pub(in crate::adapters::persistence::workflow) async fn read_prelocked_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    decision: EscalationDecisionId,
    purpose: ReadPurpose,
) -> AppResult<EscalationView> {
    let source = decision_on(db, company, decision).await?;
    let (viewer, payload_access) = scope_access_on(db, company, actor, &source).await?;
    let recipients = recipients_on(db, company, decision).await?;
    let current = source.current_decision_id == Some(decision.as_uuid())
        && source.settled_at.is_none()
        && matches!(
            source.state.as_str(),
            "waiting" | "owner_waiting" | "manual"
        );
    let can_act = current
        && payload_access == CasePayloadAccess::Visible
        && (viewer.privileged
            || recipients
                .iter()
                .any(|r| r.principal.as_uuid() == viewer.principal));
    let cancel_revision = if can_act && viewer.privileged {
        match source.run_id {
            Some(run) => sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM workflow_runs WHERE company_id=$1 AND id=$2",
            )
            .bind(company.as_uuid())
            .bind(run)
            .fetch_optional(&mut *db)
            .await?
            .map(|value| u64::try_from(value).map(RunRevision).map_err(|_| invalid()))
            .transpose()?,
            None => None,
        }
    } else {
        None
    };
    let (assignment_options, assignment_options_truncated) =
        if purpose == ReadPurpose::Interactive && can_act && source.manual_assignments < 32 {
            options_on(db, company).await?
        } else {
            (Vec::new(), false)
        };
    let can_reassign = can_act && source.manual_assignments < 32;
    let mut view = restore_view(company, decision, source)?;
    view.command = command_on(db, company, decision).await?;
    view.inspect_run = viewer.privileged && payload_access == CasePayloadAccess::Visible;
    view.payload_restricted = payload_access != CasePayloadAccess::Visible;
    view.recipients = recipients;
    view.can_acknowledge = can_act;
    view.can_reassign = can_reassign;
    view.assignment_options = assignment_options;
    view.assignment_options_truncated = assignment_options_truncated;
    view.cancel_revision = cancel_revision;
    Ok(view)
}

async fn command_on(
    db: &mut PgConnection,
    company: CompanyId,
    decision: EscalationDecisionId,
) -> AppResult<Option<crate::application::workflow::escalation::EscalationCommandView>> {
    let value: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('actor_user',actor_user_id,'actor_principal',actor_principal_id, \
         'choice',choice,'created_at',created_at,'next_decision',next_decision_id) \
         FROM workflow_escalation_commands WHERE company_id=$1 AND decision_id=$2",
    )
    .bind(company.as_uuid())
    .bind(decision.as_uuid())
    .fetch_optional(db)
    .await?;
    value
        .map(|value| serde_json::from_value(value).map_err(|_| invalid()))
        .transpose()
}
