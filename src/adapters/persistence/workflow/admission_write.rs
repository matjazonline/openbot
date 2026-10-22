use super::*;
use crate::application::workflow::binding::ConfiguredBinding;
use admission_binding::SourceKey;
use chrono::{DateTime, Utc};
use serde_json::Value;

pub(super) async fn create(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    source: &SourceKey,
) -> AppResult<()> {
    insert_run(
        tx,
        &RunRow {
            company: command.company_id(),
            run: command.proposed_run_id(),
            binding: command.binding(),
            association: command.association(),
            actor_id: command.actor().user_id(),
            trigger: command.trigger().trigger_id(),
            correlation: command.causality().correlation_id().as_uuid(),
            input: command.input(),
            deadline: RunDeadline::AdmissionWindow,
            parent: None,
            rerun_of: command.rerun_of(),
        },
    )
    .await?;
    insert_first_job(
        tx,
        &FirstJob {
            company: command.company_id(),
            run: command.proposed_run_id(),
            execution: command.first_execution_id(),
            entry: command.entry(),
            association: command.association(),
            correlation: command.causality().correlation_id().as_uuid(),
        },
    )
    .await?;
    admission_history::capture(tx, command).await?;
    insert_admission(
        tx,
        command.company_id(),
        command.binding().id(),
        source,
        command.proposed_run_id(),
    )
    .await?;
    admission_replay::insert_command(tx, command, command.proposed_run_id().as_uuid()).await?;
    insert_admitted(
        tx,
        command.company_id(),
        command.proposed_run_id(),
        command.actor().user_id(),
    )
    .await
}

/// When a new run must finish. External admission opens a fresh window; a runtime child
/// keeps its parent's absolute deadline and can never extend it (07-D1 §D1.5).
pub(super) enum RunDeadline {
    AdmissionWindow,
    Inherited(DateTime<Utc>),
}

/// The parent step execution of a runtime child. Lineage and budget links follow from it.
#[derive(Clone, Copy)]
pub(super) struct ParentLink {
    pub run: RunId,
    pub execution: ExecutionId,
}

/// One run row, shared by external admission and the runtime child writer (07-D1 §D1.2).
/// The configured binding supplies the frozen bundle, parameters and resources; its binding
/// identity is the admitting binding, or the parent's for a child (provenance only).
pub(super) struct RunRow<'a> {
    pub company: CompanyId,
    pub run: RunId,
    pub binding: &'a ConfiguredBinding,
    pub association: RelatedAssociation,
    pub actor_id: Uuid,
    pub trigger: TriggerId,
    pub correlation: Uuid,
    pub input: &'a Value,
    pub deadline: RunDeadline,
    pub parent: Option<ParentLink>,
    pub rerun_of: Option<RunId>,
}

pub(super) async fn insert_run(db: &mut PgConnection, row: &RunRow<'_>) -> AppResult<()> {
    let (channel, thread) = binding_rows::association_columns(row.association);
    let definition = row.binding.bundle().compiled().graph().definition();
    let resources = serde_json::to_value(row.binding.resources())
        .map_err(|_| AppError::Internal("Workflow resource encoding failed".into()))?;
    let inherited = match row.deadline {
        RunDeadline::AdmissionWindow => None,
        RunDeadline::Inherited(deadline) => Some(deadline),
    };
    sqlx::query(
        "INSERT INTO workflow_runs (company_id, id, binding_id, binding_revision, workflow_id, version_id, \
         channel_id, thread_id, actor_id, trigger_id, correlation_id, bundle, input, params, resources, \
         max_steps, max_context_bytes, deadline, parent_run_id, parent_execution_id, rerun_of_run_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17, \
         COALESCE($18, CURRENT_TIMESTAMP + make_interval(secs => $19)), $20, $21, $22)",
    ).bind(row.company.as_uuid()).bind(row.run.as_uuid())
        .bind(row.binding.id().as_uuid()).bind(revision(row.binding.revision().get())?)
        .bind(definition.workflow_id.as_uuid()).bind(definition.version_id.as_uuid()).bind(channel).bind(thread)
        .bind(row.actor_id).bind(row.trigger.as_uuid())
        .bind(row.correlation).bind(store_bundle(row.binding.bundle())?)
        .bind(row.input).bind(row.binding.params()).bind(resources)
        .bind(i32::try_from(definition.limits.max_steps).map_err(|_| invalid())?)
        .bind(i32::try_from(definition.limits.max_context_bytes).map_err(|_| invalid())?)
        .bind(inherited)
        .bind(f64::from(admission::ADMISSION_DEADLINE_SECONDS))
        .bind(row.parent.map(|parent| parent.run.as_uuid()))
        .bind(row.parent.map(|parent| parent.execution.as_uuid()))
        .bind(row.rerun_of.map(|original| original.as_uuid())).execute(db).await?;
    Ok(())
}

/// A new run's entry execution and its pending job.
pub(super) struct FirstJob<'a> {
    pub company: CompanyId,
    pub run: RunId,
    pub execution: ExecutionId,
    pub entry: &'a StepId,
    pub association: RelatedAssociation,
    pub correlation: Uuid,
}

pub(super) async fn insert_first_job(db: &mut PgConnection, job: &FirstJob<'_>) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO workflow_executions (company_id, run_id, id, step_id, activation) VALUES ($1,$2,$3,$4,1)",
    ).bind(job.company.as_uuid()).bind(job.run.as_uuid()).bind(job.execution.as_uuid())
        .bind(job.entry.as_str()).execute(&mut *db).await?;
    let (channel, thread) = binding_rows::association_columns(job.association);
    sqlx::query(
        "INSERT INTO background_tasks (id, company_id, channel_id, thread_id, correlation_id, task_type, \
         payload, queue_kind, workflow_execution_id) VALUES ($1,$2,$3,$4,$5,'workflow_execution',$6,'workflow',$7)",
    ).bind(Uuid::new_v4()).bind(job.company.as_uuid()).bind(channel).bind(thread)
        .bind(job.correlation)
        .bind(crate::application::workflow::activation::job_payload(job.execution))
        .bind(job.execution.as_uuid()).execute(db).await?;
    Ok(())
}

pub(super) async fn insert_admission(
    db: &mut PgConnection,
    company: CompanyId,
    binding: WorkflowBindingId,
    source: &SourceKey,
    run: RunId,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO workflow_admissions (company_id, binding_id, source_key, run_id) VALUES ($1, $2, $3, $4)",
    ).bind(company.as_uuid()).bind(binding.as_uuid())
        .bind(source.as_str()).bind(run.as_uuid()).execute(db).await?;
    Ok(())
}

pub(super) async fn insert_admitted(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    actor_id: Uuid,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO workflow_run_events (company_id, run_id, sequence, event_kind, actor_id) \
         VALUES ($1, $2, 1, 'admitted', $3)",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .bind(actor_id)
    .execute(db)
    .await?;
    Ok(())
}
