use super::*;
use admission_binding::SourceKey;
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct SavedCommand {
    run_id: Uuid,
    binding_id: Uuid,
    source_key: String,
    trigger_id: Uuid,
}
#[derive(sqlx::FromRow)]
struct SavedInput {
    input: Value,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
}

/// Command identity is stricter than canonical source identity. A source alias
/// saves its own trigger UUID, so repeating that alias cannot change the command.
pub(super) async fn resolve(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    source: &SourceKey,
) -> AppResult<Option<AdmissionResult>> {
    let saved = sqlx::query_as::<_, SavedCommand>(
        "SELECT command.run_id, admission.binding_id, admission.source_key, command.trigger_id \
         FROM workflow_admission_commands AS command JOIN workflow_admissions AS admission \
           ON admission.company_id = command.company_id AND admission.run_id = command.run_id \
         WHERE command.company_id = $1 AND command.command_key = $2",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.idempotency_key().as_str())
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(saved) = saved {
        if saved.binding_id != command.binding().id().as_uuid()
            || saved.source_key != source.as_str()
            || saved.trigger_id != command.trigger().trigger_id().as_uuid()
        {
            return Ok(Some(AdmissionResult::Conflict));
        }
        return replay(tx, command, saved.run_id).await.map(Some);
    }
    let run: Option<Uuid> = sqlx::query_scalar(
        "SELECT run_id FROM workflow_admissions \
         WHERE company_id = $1 AND binding_id = $2 AND source_key = $3",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.binding().id().as_uuid())
    .bind(source.as_str())
    .fetch_optional(&mut **tx)
    .await?;
    let Some(run) = run else { return Ok(None) };
    let result = replay(tx, command, run).await?;
    if matches!(result, AdmissionResult::Replayed(_)) {
        insert_command(tx, command, run).await?;
    }
    Ok(Some(result))
}

async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    run: Uuid,
) -> AppResult<AdmissionResult> {
    let saved = sqlx::query_as::<_, SavedInput>(
        "SELECT input, channel_id, thread_id FROM workflow_runs \
         WHERE company_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(command.company_id().as_uuid())
    .bind(run)
    .fetch_one(&mut **tx)
    .await?;
    let association = binding_rows::association_columns(command.association());
    if saved.input != *command.input() || (saved.channel_id, saved.thread_id) != association {
        return Ok(AdmissionResult::Conflict);
    }
    // Always restore the committed configuration: the caller may have prepared
    // before a competitor committed, against a newer or older head revision.
    let configuration = admission_binding::read_saved_binding(
        tx,
        command.company_id(),
        command.binding().id(),
        run,
    )
    .await?;
    authority::supported_publication(configuration.bundle())?;
    resources::check_readiness(tx, &configuration).await?;
    Ok(AdmissionResult::Replayed(RunId::new(run)))
}

pub(super) async fn insert_command(
    db: &mut PgConnection,
    command: &PreparedAdmission,
    run: Uuid,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO workflow_admission_commands (company_id, command_key, run_id, trigger_id) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.idempotency_key().as_str())
    .bind(run)
    .bind(command.trigger().trigger_id().as_uuid())
    .execute(db)
    .await?;
    Ok(())
}
