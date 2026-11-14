use super::*;
use crate::entities::{participant::PrincipalKind, transport::PrincipalId};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub(in crate::adapters::persistence::workflow) struct Target {
    principal: Uuid,
    kind: String,
    label: String,
    pub account: Uuid,
    desk: Option<Uuid>,
}

pub(super) async fn target_on(
    db: &mut PgConnection,
    company: CompanyId,
    principal: PrincipalId,
) -> AppResult<Option<Target>> {
    Ok(sqlx::query_as("SELECT principal.id AS principal,principal.kind,principal.display_label AS label, \
        COALESCE(principal.user_id,route.run_as_user_id) AS account,route.desk_id AS desk \
        FROM principals AS principal \
        LEFT JOIN LATERAL workflow_escalation_agent_route(principal.company_id,principal.id) AS route \
          ON principal.kind='agent' \
        WHERE principal.company_id=$1 AND principal.id=$2 AND ( \
          (principal.kind='person' AND principal.user_id IS NOT NULL AND EXISTS (SELECT 1 FROM company_members AS member \
            WHERE member.company_id=principal.company_id AND member.user_id=principal.user_id)) \
          OR (principal.kind='agent' AND route.run_as_user_id IS NOT NULL))")
        .bind(company.as_uuid()).bind(principal.as_uuid()).fetch_optional(db).await?)
}

pub(super) async fn reassign_on(
    db: &mut PgConnection,
    command: &SubmitEscalation,
    source: &Decision,
    hint: Option<&Target>,
) -> AppResult<Uuid> {
    let EscalationChoice::Reassign { principal } = command.choice else {
        return Err(invalid());
    };
    let target = target_on(db, command.company, principal)
        .await?
        .ok_or_else(missing)?;
    if Some(&target) != hint {
        return Err(conflict());
    }
    if source.manual_assignments >= 32 {
        return Err(AppError::Conflict(
            "escalation.manual_assignment_limit".into(),
        ));
    }
    let actor = WorkflowActor::authenticated(target.account)?;
    if target.kind == "agent" {
        // This is real run_as authority, never an account manufactured for an agent principal.
        authority::authorize_company(db, command.company, actor).await?;
        association::authorize_association(
            db,
            command.company,
            actor,
            RelatedAssociation::Channel(RelatedChannelId::new(target.desk.ok_or_else(invalid)?)),
        )
        .await?;
    }
    authorize_on(db, command.company, actor, source).await?;
    let ordinal = source.manual_assignments + 1;
    let next = Uuid::new_v4();
    sqlx::query("INSERT INTO workflow_escalation_decisions \
        (company_id,case_id,id,assignment_ordinal,assignment_kind,manual_ordinal,manual_principal_id,manual_principal_kind,manual_principal_label) \
        VALUES ($1,$2,$3,$4,'manual',$5,$6,$7,$8)")
        .bind(command.company.as_uuid()).bind(source.case_id).bind(next).bind(ordinal+2).bind(ordinal)
        .bind(target.principal).bind(&target.kind).bind(&target.label).execute(&mut *db).await?;
    let facts = HopRecipients {
        department: None,
        refusal: None,
        facts: vec![recipient_facts::RecipientFact {
            principal,
            kind: match target.kind.as_str() {
                "person" => PrincipalKind::Person,
                "agent" => PrincipalKind::Agent,
                _ => return Err(invalid()),
            },
            label: target.label,
        }],
    };
    controls::enqueue_notices_on(db, command.company.as_uuid(), source.case_id, next, &facts)
        .await?;
    sqlx::query(
        "UPDATE workflow_escalation_progress SET state='manual',manual_assignments=$3, \
        current_decision_id=$4,updated_at=clock_timestamp() WHERE company_id=$1 AND case_id=$2",
    )
    .bind(command.company.as_uuid())
    .bind(source.case_id)
    .bind(ordinal)
    .bind(next)
    .execute(db)
    .await?;
    Ok(next)
}
