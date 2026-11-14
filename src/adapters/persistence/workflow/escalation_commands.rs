use super::*;

#[derive(sqlx::FromRow)]
struct Receipt {
    fingerprint: String,
}

async fn receipt_on(
    db: &mut PgConnection,
    command: &SubmitEscalation,
) -> AppResult<Option<Receipt>> {
    Ok(sqlx::query_as("SELECT fingerprint FROM workflow_escalation_commands WHERE company_id=$1 AND command_key=$2")
        .bind(command.company.as_uuid()).bind(command.command.as_str()).fetch_optional(db).await?)
}

pub(super) async fn submit(
    p: &PostgresPersistence,
    command: SubmitEscalation,
) -> AppResult<EscalationSubmission> {
    let mut tx = p.pool.begin().await?;
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout='20s'")
        .execute(&mut *tx)
        .await?;
    let target = target_hint_on(&mut tx, &command).await?;
    lock_accounts_on(&mut tx, &command, target.as_ref()).await?;
    // All decision, ladder and notice writers serialize at company before run/case/assignment.
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(command.company.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(missing)?;
    let result = apply_on(&mut tx, &command, target.as_ref()).await?;
    tx.commit().await?;
    Ok(result)
}

pub(in crate::adapters::persistence::workflow) async fn apply_on(
    db: &mut PgConnection,
    command: &SubmitEscalation,
    target: Option<&assignment::Target>,
) -> AppResult<EscalationSubmission> {
    let fingerprint = command.fingerprint()?;
    let source = decision_on(db, command.company, command.decision).await?;
    let viewer = authorize_on(db, command.company, command.actor, &source).await?;
    if let Some(saved) = receipt_on(db, command).await? {
        if saved.fingerprint != fingerprint {
            return Err(conflict());
        }
        // Current read scope is enough; historical recipients and assignment targets add no locks.
        return Ok(EscalationSubmission::Replayed);
    }
    if source.current_decision_id != Some(command.decision.as_uuid())
        || source.settled_at.is_some()
        || !matches!(
            source.state.as_str(),
            "waiting" | "owner_waiting" | "manual"
        )
    {
        return Err(conflict());
    }
    if !assigned_on(db, command, &viewer).await? {
        return Err(missing());
    }
    let settlement = match command.choice {
        EscalationChoice::Acknowledge => "acknowledged",
        EscalationChoice::Reassign { .. } => "reassigned",
        EscalationChoice::Cancel {
            expected_run_revision,
        } => cancel_on(db, command, &source, expected_run_revision).await?,
    };
    sqlx::query(
        "UPDATE workflow_escalation_decisions SET settled_at=clock_timestamp(),settlement=$3 \
        WHERE company_id=$1 AND id=$2 AND settled_at IS NULL",
    )
    .bind(command.company.as_uuid())
    .bind(command.decision.as_uuid())
    .bind(settlement)
    .execute(&mut *db)
    .await?;
    let next = if matches!(command.choice, EscalationChoice::Reassign { .. }) {
        Some(assignment::reassign_on(db, command, &source, target).await?)
    } else {
        sqlx::query("UPDATE workflow_escalation_progress SET state='settled',updated_at=clock_timestamp() WHERE company_id=$1 AND case_id=$2")
            .bind(command.company.as_uuid()).bind(source.case_id).execute(&mut *db).await?;
        None
    };
    crate::adapters::persistence::notification::close_escalation_assignment_on(
        db,
        command.company.as_uuid(),
        command.decision.as_uuid(),
    )
    .await?;
    sqlx::query("INSERT INTO workflow_escalation_commands \
        (company_id,command_key,case_id,decision_id,actor_user_id,actor_principal_id,fingerprint,choice,settlement,next_decision_id) \
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(command.company.as_uuid()).bind(command.command.as_str()).bind(source.case_id).bind(command.decision.as_uuid())
        .bind(command.actor.user_id()).bind(viewer.principal).bind(fingerprint).bind(command.choice.value()).bind(settlement).bind(next)
        .execute(&mut *db).await?;
    Ok(EscalationSubmission::Applied)
}

async fn lock_accounts_on(
    db: &mut PgConnection,
    command: &SubmitEscalation,
    target: Option<&assignment::Target>,
) -> AppResult<()> {
    let mut accounts = vec![command.actor.user_id()];
    if let Some(target) = target {
        accounts.push(target.account);
    }
    accounts.sort_unstable();
    accounts.dedup();
    let locked: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR SHARE")
            .bind(&accounts)
            .fetch_all(db)
            .await?;
    if locked != accounts {
        return Err(missing());
    }
    Ok(())
}

async fn cancel_on(
    db: &mut PgConnection,
    command: &SubmitEscalation,
    source: &Decision,
    expected: RunRevision,
) -> AppResult<&'static str> {
    let run = RunId::new(
        source
            .run_id
            .ok_or_else(|| AppError::BadRequest("escalation.no_run_to_cancel".into()))?,
    );
    authority::authorize_company(db, command.company, command.actor).await?;
    let head = sqlx::query_as::<_, inspection::HeadRow>(&format!(
        "{} FOR UPDATE OF run",
        inspection::HEAD
    ))
    .bind(command.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?
    .restore()?;
    association::authorize_association(db, command.company, command.actor, head.association)
        .await?;
    if head.revision != expected {
        return Err(conflict());
    }
    if head.state.is_terminal() {
        return Ok("already_terminal");
    }
    // Reuse actual subtree/job/wait/delivery cancellation in this administrative transaction.
    let control = CancelCommand {
        company_id: command.company,
        actor: command.actor,
        run_id: run,
        command_key: IdempotencyKey::parse(format!("escalation:{}", command.decision.as_uuid()))?,
        expected_revision: expected,
    };
    super::super::super::controls::cancel_on(db, &control).await?;
    sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id) \
        SELECT $1,$2,COALESCE(MAX(sequence),0)+1,'control_cancel',$3 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2")
        .bind(command.company.as_uuid()).bind(run.as_uuid()).bind(command.actor.user_id()).execute(db).await?;
    Ok("cancelled")
}

pub(in crate::adapters::persistence::workflow) async fn target_hint_on(
    db: &mut PgConnection,
    command: &SubmitEscalation,
) -> AppResult<Option<assignment::Target>> {
    if receipt_on(db, command).await?.is_some() {
        return Ok(None);
    }
    match command.choice {
        EscalationChoice::Reassign { principal } => {
            assignment::target_on(db, command.company, principal).await
        }
        _ => Ok(None),
    }
}
