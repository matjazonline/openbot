//! The memory tools commit into the canonical harness invocation, like knowledge search
//! (phase 05 E5/E6): a read's result, its classification rise and its consumption record in one
//! transaction before the model can see it; a save's delivery intent under the run's lock, with
//! its receipt as the result. The driver's normal update then observes the exact value.
use super::*;
use crate::{
    application::{
        memory::{MemoryNamespaces, NamespaceOwner},
        workflow::{agent::PreparedTool, memory_tools::*},
    },
    domain::entities::access_level::AccessLevel,
    services::harness::runs::InvocationState,
};
use serde_json::json;
use std::time::Duration;

struct LoadedCall {
    checkpoint: RunCheckpoint,
    completed: Option<Value>,
    agent: Uuid,
}

#[async_trait]
impl MemoryToolPersistence for PostgresPersistence {
    async fn memory_tool_context(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
    ) -> AppResult<MemoryToolContext> {
        let bound = WriteBound::start("memory.tool.context", policy()?);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                // A read: nothing it locked or checked is kept.
                let result = context_on(self, &mut tx, fence, call, &bound)
                    .await
                    .map(End::Rollback);
                bound.end(tx, result).await
            }))
            .await;
        finish(&bound, attempt)
    }

    async fn observe_memory_read(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        observation: MemoryReadObservation,
    ) -> AppResult<Value> {
        let bound = WriteBound::start("memory.tool.observe", policy()?);
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
        // A possibly committed result grants no model continuation (E6); replay resolves it.
        finish(&bound, attempt)
    }

    async fn save_memory(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        scope: MemoryToolScope,
        facts: Vec<String>,
    ) -> AppResult<MemorySaveOutcome> {
        let bound = WriteBound::start("memory.tool.save", policy()?);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = save_on(&mut tx, fence, call, scope, &facts, &bound).await;
                bound.end(tx, result).await
            }))
            .await;
        finish(&bound, attempt)
    }

    async fn refuse_memory_tool(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        result: Value,
    ) -> AppResult<Value> {
        let bound = WriteBound::start("memory.tool.refuse", policy()?);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let result = refuse_on(&mut tx, fence, call, &result, &bound)
                    .await
                    .map(End::Commit);
                bound.end(tx, result).await
            }))
            .await;
        finish(&bound, attempt)
    }

    async fn provision_memory_scope(
        &self,
        fence: WorkflowFence,
        scope: MemoryToolScope,
    ) -> AppResult<()> {
        // Resolve the owner in a short read, then provision in its own transaction (P7b).
        let bound = WriteBound::start("memory.tool.owner", policy()?);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let frozen = super::knowledge::frozen_selection(&mut tx, fence).await?;
                let agent = frozen.selection.agent().agent_id.as_uuid();
                let owner = eligible_on(&mut tx, fence, agent)
                    .await?
                    .into_iter()
                    .find(|(eligible, _)| *eligible == scope)
                    .map(|(_, owner)| owner);
                bound.end(tx, Ok(End::Rollback(owner))).await
            }))
            .await;
        let Some(owner) = finish(&bound, attempt)? else {
            return Ok(());
        };
        self.ensure_namespace(fence.scope.company, owner)
            .await
            .map(|_| ())
    }
}

fn finish<T>(bound: &WriteBound, attempt: Attempt<T>) -> AppResult<T> {
    match attempt {
        Attempt::Done(result) => Ok(result),
        Attempt::Failed(error) => Err(error),
        Attempt::Ambiguous => Err(bound.unresolved()),
    }
}

