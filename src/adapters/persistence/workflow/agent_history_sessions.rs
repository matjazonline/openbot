use super::*;
use crate::application::workflow::agent_history::AgentHistorySessionRef;

/// Admission already holds current company/association authority. Session creation is part of
/// that same transaction, after the genuine admitted event, and never happens during a read.
pub(super) async fn capture(db: &mut PgConnection, command: &PreparedAdmission) -> AppResult<()> {
    let company = command.company_id();
    let run = command.proposed_run_id();
    let session = match command.history_session() {
        Some(reference) => reused_session(db, command, reference).await?,
        None => {
            let (channel, thread) = binding_rows::association_columns(command.association());
            let creator: Uuid = sqlx::query_scalar(
                "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
            )
            .bind(company.as_uuid())
            .bind(command.actor().user_id())
            .fetch_one(&mut *db)
            .await?;
            sqlx::query(
                "INSERT INTO agent_history_sessions \
                 (company_id,id,creator_principal_id,live_creator_principal_id,creator_user_id,channel_id,thread_id) \
                 VALUES ($1,$2,$3,$3,$4,$5,$6)",
            )
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .bind(creator)
            .bind(command.actor().user_id())
            .bind(channel)
            .bind(thread)
            .execute(&mut *db)
            .await?;
            run.as_uuid()
        }
    };
    let public_input = public_input_proven(db, command).await?;
    sqlx::query("INSERT INTO workflow_agent_sessions (company_id,run_id,session_id,thread_input_proven) VALUES ($1,$2,$3,$4)")
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(session)
        .bind(public_input)
        .execute(db)
        .await?;
    Ok(())
}

pub(super) fn check_manual(command: &PreparedAdmission) -> AppResult<()> {
    if command.history_session().is_some()
        && !matches!(command.trigger().source(), TriggerSource::Manual)
    {
        return Err(AppError::BadRequest(
            "Only a manual start can reuse an agent session".into(),
        ));
    }
    Ok(())
}

async fn reused_session(
    db: &mut PgConnection,
    command: &PreparedAdmission,
    reference: AgentHistorySessionRef,
) -> AppResult<Uuid> {
    check_manual(command)?;
    let (channel, thread) = binding_rows::association_columns(command.association());
    sqlx::query_scalar(
        "SELECT session.id FROM workflow_agent_sessions AS mapping \
         JOIN agent_history_sessions AS session \
           ON session.company_id=mapping.company_id AND session.id=mapping.session_id \
         JOIN workflow_runs AS source ON source.company_id=mapping.company_id AND source.id=mapping.run_id \
         WHERE mapping.company_id=$1 AND mapping.run_id=$2 AND source.parent_run_id IS NULL \
           AND session.creator_user_id=$3 AND source.actor_id=$3 \
           AND session.channel_id IS NOT DISTINCT FROM $4::uuid \
           AND session.thread_id IS NOT DISTINCT FROM $5::uuid \
         FOR SHARE OF session, mapping, source",
    )
    .bind(command.company_id().as_uuid())
    .bind(reference.from_run.as_uuid())
    .bind(command.actor().user_id())
    .bind(channel)
    .bind(thread)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::NotFound("agent session".into()))
}

/// Ordinary retries retain admission's actor-independent equivalence. Explicit reuse must
/// additionally resolve under this caller's current authority and match the saved identity.
pub(super) async fn replay_matches(
    db: &mut PgConnection,
    command: &PreparedAdmission,
    run: Uuid,
) -> AppResult<bool> {
    let saved: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM workflow_agent_sessions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(command.company_id().as_uuid())
    .bind(run)
    .fetch_one(&mut *db)
    .await?;
    match command.history_session() {
        Some(reference) => Ok(reused_session(db, command, reference).await? == saved),
        None => Ok(true),
    }
}

/// Only the real message producer can attest the audience. Capture once while admission
/// holds the source locks; later message edits or sender label changes cannot retarget it.
async fn public_input_proven(
    db: &mut PgConnection,
    command: &PreparedAdmission,
) -> AppResult<bool> {
    let Some(AdmissionClaim::Message(claim)) = command.admission_claim() else {
        return Ok(false);
    };
    let TriggerSource::Message { message_id } = command.trigger().source() else {
        return Err(invalid());
    };
    if claim.company != command.company_id()
        || claim.intent != command.trigger().trigger_id().as_uuid()
    {
        return Err(invalid());
    }
    let expected = message_intents::trigger_input_on(db, command.company_id(), *message_id)
        .await?
        .to_value();
    Ok(*command.input() == expected)
}
