//! Automatic recall's persistence (phase 05 E7): the rolled-back plan read through the
//! preparation's own authorization, the freeze of fetched packs into the frozen preparation
//! (level raise and consumption record in the preparation transaction), and the scopes a
//! resumed attempt revalidates against.
use super::*;
use crate::application::workflow::memory_recall::Revalidation;
use crate::application::workflow::{
    agent::{FrozenMemory, PreparedAgent, RecallState, RecalledMemory},
    lease::{ClaimedWorkflow, LeasePolicy},
    memory_recall::{FetchedRecall, MemoryRecallPersistence, RecallPlan, RecallRequest},
    memory_tools::{MemoryRecallRef, MemoryToolPersistence, MemoryToolScope, ScopeAccess},
};
use crate::entities::access_level::AccessLevel;
use serde_json::Value;

const QUERY_CHARS: usize = 1024;

#[async_trait]
impl MemoryRecallPersistence for PostgresPersistence {
    async fn recall_plan(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> AppResult<RecallPlan> {
        let bound = WriteBound::start("memory.recall.plan", policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut connection = bound.acquire(self.pool()).await?;
                let mut tx = bound.begin(&mut connection).await?;
                // A read: nothing it locked or checked is kept.
                let plan = Box::pin(plan_on(self, &mut tx, claim, &bound))
                    .await
                    .map(End::Rollback);
                bound.end(tx, plan).await
            }))
            .await;
        finished(&bound, attempt)
    }

    async fn provision_recall_scope(
        &self,
        fence: WorkflowFence,
        scope: MemoryToolScope,
    ) -> AppResult<()> {
        // The same owner resolution and own-transaction provisioning as the tools (P7b).
        self.provision_memory_scope(fence, scope).await
    }

    async fn capture_targets(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> AppResult<Vec<MemoryToolScope>> {
        let bound = WriteBound::start("memory.capture.targets", policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut connection = bound.acquire(self.pool()).await?;
                let mut tx = bound.begin(&mut connection).await?;
                let targets = Box::pin(capture_targets_on(&mut tx, claim, &bound))
                    .await
                    .map(End::Rollback);
                bound.end(tx, targets).await
            }))
            .await;
        finished(&bound, attempt)
    }

    async fn revalidation(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> AppResult<Revalidation> {
        let bound = WriteBound::start("memory.recall.revalidation", policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut connection = bound.acquire(self.pool()).await?;
                let mut tx = bound.begin(&mut connection).await?;
                let scopes = Box::pin(scopes_on(self, &mut tx, claim, &bound))
                    .await
                    .map(End::Rollback);
                bound.end(tx, scopes).await
            }))
            .await;
        finished(&bound, attempt)
    }
}

fn finished<T>(bound: &WriteBound, attempt: Attempt<T>) -> AppResult<T> {
    match attempt {
        Attempt::Done(result) => Ok(result),
        Attempt::Failed(error) => Err(error),
        Attempt::Ambiguous => Err(bound.unresolved()),
    }
}

