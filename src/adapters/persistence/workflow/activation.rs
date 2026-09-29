use super::*;
use crate::application::workflow::activation::*;
use futures::TryStreamExt;
use serde_json::Value;
use std::collections::BTreeMap;

struct FrozenActivation {
    inputs: Value,
    choice: Option<String>,
}

#[derive(sqlx::FromRow)]
struct RunRow {
    binding_id: Uuid,
    input: Value,
    max_context_bytes: i32,
    max_steps: i32,
    expired: bool,
    state: String,
}
#[derive(sqlx::FromRow)]
struct ExecutionRow {
    step_id: String,
    activation: i64,
    frozen_inputs: Option<Value>,
    frozen_choice: Option<String>,
}

#[async_trait]
impl WorkflowActivation for PostgresPersistence {
    async fn activate(&self, request: ActivationRequest) -> AppResult<ActivatedExecution> {
        let mut tx = self.pool.begin().await?;
        let result = activate_on(&mut tx, request).await?;
        tx.commit().await?;
        result.ok_or_else(budget_exhausted)
    }
}

fn budget_exhausted() -> AppError {
    AppError::Conflict("Workflow root activation budget exhausted".into())
}

/// None is a durably settled root-budget refusal; caller must commit it.
pub(super) async fn activate_on(
    db: &mut PgConnection,
    request: ActivationRequest,
) -> AppResult<Option<ActivatedExecution>> {
    let run = sqlx::query_as::<_, RunRow>(
        "SELECT binding_id, input, max_context_bytes, max_steps, state, deadline <= clock_timestamp() AS expired \
         FROM workflow_runs WHERE company_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(request.company.as_uuid()).bind(request.run.as_uuid())
    .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    let row = sqlx::query_as::<_, ExecutionRow>(
        "SELECT step_id, activation, frozen_inputs, frozen_choice FROM workflow_executions \
         WHERE company_id = $1 AND run_id = $2 AND id = $3 FOR UPDATE",
    )
    .bind(request.company.as_uuid())
    .bind(request.run.as_uuid())
    .bind(request.execution.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    validate_job(db, request).await?;
    let step = StepId::parse(row.step_id).map_err(|_| invalid())?;
    let ordinal = u64::try_from(row.activation).map_err(|_| invalid())?;
    let prepared = if let Some(inputs) = row.frozen_inputs {
        FrozenActivation {
            inputs,
            choice: row.frozen_choice,
        }
    } else {
        if run.expired
            || !matches!(run.state.as_str(), "queued" | "running")
            || row.activation > i64::from(run.max_steps)
        {
            return Err(AppError::Conflict(
                "Workflow activation limit reached".into(),
            ));
        }
        let prepared = resolve(db, request, &run, &step, row.activation).await?;
        if !budget::activate(db, request).await? {
            return Ok(None);
        }
        let written = sqlx::query(
            "UPDATE workflow_executions SET frozen_inputs = $4, frozen_choice = $5, activated_at = clock_timestamp() \
             WHERE company_id = $1 AND run_id = $2 AND id = $3 \
             AND EXISTS (SELECT 1 FROM workflow_runs AS run WHERE run.company_id = $1 \
                         AND run.id = $2 AND run.deadline > clock_timestamp())",
        )
        .bind(request.company.as_uuid()).bind(request.run.as_uuid())
        .bind(request.execution.as_uuid()).bind(&prepared.inputs).bind(&prepared.choice).execute(&mut *db).await?;
        if written.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "Workflow activation deadline reached".into(),
            ));
        }
        prepared
    };
    Ok(Some(ActivatedExecution {
        execution: request.execution,
        step,
        ordinal,
        inputs: prepared.inputs,
        choice: prepared
            .choice
            .map(ChoiceName::parse)
            .transpose()
            .map_err(|_| invalid())?,
    }))
}

async fn validate_job(db: &mut PgConnection, request: ActivationRequest) -> AppResult<()> {
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM background_tasks WHERE company_id = $1 AND id = $2 \
         AND workflow_execution_id = $3 AND queue_kind = 'workflow' FOR SHARE",
    )
    .bind(request.company.as_uuid())
    .bind(request.job.0)
    .bind(request.execution.as_uuid())
    .fetch_optional(db)
    .await?
    .ok_or_else(missing)?;
    if decode_job_payload(payload)? != request.execution {
        return Err(invalid());
    }
    Ok(())
}