/// The saved invocation, checked against the run's actual frozen selection and current
/// authority before any memory state is read (as `knowledge::load_call` does).
async fn load_call(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &MemoryToolCall,
    bound: &WriteBound,
) -> AppResult<LoadedCall> {
    authorize_on(tx, fence).await?;
    if !lock_owner(tx, fence, bound).await? {
        return Err(ownership_lost());
    }
    let saved = load_on(tx, fence).await?.ok_or_else(missing)?;
    live(tx, fence, bound).await?;
    let frozen = super::knowledge::frozen_selection(tx, fence).await?;
    if serde_json::to_value(
        crate::application::workflow::agent_contract::AgentSelectionIdentity::from_selection(
            &frozen.selection,
        ),
    )
    .map_err(|_| invalid())?
        != saved.workflow_prepared["selection"]
    {
        return Err(missing());
    }
    let tools: Vec<PreparedTool> =
        serde_json::from_value(saved.workflow_prepared["tools"].clone()).map_err(|_| invalid())?;
    let checkpoint = decode(saved.id, saved.checkpoint)?;
    let invocation =
        require_memory_invocation(fence, &frozen.selection, &tools, &checkpoint, call)?;
    let completed = (invocation.state == InvocationState::Completed)
        .then(|| invocation.result.clone())
        .flatten();
    if completed.is_none() && saved.model_request_id.is_some() {
        return Err(AppError::Conflict("Model request is still active".into()));
    }
    Ok(LoadedCall {
        checkpoint,
        completed,
        agent: frozen.selection.agent().agent_id.as_uuid(),
    })
}

/// The scopes this run may use and their owners (E2): company always, the run's own agent,
/// and the user only when the root run is bound to an unattached private session created by
/// its actor. A missing identity omits the scope; it never widens to another.
pub(in crate::adapters::persistence::workflow) async fn eligible_on(
    tx: &mut PgConnection,
    fence: WorkflowFence,
    agent: Uuid,
) -> AppResult<Vec<(MemoryToolScope, NamespaceOwner)>> {
    let mut scopes = vec![
        (MemoryToolScope::Company, NamespaceOwner::Company),
        (MemoryToolScope::Agent, NamespaceOwner::Agent(agent)),
    ];
    if let Some(user) = private_session_user_on(tx, fence).await? {
        scopes.push((MemoryToolScope::User, NamespaceOwner::User(user)));
    }
    Ok(scopes)
}

/// The user whose private memory this run may reach (E2 with U4, `user_scope_owner`).
async fn private_session_user_on(
    tx: &mut PgConnection,
    fence: WorkflowFence,
) -> AppResult<Option<Uuid>> {
    user_scope_owner_on(tx, fence.scope.company, fence.scope.run).await
}

