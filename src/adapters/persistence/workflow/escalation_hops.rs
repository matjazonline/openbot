use super::*;

pub(super) async fn create_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
    source: &ControlSource,
    configured_hours: EscalationAckHours,
) -> AppResult<()> {
    let hop = source.current_hop + 1;
    let policy = source.policy()?;
    let hours = source
        .ack_hours
        .unwrap_or(i16::try_from(configured_hours.get()).map_err(|_| invalid())?);
    let role = if hop == 1 {
        policy.notify
    } else {
        DepartmentRole::Lead
    };
    let selection = select_on(db, event, source, role, hop).await?;
    let facts = facts_on(db, CompanyId::new(event.company_id), &selection).await?;
    let canonical = facts.canonical();
    let decision = Uuid::new_v4();
    let deadline = source.now + chrono::Duration::hours(i64::from(hours));
    sqlx::query("INSERT INTO workflow_escalation_decisions(company_id,case_id,id,assignment_ordinal,assignment_kind,hop,created_at,deadline) \
        VALUES ($1,$2,$3,$4,'automatic',$4,$5,$6)")
        .bind(event.company_id).bind(source.case_id).bind(decision).bind(hop).bind(source.now).bind(deadline).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_escalation_progress SET state='waiting',current_hop=$3,current_decision_id=$4,ack_hours=$5,updated_at=clock_timestamp() \
        WHERE company_id=$1 AND case_id=$2")
        .bind(event.company_id).bind(source.case_id).bind(hop).bind(decision).bind(hours).execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_run_escalations(company_id,case_id,hop,run_id,decision_id,origin_department_id, \
        chosen_department_id,role,reason_code,resolution,recipient_facts,control_event_id,created_at,deadline) \
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)")
        .bind(event.company_id).bind(source.case_id).bind(hop).bind(source.run_id).bind(decision)
        .bind(source.origin_department_id).bind(facts.department.map(|department|department.as_uuid())).bind(role.as_str())
        .bind(&source.reason_code).bind(facts.resolution()).bind(&canonical["recipients"])
        .bind(event.id.as_uuid()).bind(source.now).bind(deadline).execute(&mut *db).await?;
    enqueue_notices_on(db, event.company_id, source.case_id, decision, &facts).await?;
    let phase = if hop == 1 && policy.hops.get() == 2 {
        2
    } else {
        3
    };
    let available = if facts.facts.is_empty() {
        source.now
    } else {
        deadline
    };
    enqueue_control_on(
        db,
        CompanyId::new(event.company_id),
        EscalationId::new(source.case_id),
        phase,
        available,
    )
    .await
}

async fn select_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
    source: &ControlSource,
    role: DepartmentRole,
    hop: i16,
) -> AppResult<NotificationRecipients> {
    let company = CompanyId::new(event.company_id);
    let origin = source.origin_department_id.map(DepartmentId::new);
    if hop == 1 {
        return recipients_on(db, company, origin, role).await;
    }
    let parent: Option<Uuid> =
        sqlx::query_scalar("SELECT parent_id FROM departments WHERE company_id=$1 AND id=$2")
            .bind(event.company_id)
            .bind(source.origin_department_id)
            .fetch_optional(&mut *db)
            .await?
            .flatten();
    let Some(parent) = parent else {
        return Ok(NotificationRecipients::Refused {
            department: None,
            reason: NotificationRoleRefusal::MissingContext,
        });
    };
    agent_manifest::notification_department_recipients_on(
        db,
        company,
        DepartmentId::new(parent),
        role,
    )
    .await
}

async fn enqueue_notices_on(
    db: &mut PgConnection,
    company: Uuid,
    case: Uuid,
    decision: Uuid,
    facts: &HopRecipients,
) -> AppResult<()> {
    for recipient in &facts.facts {
        sqlx::query("WITH notice AS (INSERT INTO workflow_escalation_notices \
            (company_id,id,case_id,decision_id,principal_id,principal_kind,principal_label,event_id) \
            VALUES ($1,gen_random_uuid(),$2,$3,$4,$5,$6,gen_random_uuid()) RETURNING id,event_id) \
            INSERT INTO notification_events(id,company_id,source_kind,source_id,action_kind,source_generation) \
            SELECT event_id,$1,'escalation_notice',id,'escalation_review',1 FROM notice")
            .bind(company).bind(case).bind(decision).bind(recipient.principal.as_uuid()).bind(recipient.kind.as_str())
            .bind(&recipient.label).execute(&mut *db).await?;
    }
    Ok(())
}