/// The plan read (E7): the preparation's own authorization and candidate, then whether recall
/// applies, the ceiling, the eligible ready scopes, the budget and the query.
async fn plan_on(
    persistence: &PostgresPersistence,
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<RecallPlan> {
    let candidate = Box::pin(preparation::candidate_on(tx, claim, bound)).await?;
    if candidate.context.is_none() || !candidate.prepared.binds_memory() {
        return Ok(RecallPlan::NotApplicable);
    }
    let fence = claim.fence;
    let frozen: Option<bool> = sqlx::query_scalar(
        "SELECT exchange.frozen_preparation IS NOT NULL FROM agent_history_exchanges AS exchange \
         WHERE exchange.company_id=$1 AND exchange.run_id=$2 AND exchange.execution_id=$3",
    )
    .bind(fence.scope.company.as_uuid())
    .bind(fence.scope.run.as_uuid())
    .bind(fence.scope.execution.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    if frozen == Some(true) {
        return Ok(RecallPlan::Frozen);
    }
    if !crate::adapters::persistence::memory::memory_enabled_on(tx, fence.scope.company).await? {
        return Ok(RecallPlan::NotApplicable);
    }
    let read_ceiling = super::super::memory_steps::read_ceiling_for(tx, fence).await?;
    let (scopes, not_ready) = ready_scopes_on(persistence, tx, fence, &candidate.prepared).await?;
    let Some(scopes) = scopes else {
        return Ok(RecallPlan::NotApplicable);
    };
    Ok(RecallPlan::Needed(RecallRequest {
        read_ceiling,
        scopes,
        not_ready,
        max_tokens: candidate.prepared.recall_tokens()?,
        query: query_on(tx, fence, &candidate.prepared).await?,
    }))
}

/// The eligible scopes (E2/U4) a switched-on capture policy of the selected agent targets and
/// whose namespace is not ready: `threads` → the agent scope, `private_sessions` → the user
/// scope (present only for the actor's own manual root in an unattached session).
async fn capture_targets_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<Vec<MemoryToolScope>> {
    let candidate = Box::pin(preparation::candidate_on(tx, claim, bound)).await?;
    if candidate.context.is_none() {
        return Ok(Vec::new());
    }
    let fence = claim.fence;
    let agent = candidate.prepared.selection().agent().agent_id.as_uuid();
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT policy.kind FROM memory_agent_capture AS policy \
         JOIN memory_tenants AS tenant ON tenant.company_id = policy.company_id \
           AND tenant.state = 'enabled' \
         WHERE policy.company_id = $1 AND policy.agent_id = $2 AND policy.enabled",
    )
    .bind(fence.scope.company.as_uuid())
    .bind(agent)
    .fetch_all(&mut **tx)
    .await?;
    if kinds.is_empty() {
        return Ok(Vec::new());
    }
    let mut targets = Vec::new();
    for (scope, owner) in checkpoints::memory_tools::eligible_on(tx, fence, agent).await? {
        let wanted = match scope {
            MemoryToolScope::Agent => kinds.iter().any(|kind| kind == "threads"),
            MemoryToolScope::User => kinds.iter().any(|kind| kind == "private_sessions"),
            MemoryToolScope::Company => false,
        };
        if wanted && !namespace_ready_on(tx, fence.scope.company, owner).await? {
            targets.push(scope);
        }
    }
    Ok(targets)
}

/// Whether `owner`'s live namespace is ready: its state only, no credential is opened.
async fn namespace_ready_on(
    tx: &mut PgConnection,
    company: crate::domain::workflow::CompanyId,
    owner: crate::application::memory::NamespaceOwner,
) -> AppResult<bool> {
    use crate::application::memory::NamespaceOwner;
    let (kind, agent, user) = match owner {
        NamespaceOwner::Company => ("company", None, None),
        NamespaceOwner::Agent(agent) => ("agent", Some(agent), None),
        NamespaceOwner::User(user) => ("user", None, Some(user)),
    };
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM memory_namespaces AS namespace \
           WHERE namespace.company_id = $1 AND namespace.scope_kind = $2 \
             AND namespace.agent_id IS NOT DISTINCT FROM $3 \
             AND namespace.user_id IS NOT DISTINCT FROM $4 AND namespace.state = 'ready')",
    )
    .bind(company.as_uuid())
    .bind(kind)
    .bind(agent)
    .bind(user)
    .fetch_one(&mut *tx)
    .await?)
}

/// The ready namespaces of the run's eligible scopes (E2/U4) with their credentials, and the
/// eligible ones not ready yet. `None` when memory is no longer enabled.
async fn ready_scopes_on(
    persistence: &PostgresPersistence,
    tx: &mut PgConnection,
    fence: WorkflowFence,
    prepared: &PreparedAgent,
) -> AppResult<(Option<Vec<ScopeAccess>>, Vec<MemoryToolScope>)> {
    let agent = prepared.selection().agent().agent_id.as_uuid();
    let mut scopes = Vec::new();
    let mut not_ready = Vec::new();
    for (scope, owner) in checkpoints::memory_tools::eligible_on(tx, fence, agent).await? {
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
            super::super::memory_steps::Namespace::NotEnabled => return Ok((None, not_ready)),
        }
    }
    Ok((Some(scopes), not_ready))
}

