use super::*;

pub(super) async fn authorize(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
) -> AppResult<()> {
    if command.rerun_of().is_some() && !matches!(command.trigger().source(), TriggerSource::Manual)
    {
        return Err(AppError::BadRequest(
            "Only a manual start can rerun a workflow run".into(),
        ));
    }
    match command.trigger().source() {
        TriggerSource::Manual => rerun(tx, command).await,
        TriggerSource::Message { message_id } => message(tx, command, message_id.as_uuid()).await,
        TriggerSource::Schedule {
            schedule_id,
            occurrence_id,
        } => schedule(tx, command, schedule_id.as_uuid(), occurrence_id.as_uuid()).await,
        // The runtime child writer is the only creation path (07-D1 §D1.2a).
        TriggerSource::Child { .. } => Err(AppError::BadRequest(
            "Child workflow runs are created only by their parent's workflow step".into(),
        )),
    }
}

#[derive(sqlx::FromRow)]
struct RerunOriginal {
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    workflow_id: Uuid,
    parent_run_id: Option<Uuid>,
}

/// The only place the rerun original is checked (07-D1 §D1.7), also on replay. An original the
/// actor cannot see is indistinguishable from a missing one. Its association lock follows the
/// new run's own association authorization, as the parent lock of a child admission did.
async fn rerun(tx: &mut Transaction<'_, Postgres>, command: &PreparedAdmission) -> AppResult<()> {
    let Some(original) = command.rerun_of() else {
        return Ok(());
    };
    let original = sqlx::query_as::<_, RerunOriginal>(
        "SELECT channel_id, thread_id, workflow_id, parent_run_id FROM workflow_runs \
         WHERE company_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(command.company_id().as_uuid())
    .bind(original.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    let association =
        binding_rows::association_from_columns(original.channel_id, original.thread_id)?;
    association::authorize_association(tx, command.company_id(), command.actor(), association)
        .await?;
    if original.parent_run_id.is_some() {
        return Err(AppError::BadRequest(
            "A rerun original must be a top-level workflow run".into(),
        ));
    }
    if original.workflow_id != command.workflow_id().as_uuid() {
        return Err(AppError::BadRequest(
            "A rerun must run the original's workflow".into(),
        ));
    }
    Ok(())
}

async fn message(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    message: Uuid,
) -> AppResult<()> {
    let RelatedAssociation::Thread {
        channel_id,
        thread_id,
    } = command.association()
    else {
        return Err(missing());
    };
    sqlx::query_scalar::<_, Uuid>(
        "SELECT message.id FROM messages AS message JOIN thread_messages AS membership \
           ON membership.company_id = message.company_id AND membership.message_id = message.id \
         WHERE message.company_id = $1 AND message.id = $2 \
           AND membership.channel_id = $3 AND membership.thread_id = $4 \
         FOR SHARE OF message, membership",
    )
    .bind(command.company_id().as_uuid())
    .bind(message)
    .bind(channel_id.as_uuid())
    .bind(thread_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    Ok(())
}

async fn schedule(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    schedule: Uuid,
    occurrence: Uuid,
) -> AppResult<()> {
    // A scheduled run is associated with its schedule's channel (p08 D1.9).
    let RelatedAssociation::Channel(channel) = command.association() else {
        return Err(missing());
    };
    // A schedule's channel is immutable. Lock schedule before occurrence, matching
    // the capture/materialization owner; the occurrence supplies its captured binding,
    // so retargeting the schedule later never changes what an occurrence admits.
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM channel_schedules WHERE company_id = $1 AND id = $2 AND channel_id = $3 FOR SHARE",
    ).bind(command.company_id().as_uuid()).bind(schedule).bind(channel.as_uuid())
        .fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM schedule_runs WHERE company_id = $1 AND schedule_id = $2 AND id = $3 \
         AND channel_id = $4 AND binding_id = $5 FOR SHARE",
    )
    .bind(command.company_id().as_uuid())
    .bind(schedule)
    .bind(occurrence)
    .bind(channel.as_uuid())
    .bind(command.binding().id().as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    Ok(())
}