async fn resolve(
    db: &mut PgConnection,
    request: ActivationRequest,
    run: &RunRow,
    step: &StepId,
    ordinal: i64,
) -> AppResult<FrozenActivation> {
    let binding = admission_binding::read_saved_binding(
        db,
        request.company,
        WorkflowBindingId::new(run.binding_id),
        request.run.as_uuid(),
    )
    .await?;
    let bundle = binding.bundle();
    let limits = bundle.compiled().graph().context_limits();
    if limits.output_bytes != usize::try_from(run.max_context_bytes).map_err(|_| invalid())? {
        return Err(invalid());
    }
    let dependencies = input_dependencies(bundle, step)?;
    let outputs = outputs(db, request, ordinal, bundle, limits, &dependencies).await?;
    let source: String = sqlx::query_scalar(
        "SELECT source_key FROM workflow_admissions WHERE company_id = $1 AND run_id = $2",
    )
    .bind(request.company.as_uuid())
    .bind(request.run.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let context = Context {
        input: &run.input,
        params: binding.params(),
        step_outputs: &outputs,
        run: RunMetadata {
            run_id: request.run,
            parent_run_id: parent_run(&source)?,
        },
    };
    let inputs = prepare_inputs(bundle, step, &context)?;
    let choice = bundle
        .compiled()
        .rule(step)
        .map(|rule| rule.decide(&context, limits))
        .transpose()
        .map_err(|_| AppError::BadRequest("Invalid rule predicate".into()))?
        .map(|choice| choice.as_str().to_owned());
    Ok(FrozenActivation { inputs, choice })
}

async fn outputs(
    db: &mut PgConnection,
    request: ActivationRequest,
    ordinal: i64,
    bundle: &PublishedBundle,
    limits: ContextLimits,
    dependencies: &std::collections::BTreeSet<StepId>,
) -> AppResult<BTreeMap<StepId, Value>> {
    let steps: Vec<_> = dependencies.iter().map(|step| step.as_str()).collect();
    // Select the latest activation before excluding failures: an earlier success
    // must not become visible when the newest activation has no successful output.
    let mut rows = sqlx::query_as::<_, (String, Value)>(
        "SELECT requested.step_id, prior.committed_output \
         FROM unnest($4::text[]) AS requested(step_id) \
         CROSS JOIN LATERAL (SELECT execution.committed_output, execution.committed_route \
           FROM workflow_executions AS execution \
           WHERE execution.company_id = $1 AND execution.run_id = $2 \
             AND execution.activation < $3 AND execution.step_id = requested.step_id \
             AND execution.completed_at IS NOT NULL \
           ORDER BY execution.activation DESC LIMIT 1) AS prior \
         WHERE prior.committed_route IS DISTINCT FROM 'final_error'",
    )
    .bind(request.company.as_uuid())
    .bind(request.run.as_uuid())
    .bind(ordinal)
    .bind(steps)
    .fetch(db);
    let mut outputs = BTreeMap::new();
    let mut remaining = limits.output_bytes;
    while let Some((step, value)) = rows.try_next().await? {
        let step = StepId::parse(step).map_err(|_| invalid())?;
        validate_context_value(&value, limits)
            .map_err(|_| AppError::BadRequest("Workflow context limit reached".into()))?;
        let size = serde_json::to_vec(&value).map_err(|_| invalid())?.len();
        remaining = remaining
            .checked_sub(size)
            .ok_or_else(|| AppError::BadRequest("Workflow context limit reached".into()))?;
        bundle
            .compiled()
            .validate_step_output(&step, &value)
            .map_err(|_| invalid())?;
        outputs.insert(step, value);
    }
    Ok(outputs)
}

fn parent_run(source: &str) -> AppResult<Option<RunId>> {
    let value: Value = serde_json::from_str(source.strip_prefix("v1:").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let fields = value.as_array().ok_or_else(invalid)?;
    match fields.first().and_then(Value::as_str) {
        Some("manual" | "message") if fields.len() == 2 => Ok(None),
        Some("schedule") if fields.len() == 3 => Ok(None),
        Some("child") if fields.len() == 5 => {
            let id = fields[1].as_str().ok_or_else(invalid)?;
            Ok(Some(RunId::new(
                Uuid::parse_str(id).map_err(|_| invalid())?,
            )))
        }
        _ => Err(invalid()),
    }
}
