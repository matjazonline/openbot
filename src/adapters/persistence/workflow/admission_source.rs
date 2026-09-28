use super::*;

pub(super) async fn authorize(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
) -> AppResult<()> {
    match command.trigger().source() {
        TriggerSource::Manual => Ok(()),
        TriggerSource::Message { message_id } => message(tx, command, message_id.as_uuid()).await,
        TriggerSource::Schedule {
            schedule_id,
            occurrence_id,
        } => schedule(tx, command, schedule_id.as_uuid(), occurrence_id.as_uuid()).await,
        TriggerSource::Child {
            parent: ChildCause::Execution(execution),
        } => child(tx, command, execution).await,
        TriggerSource::Child {
            parent: ChildCause::Action(_),
        } => Err(AppError::BadRequest(
            "Workflow child action authority is not available".into(),
        )),
    }
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
    let (channel, thread) = binding_rows::association_columns(command.association());
    // A schedule's channel is immutable. Lock schedule before occurrence, matching
    // the capture/materialization owner; the occurrence supplies its saved scope.
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM channel_schedules WHERE company_id = $1 AND id = $2 AND channel_id = $3 FOR SHARE",
    ).bind(command.company_id().as_uuid()).bind(schedule).bind(channel)
        .fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM schedule_runs WHERE company_id = $1 AND schedule_id = $2 AND id = $3 \
         AND channel_id = $4 AND thread_id IS NOT DISTINCT FROM $5 FOR SHARE",
    )
    .bind(command.company_id().as_uuid())
    .bind(schedule)
    .bind(occurrence)
    .bind(channel)
    .bind(thread)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ParentAssociation {
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
}
async fn child(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    parent: &ExecutionRef,
) -> AppResult<()> {
    if parent.company_id() != command.company_id() {
        return Err(missing());
    }
    // Run first, then its execution, matching the runtime transition lock order.
    let scope = sqlx::query_as::<_, ParentAssociation>(
        "SELECT channel_id, thread_id FROM workflow_runs WHERE company_id = $1 AND id = $2 FOR SHARE",
    ).bind(command.company_id().as_uuid()).bind(parent.run_id().as_uuid())
        .fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM workflow_executions WHERE company_id = $1 AND run_id = $2 \
         AND id = $3 AND step_id = $4 FOR SHARE",
    )
    .bind(command.company_id().as_uuid())
    .bind(parent.run_id().as_uuid())
    .bind(parent.execution_id().as_uuid())
    .bind(parent.step_id().as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    let association = match (scope.channel_id, scope.thread_id) {
        (None, None) => RelatedAssociation::Company,
        (Some(channel), None) => RelatedAssociation::Channel(RelatedChannelId::new(channel)),
        (Some(channel), Some(thread)) => RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(channel),
            thread_id: RelatedThreadId::new(thread),
        },
        _ => return Err(invalid()),
    };
    association::authorize_association(tx, command.company_id(), command.actor(), association).await
}