/// The trigger message's text, else the current input's, cut at a character boundary; none
/// (recency) when both are blank.
async fn query_on(
    tx: &mut PgConnection,
    fence: WorkflowFence,
    prepared: &PreparedAgent,
) -> AppResult<Option<String>> {
    let sources: Vec<String> = sqlx::query_scalar(
        "SELECT admission.source_key FROM workflow_admissions AS admission \
         WHERE admission.company_id=$1 AND admission.run_id=$2 LIMIT 2",
    )
    .bind(fence.scope.company.as_uuid())
    .bind(fence.scope.run.as_uuid())
    .fetch_all(&mut *tx)
    .await?;
    let message = match sources.as_slice() {
        [source] => super::super::admission_binding::SourceKey::message(source),
        _ => None,
    };
    let trigger: Option<String> = match message {
        Some(message) => {
            sqlx::query_scalar(
                "SELECT message.clean_text_body FROM messages AS message \
                 WHERE message.company_id=$1 AND message.id=$2",
            )
            .bind(fence.scope.company.as_uuid())
            .bind(message)
            .fetch_optional(&mut *tx)
            .await?
        }
        None => None,
    };
    let current = match prepared.logical_input().value() {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    Ok([trigger.unwrap_or_default(), current]
        .into_iter()
        .map(|text| text.trim().chars().take(QUERY_CHARS).collect::<String>())
        .find(|text| !text.trim().is_empty()))
}

/// Freezes what a fresh attempt fetched (E7), in the preparation's transaction, before history
/// selection: a changed ceiling discards the packs unseen; otherwise their exposure must fit,
/// the run is raised to it and the items are recorded as consumed.
pub(super) async fn freeze_memory_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    fetched: FetchedRecall,
) -> AppResult<FrozenMemory> {
    let ceiling = super::super::memory_steps::read_ceiling_for(db, fence).await?;
    if fetched.read_ceiling() != ceiling {
        return Ok(FrozenMemory::without_items(
            RecallState::Unavailable {
                code: "ceiling_changed".into(),
            },
            ceiling,
        ));
    }
    let packs = match fetched {
        FetchedRecall::Skipped { state, .. } => {
            return Ok(FrozenMemory::without_items(state, ceiling));
        }
        FetchedRecall::Packs { packs, .. } => packs,
    };
    let level = |value: u8| AccessLevel::try_from(i16::from(value)).map_err(|_| invalid());
    let mut memory = FrozenMemory::without_items(RecallState::Included, ceiling);
    for (scope, pack) in packs {
        memory.exposed_level = memory.exposed_level.max(level(pack.exposed_level)?);
        for item in pack.items {
            let access_level = level(item.access_level)?;
            memory.exposed_level = memory.exposed_level.max(access_level);
            memory.items.push(RecalledMemory {
                scope,
                observation_id: item.observation_id,
                kind: item.kind,
                title: item.title,
                text: item.text,
                occurred_at: item.occurred_at,
                access_level,
                revision: item.revision,
                content_sha256: item.content_sha256,
                sources_sha256: item.sources_sha256,
            });
        }
        memory.digests.push(pack.pack_digest);
        memory.valid_until = match (memory.valid_until, pack.valid_until) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
    }
    if memory.exposed_level > ceiling {
        return Err(AppError::KnowledgeRelease {
            content: memory.exposed_level,
            destination: ceiling,
        });
    }
    let scope = fence.scope;
    super::super::knowledge::raise_content_level_on(
        db,
        scope.company,
        scope.run,
        memory.exposed_level,
    )
    .await?;
    let refs: Vec<MemoryRecallRef> = memory.items.iter().map(MemoryRecallRef::from).collect();
    sqlx::query(
        "INSERT INTO workflow_memory_consumptions (company_id,run_id,execution_id,job_id, \
           attempt_number,invocation_id,read_ceiling,exposed_level,items) \
         VALUES ($1,$2,$3,$4,$5,'00000000-0000-0000-0000-000000000000',$6::smallint,$7::smallint,$8) \
         ON CONFLICT (company_id,run_id,execution_id,job_id,attempt_number,invocation_id) \
         DO UPDATE SET read_ceiling=EXCLUDED.read_ceiling, exposed_level=EXCLUDED.exposed_level, \
           items=EXCLUDED.items",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .bind(fence.attempt.0)
    .bind(ceiling.as_i16())
    .bind(memory.exposed_level.as_i16())
    .bind(serde_json::to_value(&refs).map_err(|_| invalid())?)
    .execute(&mut *db)
    .await?;
    Ok(memory)
}

/// The current ceiling, ready scopes and recorded tool items, read through the preparation's
/// own authorization. Bounded: one consumption row per tool invocation (the checkpoint's
/// `tool_calls` policy caps those), at most 64 items each.
async fn scopes_on(
    persistence: &PostgresPersistence,
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    bound: &WriteBound,
) -> AppResult<Revalidation> {
    let candidate = Box::pin(preparation::candidate_on(tx, claim, bound)).await?;
    let read_ceiling = super::super::memory_steps::read_ceiling_for(tx, claim.fence).await?;
    let scope = claim.fence.scope;
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT consumption.items FROM workflow_memory_consumptions AS consumption \
         WHERE consumption.company_id=$1 AND consumption.run_id=$2 \
           AND consumption.execution_id=$3 AND consumption.job_id=$4 \
           AND consumption.invocation_id <> '00000000-0000-0000-0000-000000000000'",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .fetch_all(&mut **tx)
    .await?;
    let mut tool_items = Vec::new();
    for items in rows {
        let items: Vec<MemoryRecallRef> = serde_json::from_value(items).map_err(|_| invalid())?;
        tool_items.extend(items);
    }
    if tool_items.is_empty() && !candidate.prepared.binds_memory() {
        return Ok(Revalidation {
            read_ceiling,
            scopes: Vec::new(),
            tool_items,
        });
    }
    let (scopes, _) = ready_scopes_on(persistence, tx, claim.fence, &candidate.prepared).await?;
    Ok(Revalidation {
        read_ceiling,
        scopes: scopes.unwrap_or_default(),
        tool_items,
    })
}
