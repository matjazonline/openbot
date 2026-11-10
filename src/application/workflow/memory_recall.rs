//! Automatic recall at agent preparation (phase 05 E7). A rolled-back plan read decides whether
//! recall applies; the packs are fetched outside any transaction, as one interactive operation
//! under the recall deadline; the preparation freezes them (or discards them unseen). A resumed
//! attempt revalidates its frozen pack once, before its first provider entry.
use crate::{
    app_error::{AppError, AppResult},
    application::{
        memory::{
            MemoryClient, MemoryPack, MemoryPackRequest, MemoryServiceError, PackItemStatus,
            PackedRevision, ScopedPack,
        },
        workflow::{
            agent::{
                FrozenMemory, MAX_RECALL_BYTES, MIN_RECALL_TOKENS, PreparedAgent, RecallState,
            },
            lease::{ClaimedWorkflow, LeasePolicy, WorkflowFence},
            memory_tools::{MemoryRecallRef, MemoryToolScope, ScopeAccess},
        },
    },
    entities::access_level::AccessLevel,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Whether this attempt recalls, and with what (a rolled-back read; nothing is held).
pub enum RecallPlan {
    /// Not `agent.run`, no history exchange, the agent does not bind `search_memory`, or memory
    /// is not enabled.
    NotApplicable,
    /// An earlier attempt froze this execution's preparation: replay it, then revalidate.
    Frozen,
    Needed(RecallRequest),
}

pub struct RecallRequest {
    /// `min(run ceiling, current destination, grant)` (E3), read with the preparation's own
    /// authority; the freeze re-reads it and discards the packs unseen if it changed.
    pub read_ceiling: AccessLevel,
    /// The ready namespaces of every eligible scope (E2/U4), in scope order.
    pub scopes: Vec<ScopeAccess>,
    /// Eligible scopes not provisioned yet; requested in their own transaction.
    pub not_ready: Vec<MemoryToolScope>,
    /// `RecalledMemory` budget for the whole recall (E7).
    pub max_tokens: u32,
    /// The trigger message, else the current input, cut to 1,024 characters; none = recency.
    pub query: Option<String>,
}

/// What a fresh attempt fetched, frozen by the preparation unless it already has a freeze.
pub enum FetchedRecall {
    Packs {
        read_ceiling: AccessLevel,
        packs: Vec<(MemoryToolScope, MemoryPack)>,
    },
    Skipped {
        read_ceiling: AccessLevel,
        state: RecallState,
    },
}

impl FetchedRecall {
    pub fn read_ceiling(&self) -> AccessLevel {
        match self {
            Self::Packs { read_ceiling, .. } | Self::Skipped { read_ceiling, .. } => *read_ceiling,
        }
    }
}

/// The recall step of one claimed attempt.
pub enum RecallStep {
    /// Recall does not apply.
    None,
    /// A fresh preparation: freeze what was fetched.
    Fresh(FetchedRecall),
    /// The preparation was frozen by an earlier attempt: validate before provider entry.
    Resumed,
}

/// How a resumed attempt's frozen pack stands now (E7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallValidation {
    Valid,
    /// Changed, expired, unavailable or refused: terminal `memory.context_revoked`.
    Revoked,
    /// The service could not answer: retryable, the attempt consumed.
    Unavailable,
}

