//! A local read commits directly into the canonical harness invocation. The driver's normal
//! result update then observes the exact AlreadyApplied value, without a second state machine.
use super::*;
use crate::application::agent::AgentKey;
use crate::{
    application::{
        knowledge::search::KnowledgeObservation,
        workflow::{agent::PreparedTool, knowledge::*, publication::ResolvedAgent},
    },
    domain::{entities::access_level::AccessLevel, workflow::StepId},
    services::harness::runs::InvocationState,
};
use std::time::Duration;

struct LoadedCall {
    checkpoint: RunCheckpoint,
    access: KnowledgeSearchAccess,
    completed: Option<Value>,
}

#[derive(sqlx::FromRow)]
struct FrozenSearchStep {
    step_id: String,
    frozen_inputs: Value,
    ceiling: i16,
}

#[derive(sqlx::FromRow)]
struct SavedConsumption {
    content_level: i16,
    result: Value,
}

#[async_trait]
impl KnowledgeSearchPersistence for PostgresPersistence {
    async fn knowledge_context(
        &self,
        fence: WorkflowFence,
        call: &KnowledgeToolCall,
    ) -> AppResult<KnowledgeSearchContext> {
        let bound = WriteBound::start("knowledge.context", read_policy()?);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = context_on(&mut tx, fence, call, &bound)
                    .await
                    .map(End::Commit);
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(result) => Ok(result),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => Err(bound.unresolved()),
        }
    }

    async fn observe_knowledge(
        &self,
        fence: WorkflowFence,
        call: &KnowledgeToolCall,
        observation: KnowledgeObservation,
    ) -> AppResult<Value> {
        let bound = WriteBound::start("knowledge.observe", read_policy()?);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = observe_on(&mut tx, fence, call, &observation, &bound)
                    .await
                    .map(End::Commit);
                bound.end(tx, result).await
            }))
            .await;
        match attempt {
            Attempt::Done(result) => Ok(result),
            Attempt::Failed(error) => Err(error),
            // A possibly committed result grants no model continuation. Canonical replay resolves it.
            Attempt::Ambiguous => Err(bound.unresolved()),
        }
    }
}

async fn context_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &KnowledgeToolCall,
    bound: &WriteBound,
) -> AppResult<KnowledgeSearchContext> {
    let loaded = load_call(tx, fence, call, bound).await?;
    if let Some(result) = &loaded.completed {
        let original = replay_on(tx, fence, call, &loaded, result, bound).await?;
        return Ok(KnowledgeSearchContext::Recorded(original));
    }
    token_budget::require_progression(tx, fence.scope).await?;
    live(tx, fence, bound).await?;
    Ok(KnowledgeSearchContext::Pending(loaded.access))
}

async fn observe_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &KnowledgeToolCall,
    observation: &KnowledgeObservation,
    bound: &WriteBound,
) -> AppResult<Value> {
    let loaded = load_call(tx, fence, call, bound).await?;
    if let Some(result) = &loaded.completed {
        return replay_on(tx, fence, call, &loaded, result, bound).await;
    }
    validate_knowledge_observation(observation, loaded.access.ceiling)?;
    let write = WorkflowCheckpointWrite {
        fence,
        checkpoint: loaded.checkpoint.run_id,
        expected_revision: loaded.checkpoint.revision,
        lease_policy: read_policy()?,
    };
    let update = WorkflowCheckpointUpdate::InvocationResult {
        invocation: call.invocation()?,
        result: observation.result().clone(),
    };
    let changed = updates::update_on(tx, write, update, bound).await?;
    if !matches!(changed, End::Commit(WriteOutcome::Applied(_))) {
        return Err(ownership_lost());
    }
    if let Some(level) = observation.highest() {
        super::super::knowledge::raise_content_level_on(
            tx,
            fence.scope.company,
            fence.scope.run,
            level,
        )
        .await?;
    }
    record_consumption(
        tx,
        fence,
        call,
        loaded.checkpoint.run_id.0,
        observation.highest().unwrap_or(AccessLevel::PUBLIC),
        observation.result(),
    )
    .await?;
    live(tx, fence, bound).await?;
    Ok(observation.result().clone())
}

