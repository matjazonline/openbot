//! Follow refresh already owns company, run and decision. Only durable notices are enqueued.
use super::*;

pub(in crate::adapters::persistence::workflow) async fn assignment_changed_on(
    db: &mut PgConnection,
    company: CompanyId,
    decision: Uuid,
) -> AppResult<()> {
    let populated: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$2)",
    )
    .bind(company.as_uuid())
    .bind(decision)
    .fetch_one(&mut *db)
    .await?;
    if populated {
        return withdraw_on(db, company, decision).await;
    }
    let case: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO workflow_escalation_cases \
         (company_id,id,source_kind,run_id,execution_id,empty_decision_id,binding_id,binding_revision, \
          origin_department_id,policy,reason_code,step_id) \
         SELECT run.company_id,gen_random_uuid(),'empty_reviewers',run.id,follow.execution_id,follow.decision_id, \
                run.binding_id,run.binding_revision,follow.context_department_id,run.escalation,'decision.empty_reviewers',follow.step \
         FROM workflow_follow_decisions AS follow JOIN workflow_runs AS run \
           ON run.company_id=follow.company_id AND run.id=follow.run_id \
         WHERE follow.company_id=$1 AND follow.decision_id=$2 AND run.escalation IS NOT NULL \
           AND NOT EXISTS(SELECT 1 FROM workflow_run_notification_origins AS origin \
             WHERE origin.company_id=run.company_id AND origin.run_id=run.id) \
           AND workflow_follow_is_open(follow.company_id,follow.decision_id) \
           AND NOT EXISTS (SELECT 1 FROM workflow_decision_reviewers AS reviewer \
             WHERE reviewer.company_id=follow.company_id AND reviewer.decision_id=follow.decision_id) \
         ON CONFLICT (company_id,run_id) WHERE run_id IS NOT NULL DO NOTHING RETURNING id",
    )
    .bind(company.as_uuid())
    .bind(decision)
    .fetch_optional(&mut *db)
    .await?;
    if let Some(case) = case {
        enqueue_control_on(db, company, EscalationId::new(case), 1, chrono::Utc::now()).await?;
    }
    Ok(())
}

async fn withdraw_on(db: &mut PgConnection, company: CompanyId, decision: Uuid) -> AppResult<()> {
    let current: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT progress.case_id,progress.current_decision_id FROM workflow_escalation_cases AS source \
         JOIN workflow_escalation_progress AS progress ON progress.company_id=source.company_id AND progress.case_id=source.id \
         WHERE source.company_id=$1 AND source.source_kind='empty_reviewers' AND source.empty_decision_id=$2 \
           AND progress.state NOT IN ('settled','withdrawn') FOR UPDATE OF progress",
    )
    .bind(company.as_uuid())
    .bind(decision)
    .fetch_optional(&mut *db)
    .await?;
    let Some((case, assignment)) = current else {
        return Ok(());
    };
    if let Some(assignment) = assignment {
        sqlx::query("UPDATE workflow_escalation_decisions SET settled_at=clock_timestamp(),settlement='withdrawn' \
            WHERE company_id=$1 AND id=$2 AND settled_at IS NULL")
            .bind(company.as_uuid()).bind(assignment).execute(&mut *db).await?;
        crate::adapters::persistence::notification::close_escalation_assignment_on(
            db,
            company.as_uuid(),
            assignment,
        )
        .await?;
    }
    sqlx::query("UPDATE workflow_escalation_progress SET state='withdrawn',updated_at=clock_timestamp() WHERE company_id=$1 AND case_id=$2")
        .bind(company.as_uuid()).bind(case).execute(db).await?;
    Ok(())
}