/// Reads the run's actor and its root's facts: the root found by the parent walk (≤ 64), its
/// bound session and its admission source. Every admission binds a session, so a schedule's or
/// a message's root is told apart by its source, never by the session.
pub(in crate::adapters::persistence::workflow) async fn user_scope_owner_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
) -> AppResult<Option<Uuid>> {
    #[derive(sqlx::FromRow)]
    struct RootRow {
        run_actor: Uuid,
        root_actor: Uuid,
        creator: Uuid,
        attached: bool,
        source_key: String,
    }
    let roots: Vec<RootRow> = sqlx::query_as(
        "WITH RECURSIVE chain(id, parent, actor, depth) AS ( \
           SELECT run.id, run.parent_run_id, run.actor_id, 0 FROM workflow_runs AS run \
           WHERE run.company_id = $1 AND run.id = $2 \
           UNION ALL \
           SELECT run.id, run.parent_run_id, run.actor_id, chain.depth + 1 \
           FROM workflow_runs AS run JOIN chain ON run.company_id = $1 AND run.id = chain.parent \
           WHERE chain.depth < 64) \
         SELECT (SELECT run.actor_id FROM workflow_runs AS run \
                 WHERE run.company_id = $1 AND run.id = $2) AS run_actor, \
           chain.actor AS root_actor, session.creator_user_id AS creator, \
           (session.channel_id IS NOT NULL OR session.thread_id IS NOT NULL) AS attached, \
           admission.source_key \
         FROM chain \
         JOIN workflow_agent_sessions AS bound \
           ON bound.company_id = $1 AND bound.run_id = chain.id \
         JOIN agent_history_sessions AS session \
           ON session.company_id = $1 AND session.id = bound.session_id \
         JOIN workflow_admissions AS admission \
           ON admission.company_id = $1 AND admission.run_id = chain.id \
         WHERE chain.parent IS NULL \
         LIMIT 2",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_all(db)
    .await?;
    // No root (or an ambiguous one) omits the scope; it never widens to another.
    let [root] = roots.as_slice() else {
        return Ok(None);
    };
    Ok(user_scope_owner(
        root.run_actor,
        Some(&UserScopeRoot {
            actor: root.root_actor,
            manual_start: super::super::admission_binding::SourceKey::is_manual(&root.source_key),
            session_creator: root.creator,
            session_attached: root.attached,
        }),
    ))
}

async fn context_on(
    persistence: &PostgresPersistence,
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &MemoryToolCall,
    bound: &WriteBound,
) -> AppResult<MemoryToolContext> {
    let loaded = load_call(tx, fence, call, bound).await?;
    // A destination below what the run already consumed refuses here (E3).
    let read_ceiling = super::super::memory_steps::read_ceiling_for(tx, fence).await?;
    if let Some(result) = &loaded.completed {
        replay_on(tx, fence, call, result, read_ceiling).await?;
        return Ok(MemoryToolContext::Recorded(result.clone()));
    }
    token_budget::require_progression(tx, fence.scope).await?;
    if !crate::adapters::persistence::memory::memory_enabled_on(tx, fence.scope.company).await? {
        return Ok(MemoryToolContext::NotEnabled);
    }
    let mut scopes = Vec::new();
    let mut not_ready = Vec::new();
    for (scope, owner) in eligible_on(tx, fence, loaded.agent).await? {
        match super::super::memory_steps::namespace_on(persistence, tx, fence.scope.company, owner)
            .await?
        {
            super::super::memory_steps::Namespace::Ready(namespace, credential) => {
                scopes.push(ScopeAccess {
                    scope,
                    namespace,
                    credential,
                })
            }
            super::super::memory_steps::Namespace::NotReady => not_ready.push(scope),
            super::super::memory_steps::Namespace::NotEnabled => {
                return Ok(MemoryToolContext::NotEnabled);
            }
        }
    }
    live(tx, fence, bound).await?;
    Ok(MemoryToolContext::Pending(MemoryToolAccess {
        read_ceiling,
        scopes,
        not_ready,
    }))
}

/// A completed read replays only while its recorded exposure still fits the current ceiling.
async fn replay_on(
    tx: &mut PgConnection,
    fence: WorkflowFence,
    call: &MemoryToolCall,
    result: &Value,
    read_ceiling: AccessLevel,
) -> AppResult<()> {
    let scope = fence.scope;
    let exposed: Option<i16> = sqlx::query_scalar(
        "SELECT max(exposed_level)::smallint FROM workflow_memory_consumptions \
         WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND job_id=$4 AND invocation_id=$5",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .bind(call.invocation()?.0)
    .fetch_one(tx)
    .await?;
    match exposed {
        // A recorded read: its exposure must still be within the ceiling.
        Some(level) => {
            if AccessLevel::try_from(level).map_err(|_| invalid())? > read_ceiling {
                return Err(AppError::KnowledgeRelease {
                    content: AccessLevel::try_from(level).map_err(|_| invalid())?,
                    destination: read_ceiling,
                });
            }
        }
        // A save receipt or a refusal exposes no memory content.
        None if call.tool == MemoryTool::Save || result.get("error").is_some() => {}
        None => return Err(missing()),
    }
    Ok(())
}

async fn observe_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &MemoryToolCall,
    observation: &MemoryReadObservation,
    bound: &WriteBound,
) -> AppResult<Value> {
    let loaded = load_call(tx, fence, call, bound).await?;
    let read_ceiling = super::super::memory_steps::read_ceiling_for(tx, fence).await?;
    if let Some(result) = &loaded.completed {
        replay_on(tx, fence, call, result, read_ceiling).await?;
        return Ok(result.clone());
    }
    // The answer stays within the ceiling it was authorized at and the current one (E6).
    if observation.exposed > observation.read_ceiling || observation.exposed > read_ceiling {
        return Err(AppError::KnowledgeRelease {
            content: observation.exposed,
            destination: read_ceiling.min(observation.read_ceiling),
        });
    }
    validate_result(call.tool, &observation.result)?;
    let write = WorkflowCheckpointWrite {
        fence,
        checkpoint: loaded.checkpoint.run_id,
        expected_revision: loaded.checkpoint.revision,
        lease_policy: policy()?,
    };
    let update = WorkflowCheckpointUpdate::InvocationResult {
        invocation: call.invocation()?,
        result: observation.result.clone(),
    };
    let changed = updates::update_on(tx, write, update, bound).await?;
    if !matches!(changed, End::Commit(WriteOutcome::Applied(_))) {
        return Err(ownership_lost());
    }
    let scope = fence.scope;
    super::super::knowledge::raise_content_level_on(
        tx,
        scope.company,
        scope.run,
        observation.exposed,
    )
    .await?;
    let refs = serde_json::to_value(&observation.items).map_err(|_| invalid())?;
    sqlx::query(
        "INSERT INTO workflow_memory_consumptions (company_id,run_id,execution_id,job_id, \
           attempt_number,invocation_id,read_ceiling,exposed_level,items) \
         VALUES ($1,$2,$3,$4,$5,$6,$7::smallint,$8::smallint,$9) \
         ON CONFLICT (company_id,run_id,execution_id,job_id,attempt_number,invocation_id) \
         DO UPDATE SET read_ceiling=EXCLUDED.read_ceiling, exposed_level=EXCLUDED.exposed_level, \
           items=EXCLUDED.items",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .bind(fence.attempt.0)
    .bind(call.invocation()?.0)
    .bind(observation.read_ceiling.as_i16())
    .bind(observation.exposed.as_i16())
    .bind(refs)
    .execute(&mut **tx)
    .await?;
    live(tx, fence, bound).await?;
    Ok(observation.result.clone())
}

async fn refuse_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &MemoryToolCall,
    result: &Value,
    bound: &WriteBound,
) -> AppResult<Value> {
    let loaded = load_call(tx, fence, call, bound).await?;
    if let Some(completed) = &loaded.completed {
        return Ok(completed.clone());
    }
    // A refusal carries no memory content, so it raises no level.
    if result.get("error").is_none() {
        return Err(invalid());
    }
    validate_result(call.tool, result)?;
    let write = WorkflowCheckpointWrite {
        fence,
        checkpoint: loaded.checkpoint.run_id,
        expected_revision: loaded.checkpoint.revision,
        lease_policy: policy()?,
    };
    let update = WorkflowCheckpointUpdate::InvocationResult {
        invocation: call.invocation()?,
        result: result.clone(),
    };
    let changed = updates::update_on(tx, write, update, bound).await?;
    if !matches!(changed, End::Commit(WriteOutcome::Applied(_))) {
        return Err(ownership_lost());
    }
    live(tx, fence, bound).await?;
    Ok(result.clone())
}

/// A tool save (E5/E6): the frozen request and its delivery intent in this ownership
/// transaction, the run row locked by the freeze so no concurrent read can lower its label;
/// the receipt is the invocation result. The delivery itself is asynchronous.
async fn save_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &MemoryToolCall,
    scope: MemoryToolScope,
    facts: &[String],
    bound: &WriteBound,
) -> AppResult<End<MemorySaveOutcome>> {
    let loaded = load_call(tx, fence, call, bound).await?;
    if let Some(result) = &loaded.completed {
        return Ok(End::Rollback(MemorySaveOutcome::Accepted(result.clone())));
    }
    let owner = eligible_on(tx, fence, loaded.agent)
        .await?
        .into_iter()
        .find(|(eligible, _)| *eligible == scope && scope != MemoryToolScope::Company)
        .map(|(_, owner)| owner);
    let save_scope = match owner {
        Some(NamespaceOwner::Agent(agent)) => {
            crate::adapters::persistence::memory::deliveries::SaveScope::Agent(agent)
        }
        Some(NamespaceOwner::User(user)) => {
            crate::adapters::persistence::memory::deliveries::SaveScope::User(user)
        }
        _ => return Ok(End::Rollback(MemorySaveOutcome::ScopeNotAllowed)),
    };
    let run = fence.scope;
    let invocation = call.invocation()?.0;
    // One logical save per tool call: its key survives every retry of this invocation (E5).
    let key = format!("ob.t.{}", invocation.simple());
    let intent = crate::adapters::persistence::memory::deliveries::SaveIntent {
        company: run.company,
        run: run.run,
        execution: run.execution,
        invocation: Some(invocation),
        wait: None,
        scope: save_scope,
        key: &key,
        facts,
        external_sources: save_sources(recalled_on(tx, fence).await?),
    };
    use crate::adapters::persistence::memory::deliveries::SaveEnqueue;
    let id = match crate::adapters::persistence::memory::deliveries::enqueue_save_on(tx, &intent)
        .await?
    {
        SaveEnqueue::Enqueued { id, .. } => id,
        SaveEnqueue::NotEnabled => return Ok(End::Rollback(MemorySaveOutcome::NotEnabled)),
        SaveEnqueue::NotReady => return Ok(End::Rollback(MemorySaveOutcome::NotReady(scope))),
        SaveEnqueue::TooLarge => {
            return Err(AppError::BadRequest("Memory save is too large".into()));
        }
    };
    let receipt = json!({"status": "accepted", "intent": id});
    validate_result(call.tool, &receipt)?;
    let write = WorkflowCheckpointWrite {
        fence,
        checkpoint: loaded.checkpoint.run_id,
        expected_revision: loaded.checkpoint.revision,
        lease_policy: policy()?,
    };
    let update = WorkflowCheckpointUpdate::InvocationResult {
        invocation: call.invocation()?,
        result: receipt.clone(),
    };
    let changed = updates::update_on(tx, write, update, bound).await?;
    if !matches!(changed, End::Commit(WriteOutcome::Applied(_))) {
        return Err(ownership_lost());
    }
    live(tx, fence, bound).await?;
    Ok(End::Commit(MemorySaveOutcome::Accepted(receipt)))
}