async fn load_call(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &KnowledgeToolCall,
    bound: &WriteBound,
) -> AppResult<LoadedCall> {
    // Current agent/model/skill authority before run/job/harness locks, as in the driver.
    authorize_on(tx, fence).await?;
    if !lock_owner(tx, fence, bound).await? {
        return Err(ownership_lost());
    }
    let saved = load_on(tx, fence).await?.ok_or_else(missing)?;
    live(tx, fence, bound).await?;
    let frozen = frozen_selection(tx, fence).await?;
    if serde_json::to_value(&frozen.selection).map_err(|_| invalid())?
        != saved.workflow_prepared["selection"]
    {
        return Err(missing());
    }
    let tools: Vec<PreparedTool> =
        serde_json::from_value(saved.workflow_prepared["tools"].clone()).map_err(|_| invalid())?;
    let checkpoint = decode(saved.id, saved.checkpoint)?;
    let invocation =
        require_knowledge_invocation(fence, &frozen.selection, &tools, &checkpoint, call)?;
    let completed = (invocation.state == InvocationState::Completed)
        .then(|| invocation.result.clone())
        .flatten();
    if completed.is_none() && saved.model_request_id.is_some() {
        return Err(AppError::Conflict("Model request is still active".into()));
    }
    Ok(LoadedCall {
        checkpoint,
        access: frozen.access,
        completed,
    })
}

struct FrozenSelection {
    selection: ResolvedAgent,
    access: KnowledgeSearchAccess,
}

async fn frozen_selection(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
) -> AppResult<FrozenSelection> {
    let scope = fence.scope;
    let facts = super::super::http::runtime::facts(
        tx,
        ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
    )
    .await?;
    let row = sqlx::query_as::<_, FrozenSearchStep>(
        "SELECT execution.step_id, execution.frozen_inputs,
        run.knowledge_ceiling::smallint AS ceiling FROM workflow_executions AS execution
        JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
        WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    let step = StepId::parse(&row.step_id).map_err(|_| invalid())?;
    if facts
        .bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&step)
        .is_none_or(|step| step.step_type.as_str() != "agent.run")
    {
        return Err(missing());
    }
    let key = row
        .frozen_inputs
        .get("agent")
        .and_then(Value::as_str)
        .and_then(AgentKey::parse)
        .ok_or_else(missing)?;
    Ok(FrozenSelection {
        selection: facts.bundle.resolve_agent(scope.company, &key)?,
        access: KnowledgeSearchAccess {
            company: scope.company,
            ceiling: AccessLevel::try_from(row.ceiling).map_err(|_| invalid())?,
        },
    })
}

async fn replay_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &KnowledgeToolCall,
    loaded: &LoadedCall,
    result: &Value,
    bound: &WriteBound,
) -> AppResult<Value> {
    let scope = fence.scope;
    let proof = sqlx::query_as::<_, SavedConsumption>(
        "SELECT content_level::smallint AS content_level,result
        FROM workflow_knowledge_consumptions WHERE company_id=$1 AND run_id=$2 AND execution_id=$3
        AND job_id=$4 AND checkpoint_id=$5 AND invocation_id=$6 ORDER BY attempt_number LIMIT 1",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .bind(loaded.checkpoint.run_id.0)
    .bind(call.invocation()?.0)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    let level = AccessLevel::try_from(proof.content_level).map_err(|_| invalid())?;
    if proof.result != *result || level > loaded.access.ceiling {
        return Err(missing());
    }
    record_consumption(tx, fence, call, loaded.checkpoint.run_id.0, level, result).await?;
    live(tx, fence, bound).await?;
    Ok(result.clone())
}

async fn record_consumption(
    tx: &mut PgConnection,
    fence: WorkflowFence,
    call: &KnowledgeToolCall,
    checkpoint: Uuid,
    level: AccessLevel,
    result: &Value,
) -> AppResult<()> {
    let scope = fence.scope;
    // An exact replay still schedules the deferred check of this attempt's actual live claim.
    sqlx::query(
        "INSERT INTO workflow_knowledge_consumptions
        (company_id,run_id,execution_id,job_id,attempt_number,execution_generation,worker_id,
         checkpoint_id,invocation_id,content_level,result)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
        ON CONFLICT (company_id,run_id,execution_id,job_id,attempt_number,invocation_id)
        DO UPDATE SET content_level=EXCLUDED.content_level,result=EXCLUDED.result",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .bind(fence.attempt.0)
    .bind(fence.generation.0)
    .bind(fence.worker.0)
    .bind(checkpoint)
    .bind(call.invocation()?.0)
    .bind(level.as_i16())
    .bind(result)
    .execute(tx)
    .await?;
    Ok(())
}

fn read_policy() -> AppResult<LeasePolicy> {
    LeasePolicy::new(Duration::from_secs(6))
}

fn ownership_lost() -> AppError {
    AppError::Execution(crate::app_error::ExecutionFailure::OwnershipLost)
}
