use super::*;
use admission_binding::SourceKey;

pub(super) async fn create(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    source: &SourceKey,
) -> AppResult<()> {
    insert_run(tx, command).await?;
    insert_execution_job(tx, command).await?;
    admission_history::capture(tx, command).await?;
    sqlx::query(
        "INSERT INTO workflow_admissions (company_id, binding_id, source_key, run_id) VALUES ($1, $2, $3, $4)",
    ).bind(command.company_id().as_uuid()).bind(command.binding().id().as_uuid())
        .bind(source.as_str()).bind(command.proposed_run_id().as_uuid()).execute(&mut **tx).await?;
    admission_replay::insert_command(tx, command, command.proposed_run_id().as_uuid()).await?;
    sqlx::query(
        "INSERT INTO workflow_run_events (company_id, run_id, sequence, event_kind, actor_id) \
         VALUES ($1, $2, 1, 'admitted', $3)",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.proposed_run_id().as_uuid())
    .bind(command.actor().user_id())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_run(db: &mut PgConnection, command: &PreparedAdmission) -> AppResult<()> {
    let (channel, thread) = binding_rows::association_columns(command.association());
    let limits = command
        .binding()
        .bundle()
        .compiled()
        .graph()
        .definition()
        .limits;
    let resources = serde_json::to_value(command.binding().resources())
        .map_err(|_| AppError::Internal("Workflow resource encoding failed".into()))?;
    sqlx::query(
        "INSERT INTO workflow_runs (company_id, id, binding_id, binding_revision, workflow_id, version_id, \
         channel_id, thread_id, actor_id, trigger_id, correlation_id, bundle, input, params, resources, \
         max_steps, max_context_bytes, deadline) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17, \
         CURRENT_TIMESTAMP + make_interval(secs => $18))",
    ).bind(command.company_id().as_uuid()).bind(command.proposed_run_id().as_uuid())
        .bind(command.binding().id().as_uuid()).bind(revision(command.binding().revision().get())?)
        .bind(command.workflow_id().as_uuid()).bind(command.version_id().as_uuid()).bind(channel).bind(thread)
        .bind(command.actor().user_id()).bind(command.trigger().trigger_id().as_uuid())
        .bind(command.causality().correlation_id().as_uuid()).bind(store_bundle(command.binding().bundle())?)
        .bind(command.input()).bind(command.params()).bind(resources)
        .bind(i32::try_from(limits.max_steps).map_err(|_| invalid())?)
        .bind(i32::try_from(limits.max_context_bytes).map_err(|_| invalid())?)
        .bind(f64::from(admission::ADMISSION_DEADLINE_SECONDS)).execute(db).await?;
    Ok(())
}

async fn insert_execution_job(db: &mut PgConnection, command: &PreparedAdmission) -> AppResult<()> {
    let execution = command.first_execution_id().as_uuid();
    sqlx::query(
        "INSERT INTO workflow_executions (company_id, run_id, id, step_id, activation) VALUES ($1,$2,$3,$4,1)",
    ).bind(command.company_id().as_uuid()).bind(command.proposed_run_id().as_uuid()).bind(execution)
        .bind(command.entry().as_str()).execute(&mut *db).await?;
    let (channel, thread) = binding_rows::association_columns(command.association());
    sqlx::query(
        "INSERT INTO background_tasks (id, company_id, channel_id, thread_id, correlation_id, task_type, \
         payload, queue_kind, workflow_execution_id) VALUES ($1,$2,$3,$4,$5,'workflow_execution',$6,'workflow',$7)",
    ).bind(Uuid::new_v4()).bind(command.company_id().as_uuid()).bind(channel).bind(thread)
        .bind(command.causality().correlation_id().as_uuid())
        .bind(crate::application::workflow::activation::job_payload(command.first_execution_id()))
        .bind(execution).execute(db).await?;
    Ok(())
}