/// Every observation this execution showed the model, any attempt (a replayed checkpoint
/// carries them all): its tool reads and its automatic recall (the nil-invocation row; a
/// `memory.load` step never shares an `agent.run` execution). Read under the run lock, so no
/// in-flight read is missed (E6).
/// Bounded: one row per invocation (the checkpoint's `tool_calls` policy caps those), each row
/// at most 64 items by its CHECK.
async fn recalled_on(
    tx: &mut PgConnection,
    fence: WorkflowFence,
) -> AppResult<Vec<MemoryRecallRef>> {
    let scope = fence.scope;
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT consumption.items FROM workflow_memory_consumptions AS consumption \
         WHERE consumption.company_id=$1 AND consumption.run_id=$2 \
           AND consumption.execution_id=$3 AND consumption.job_id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .fetch_all(tx)
    .await?;
    let mut recalled = Vec::new();
    for items in rows {
        let items: Vec<MemoryRecallRef> = serde_json::from_value(items)
            .map_err(|_| AppError::Internal("Invalid workflow_memory_consumptions.items".into()))?;
        recalled.extend(items);
    }
    Ok(recalled)
}

fn validate_result(tool: MemoryTool, result: &Value) -> AppResult<()> {
    use crate::application::workflow::compiler::{Schema, SourceSpan};
    let max_bytes = crate::application::workflow::tools::MAX_TOOL_RESULT_BYTES
        .min(crate::services::harness::runs::MAX_RESULT_BYTES);
    if result.to_string().len() > max_bytes {
        return Err(AppError::Internal("Memory tool result too large".into()));
    }
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
    Schema::compile(memory_tool_schemas(tool).output, "/output", span)
        .and_then(|schema| schema.validate(result, "/output", span))
        .map_err(|_| AppError::Internal("Invalid memory tool result".into()))
}

fn policy() -> AppResult<LeasePolicy> {
    LeasePolicy::new(Duration::from_secs(6))
}

fn ownership_lost() -> AppError {
    AppError::Execution(crate::app_error::ExecutionFailure::OwnershipLost)
}