#[async_trait]
pub trait MemoryRecallPersistence: Send + Sync {
    async fn recall_plan(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> AppResult<RecallPlan>;

    /// Lazy provisioning of an eligible scope, in its own transaction (P7b obligation).
    async fn provision_recall_scope(
        &self,
        fence: WorkflowFence,
        scope: MemoryToolScope,
    ) -> AppResult<()>;

    /// Capture targets (E8) whose namespace is not ready yet: the agent scope while its
    /// `threads` switch is on, and the run's eligible user scope while its `private_sessions`
    /// switch is on. Capture freezes only into a ready namespace and cannot provision inside
    /// the accepting transaction, so preparation requests them here.
    async fn capture_targets(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> AppResult<Vec<MemoryToolScope>>;

    /// What a resumed attempt revalidates against: the run's current read ceiling, the ready
    /// namespaces of its eligible scopes (a scope missing now is absent), and every memory item
    /// this execution's tool reads showed the model in earlier attempts.
    async fn revalidation(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> AppResult<Revalidation>;
}

pub struct Revalidation {
    pub read_ceiling: AccessLevel,
    pub scopes: Vec<ScopeAccess>,
    pub tool_items: Vec<MemoryRecallRef>,
}

/// What the agent runtime's memory composition contributes to a fresh preparation.
#[derive(Default)]
pub struct MemoryPreparation {
    /// What automatic recall fetched for this attempt (E7).
    pub recall: Option<FetchedRecall>,
    /// The memory service is configured for this runtime, so an accepted exchange may be
    /// captured (E8, U7). Frozen with the preparation.
    pub capture: bool,
}

pub struct MemoryRecall {
    persistence: Arc<dyn MemoryRecallPersistence>,
    /// Absent while the memory service is unconfigured: nothing is recalled, and a resumed
    /// attempt carrying earlier memory cannot be revalidated, so it answers unavailable (U5).
    client: Option<Arc<dyn MemoryClient>>,
    monitoring: Option<Arc<dyn crate::domain::monitoring::MonitoringService>>,
}

impl MemoryRecall {
    /// Count recall outcomes and omissions (p06 A2-4).
    pub fn with_monitoring(
        mut self,
        monitoring: Arc<dyn crate::domain::monitoring::MonitoringService>,
    ) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    pub fn new(
        persistence: Arc<dyn MemoryRecallPersistence>,
        client: Arc<dyn MemoryClient>,
    ) -> Self {
        Self {
            persistence,
            client: Some(client),
            monitoring: None,
        }
    }

    /// Whether the memory service is configured for this runtime (it arms capture, U7).
    pub fn configured(&self) -> bool {
        self.client.is_some()
    }

    /// The guard an unconfigured deployment composes: no recall, and no replay of earlier
    /// memory it cannot revalidate.
    pub fn unconfigured(persistence: Arc<dyn MemoryRecallPersistence>) -> Self {
        Self {
            persistence,
            client: None,
            monitoring: None,
        }
    }

    /// Plan, then fetch outside any transaction. Optional failures (transport, capacity, not
    /// ready, deadline) become an `unavailable` freeze and the run continues; authorization
    /// failures fail closed.
    pub async fn step(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> AppResult<RecallStep> {
        let scope = claim.fence.scope;
        let step = crate::application::memory::with_correlation(
            crate::application::memory::read_correlation(
                scope.run.as_uuid(),
                scope.execution.as_uuid(),
            ),
            self.step_reads(claim, policy),
        )
        .await;
        if let (Some(monitoring), Ok(step)) = (&self.monitoring, &step) {
            let outcome = match step {
                RecallStep::None => "none",
                RecallStep::Resumed => "resumed",
                RecallStep::Fresh(FetchedRecall::Packs { packs, .. }) => {
                    let omitted: u64 = packs.iter().map(|(_, pack)| u64::from(pack.omitted)).sum();
                    monitoring.increment_counter("memory_recall_omitted_total", omitted, &[]);
                    "recalled"
                }
                RecallStep::Fresh(FetchedRecall::Skipped { .. }) => "skipped",
            };
            monitoring.increment_counter("memory_recall_total", 1, &[("outcome", outcome)]);
        }
        step
    }

    async fn step_reads(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> AppResult<RecallStep> {
        let Some(client) = &self.client else {
            return Ok(RecallStep::None);
        };
        // Best effort: a failed capture-target request never fails the run (E8).
        match self.persistence.capture_targets(claim, policy).await {
            Ok(scopes) => self.provision(claim, &scopes).await,
            Err(error) => {
                tracing::warn!(error = %error, "Memory capture target lookup failed");
            }
        }
        let request = match self.persistence.recall_plan(claim, policy).await? {
            RecallPlan::NotApplicable => return Ok(RecallStep::None),
            RecallPlan::Frozen => return Ok(RecallStep::Resumed),
            RecallPlan::Needed(request) => request,
        };
        self.provision(claim, &request.not_ready).await;
        let skipped = |state| {
            Ok(RecallStep::Fresh(FetchedRecall::Skipped {
                read_ceiling: request.read_ceiling,
                state,
            }))
        };
        if request.max_tokens < MIN_RECALL_TOKENS {
            return skipped(RecallState::SkippedBudget);
        }
        if request.scopes.is_empty() {
            return skipped(RecallState::Unavailable {
                code: "not_ready".into(),
            });
        }
        let ceiling = request.read_ceiling.as_i16() as u8;
        let packs: Vec<ScopedPack<'_>> = request
            .scopes
            .iter()
            .map(|entry| ScopedPack {
                credential: &entry.credential,
                pack: MemoryPackRequest {
                    namespace: entry.namespace,
                    read_ceiling: ceiling,
                    query: request.query.clone(),
                    max_tokens: share(entry.scope, request.max_tokens),
                    max_bytes: MAX_RECALL_BYTES,
                },
            })
            .collect();
        match client.context_pack(&packs).await {
            Ok(answers) if answers.len() == packs.len() => {
                Ok(RecallStep::Fresh(FetchedRecall::Packs {
                    read_ceiling: request.read_ceiling,
                    packs: request
                        .scopes
                        .iter()
                        .map(|entry| entry.scope)
                        .zip(answers)
                        .collect(),
                }))
            }
            Ok(_) => Err(AppError::Internal(
                "The memory service answered a different number of packs".into(),
            )),
            Err(error) => match optional_failure(&error) {
                Some(code) => skipped(RecallState::Unavailable { code: code.into() }),
                None => Err(AppError::Forbidden(format!(
                    "Memory recall was refused: {error}"
                ))),
            },
        }
    }

    /// Requests each scope's namespace in its own transaction; a failed request is logged and
    /// never fails the run.
    async fn provision(&self, claim: &ClaimedWorkflow, scopes: &[MemoryToolScope]) {
        for scope in scopes {
            if let Err(error) = self
                .persistence
                .provision_recall_scope(claim.fence, *scope)
                .await
            {
                tracing::warn!(error = %error, "Memory namespace provisioning request failed");
            }
        }
    }

    /// Before a claimed attempt's first provider entry (E7): a resumed attempt revalidates its
    /// frozen pack, and every attempt revalidates the memory its earlier attempts' tool reads
    /// showed the model (both would be replayed). Nothing to check is valid; anything to check
    /// without a configured service is unavailable.
    pub async fn validate(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
        prepared: &PreparedAgent,
        resumed: bool,
    ) -> AppResult<RecallValidation> {
        let scope = claim.fence.scope;
        crate::application::memory::with_correlation(
            crate::application::memory::read_correlation(
                scope.run.as_uuid(),
                scope.execution.as_uuid(),
            ),
            self.validate_reads(claim, policy, prepared, resumed),
        )
        .await
    }

    async fn validate_reads(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
        prepared: &PreparedAgent,
        resumed: bool,
    ) -> AppResult<RecallValidation> {
        let pack = prepared
            .frozen_memory()
            .filter(|memory| !memory.items.is_empty())
            // Without a client nothing was frozen this attempt, so a pack is an earlier one's.
            .filter(|_| resumed || self.client.is_none());
        if pack.is_none() && !prepared.uses_memory_tools() {
            // Nothing earlier could be replayed: no extra read for agents without memory.
            return Ok(RecallValidation::Valid);
        }
        let facts = self.persistence.revalidation(claim, policy).await?;
        if pack.is_none() && facts.tool_items.is_empty() {
            return Ok(RecallValidation::Valid);
        }
        let Some(client) = &self.client else {
            return Ok(RecallValidation::Unavailable);
        };
        let exposed = pack
            .map(|memory| memory.exposed_level)
            .into_iter()
            .chain(facts.tool_items.iter().map(|item| item.access_level))
            .max()
            .unwrap_or(AccessLevel::PUBLIC);
        if facts.read_ceiling < exposed {
            return Ok(RecallValidation::Revoked);
        }
        if let Some(memory) = pack {
            let pack = validate_scopes(client.as_ref(), memory, facts.read_ceiling, &facts.scopes);
            match Box::pin(pack).await {
                RecallValidation::Valid => {}
                other => return Ok(other),
            }
        }
        Ok(Box::pin(validate_tool_items(
            client.as_ref(),
            &facts.tool_items,
            facts.read_ceiling,
            &facts.scopes,
        ))
        .await)
    }
}

/// Recorded tool items (E7, review MEM-P05-B4-F2): `context.validate` needs each item's
/// `sources_sha256`, which tool results do not carry, so a fetch under the current ceiling
/// stands in: an id now absent (deleted, purged or above the ceiling) or at another revision
/// revokes the attempt. Fetches go per scope, at most 64 ids each.
async fn validate_tool_items(
    client: &dyn MemoryClient,
    items: &[MemoryRecallRef],
    ceiling: AccessLevel,
    scopes: &[ScopeAccess],
) -> RecallValidation {
    for scope in MemoryToolScope::ALL {
        let mut wanted: Vec<(uuid::Uuid, i64)> = items
            .iter()
            .filter(|item| item.scope == scope)
            .map(|item| (item.id, item.revision))
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        if wanted.is_empty() {
            continue;
        }
        let Some(entry) = scopes.iter().find(|entry| entry.scope == scope) else {
            return RecallValidation::Revoked;
        };
        for chunk in wanted.chunks(64) {
            let ids: Vec<uuid::Uuid> = chunk.iter().map(|(id, _)| *id).collect();
            let fetched = match client
                .fetch(
                    &entry.credential,
                    entry.namespace,
                    ceiling.as_i16() as u8,
                    &ids,
                )
                .await
            {
                Ok(fetched) => fetched,
                Err(MemoryServiceError::NotFound) => return RecallValidation::Revoked,
                Err(error) if optional_failure(&error).is_some() => {
                    return RecallValidation::Unavailable;
                }
                Err(_) => return RecallValidation::Revoked,
            };
            let current = |id: uuid::Uuid, revision: i64| {
                fetched
                    .items
                    .iter()
                    .any(|item| item.observation_id == id && item.revision == revision)
            };
            if !chunk.iter().all(|(id, revision)| current(*id, *revision)) {
                return RecallValidation::Revoked;
            }
        }
    }
    RecallValidation::Valid
}

async fn validate_scopes(
    client: &dyn MemoryClient,
    memory: &FrozenMemory,
    ceiling: AccessLevel,
    scopes: &[ScopeAccess],
) -> RecallValidation {
    for scope in MemoryToolScope::ALL {
        let items: Vec<PackedRevision> = memory
            .items
            .iter()
            .filter(|item| item.scope == scope)
            .map(|item| PackedRevision {
                observation_id: item.observation_id,
                revision: item.revision,
                sources_sha256: item.sources_sha256.clone(),
            })
            .collect();
        if items.is_empty() {
            continue;
        }
        let Some(entry) = scopes.iter().find(|entry| entry.scope == scope) else {
            return RecallValidation::Revoked;
        };
        match client
            .context_validate(
                &entry.credential,
                entry.namespace,
                ceiling.as_i16() as u8,
                &items,
            )
            .await
        {
            Ok(statuses)
                if statuses
                    .iter()
                    .all(|status| *status == PackItemStatus::Valid) => {}
            Ok(_) => return RecallValidation::Revoked,
            // A namespace gone since the freeze revokes its items; only an outage is retryable.
            Err(MemoryServiceError::NotFound) => return RecallValidation::Revoked,
            Err(error) if optional_failure(&error).is_some() => {
                return RecallValidation::Unavailable;
            }
            Err(_) => return RecallValidation::Revoked,
        }
    }
    RecallValidation::Valid
}

/// A scope's share of the recall budget (E7): company 1/2, agent 1/4, user 1/4.
fn share(scope: MemoryToolScope, max_tokens: u32) -> u32 {
    match scope {
        MemoryToolScope::Company => max_tokens / 2,
        MemoryToolScope::Agent | MemoryToolScope::User => max_tokens / 4,
    }
    .max(1)
}

/// The failures that leave recall optional (the run continues without memory); every other
/// one is an authorization or contract failure and fails closed.
fn optional_failure(error: &MemoryServiceError) -> Option<&'static str> {
    match error {
        MemoryServiceError::Unavailable => Some("unavailable"),
        MemoryServiceError::Ambiguous => Some("deadline"),
        MemoryServiceError::Capacity { .. } => Some("capacity"),
        MemoryServiceError::NotFound => Some("not_ready"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "memory_recall_tests.rs"]
mod tests;
