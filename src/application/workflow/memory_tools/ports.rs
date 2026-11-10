use super::{MemoryTool, memory_tool_schemas, trusted_memory_tool};
use crate::{
    app_error::{AppError, AppResult},
    application::{
        memory::{
            ClientCredential, ExternalSourceRef, MemoryDetail, MemoryHit, NamespaceExternalId,
        },
        workflow::{
            actions::ModelToolCallId,
            agent::{AgentToolObservation, AgentToolTarget, PreparedTool},
            lease::WorkflowFence,
            publication::ResolvedAgent,
        },
    },
    entities::access_level::AccessLevel,
    services::harness::runs::{InvocationId, InvocationState, RunCheckpoint, SavedInvocation},
};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

pub struct MemoryToolCall {
    pub origin: crate::application::agent::ToolCallOrigin,
    pub tool: MemoryTool,
    pub call: ModelToolCallId,
    pub arguments: Value,
}

impl MemoryToolCall {
    pub fn invocation(&self) -> AppResult<InvocationId> {
        Uuid::parse_str(self.call.as_str())
            .map(InvocationId)
            .map_err(|_| missing())
    }
}

/// A memory scope a tool may name. Which ones a run may use is decided by the host (E2).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum MemoryToolScope {
    Company,
    Agent,
    User,
}

impl MemoryToolScope {
    pub const ALL: [Self; 3] = [Self::Company, Self::Agent, Self::User];

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "company" => Some(Self::Company),
            "agent" => Some(Self::Agent),
            "user" => Some(Self::User),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Company => "company",
            Self::Agent => "agent",
            Self::User => "user",
        }
    }
}

/// The facts of a run's root that decide whether it may reach the run actor's private (user)
/// memory (E2). Only a trusted persistence read produces them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserScopeRoot {
    pub actor: Uuid,
    /// A person's manual start, not a message, schedule or child admission.
    pub manual_start: bool,
    /// The creator of the session the root is bound to (every admission binds one).
    pub session_creator: Uuid,
    /// That session belongs to a channel or thread (its context can include others' content).
    pub session_attached: bool,
}

/// The user whose namespace a run may use (E2 with U4): the actor of a root that is their own
/// manual start in an unattached session they created, for that root and every child of it run
/// as the same actor. Who published the workflow, configured the binding or wrote the agent is
/// deliberately not a fact (U4, 2026-11-09: kept and documented).
pub fn user_scope_owner(run_actor: Uuid, root: Option<&UserScopeRoot>) -> Option<Uuid> {
    let root = root?;
    (root.manual_start
        && !root.session_attached
        && root.session_creator == root.actor
        && root.actor == run_actor)
        .then_some(run_actor)
}

/// One namespace the run may read or write, with its own client credential.
pub struct ScopeAccess {
    pub scope: MemoryToolScope,
    pub namespace: NamespaceExternalId,
    pub credential: ClientCredential,
}

/// What the host authorized for one pending tool call (nothing committed).
pub struct MemoryToolAccess {
    /// `min(run ceiling, current destination, grant)` (E3); never a model-supplied value.
    pub read_ceiling: AccessLevel,
    /// The ready namespaces of every eligible scope, in `MemoryToolScope` order. A scope the
    /// run may not use is absent, never replaced by a wider one (E2).
    pub scopes: Vec<ScopeAccess>,
    /// Eligible scopes whose namespace is not provisioned yet; the caller provisions them in
    /// their own transaction (P7b obligation).
    pub not_ready: Vec<MemoryToolScope>,
}

pub enum MemoryToolContext {
    /// A completed invocation: replay its committed result exactly.
    Recorded(Value),
    Pending(MemoryToolAccess),
    /// Memory is not enabled for the company (now or any more): a controlled tool error.
    NotEnabled,
}

/// A read's answer, checked against the ceiling it was authorized at before it is recorded.
pub struct MemoryReadObservation {
    pub result: Value,
    /// The highest level of everything in `result`, provenance included (`carried_level`).
    pub exposed: AccessLevel,
    pub read_ceiling: AccessLevel,
    /// Every observation `result` shows the model, in result order.
    pub items: Vec<MemoryRecallRef>,
}

/// What a consumption row records about one exposed observation. A later tool save in the
/// same execution cites these as its `external_sources` (E5).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecallRef {
    pub scope: MemoryToolScope,
    pub id: Uuid,
    pub revision: i64,
    pub access_level: AccessLevel,
    /// Present for a whole body (details) and a recalled pack item; a snippet carries none.
    pub content_sha256: Option<String>,
    /// Present for a recalled pack item (E7: what `context.validate` checks).
    pub sources_sha256: Option<String>,
}

impl MemoryRecallRef {
    pub fn hit(scope: MemoryToolScope, hit: &MemoryHit) -> AppResult<Self> {
        Ok(Self {
            scope,
            id: hit.observation_id,
            revision: hit.revision,
            access_level: service_level(hit.access_level)?,
            content_sha256: None,
            sources_sha256: None,
        })
    }

    pub fn detail(scope: MemoryToolScope, detail: &MemoryDetail) -> AppResult<Self> {
        Ok(Self {
            scope,
            id: detail.observation_id,
            revision: detail.revision,
            access_level: service_level(detail.access_level)?,
            content_sha256: Some(detail.content_sha256.clone()),
            sources_sha256: None,
        })
    }
}

impl From<&crate::application::workflow::agent::RecalledMemory> for MemoryRecallRef {
    fn from(item: &crate::application::workflow::agent::RecalledMemory) -> Self {
        Self {
            scope: item.scope,
            id: item.observation_id,
            revision: item.revision,
            access_level: item.access_level,
            content_sha256: Some(item.content_sha256.clone()),
            sources_sha256: Some(item.sources_sha256.clone()),
        }
    }
}

/// A level the memory service returned; outside 0–9 it is a protocol fault, never clamped.
pub fn service_level(level: u8) -> AppResult<AccessLevel> {
    AccessLevel::try_from(i16::from(level))
        .map_err(|_| AppError::Internal("The memory service returned an invalid level".into()))
}

/// The provenance a tool save cites (E5): the distinct observations this execution recalled,
/// highest level first then by id, at most `MAX_SAVE_SOURCES`. Provenance only: the run's
/// watermark already covers every level.
pub fn save_sources(recalled: impl IntoIterator<Item = MemoryRecallRef>) -> Vec<ExternalSourceRef> {
    let mut distinct: std::collections::BTreeMap<(MemoryToolScope, Uuid), AccessLevel> =
        std::collections::BTreeMap::new();
    for item in recalled {
        let level = distinct
            .entry((item.scope, item.id))
            .or_insert(item.access_level);
        *level = (*level).max(item.access_level);
    }
    let mut sources: Vec<_> = distinct.into_iter().collect();
    sources.sort_by(|((_, left), left_level), ((_, right), right_level)| {
        right_level.cmp(left_level).then(left.cmp(right))
    });
    sources
        .into_iter()
        .take(MAX_SAVE_SOURCES)
        .map(|((scope, id), level)| ExternalSourceRef {
            reference: format!("memory:{}:{}", scope.label(), id.hyphenated()),
            access_level: level.as_i16() as u8,
        })
        .collect()
}

/// The service's bound on a save's `external_sources`.
pub const MAX_SAVE_SOURCES: usize = 64;

pub enum MemorySaveOutcome {
    Accepted(Value),
    NotEnabled,
    NotReady(MemoryToolScope),
    ScopeNotAllowed,
}

#[async_trait]
pub trait MemoryToolPersistence: Send + Sync {
    async fn memory_tool_context(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
    ) -> AppResult<MemoryToolContext>;

    /// One transaction: the invocation result, the monotone classification rise and the
    /// consumption record commit together, before the result can reach the model (E6).
    async fn observe_memory_read(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        observation: MemoryReadObservation,
    ) -> AppResult<Value>;

    /// One transaction: the frozen save request and its delivery intent under the run's
    /// classification lock, and the receipt as the invocation result (E5/E6).
    async fn save_memory(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        scope: MemoryToolScope,
        facts: Vec<String>,
    ) -> AppResult<MemorySaveOutcome>;

    /// Records a controlled refusal (invalid input, not ready, service refusal) as the
    /// invocation result. It exposes nothing, so it raises no level.
    async fn refuse_memory_tool(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        result: Value,
    ) -> AppResult<Value>;

    /// Lazy provisioning in its own transaction, never inside the run's (P7b obligation).
    async fn provision_memory_scope(
        &self,
        fence: WorkflowFence,
        scope: MemoryToolScope,
    ) -> AppResult<()>;
}

#[async_trait]
pub trait MemoryToolDispatch: Send + Sync {
    async fn call_memory_tool(
        &self,
        fence: WorkflowFence,
        call: MemoryToolCall,
    ) -> AppResult<AgentToolObservation>;
}

/// The saved invocation of a trusted memory tool call, checked against the run's actual
/// frozen selection (as `require_knowledge_invocation` does for knowledge search).
pub fn require_memory_invocation<'a>(
    fence: WorkflowFence,
    selection: &ResolvedAgent,
    tools: &[PreparedTool],
    checkpoint: &'a RunCheckpoint,
    call: &MemoryToolCall,
) -> AppResult<&'a SavedInvocation> {
    if selection.company_id() != fence.scope.company
        || checkpoint.identity.company_id != fence.scope.company.as_uuid()
        || checkpoint.identity.task_id != fence.scope.job.0
        || checkpoint.identity.agent_id != selection.agent().agent_id.as_uuid()
    {
        return Err(missing());
    }
    let id = call.invocation()?;
    let saved = checkpoint
        .invocations
        .iter()
        .find(|item| item.call.invocation_id == id && item.call.arguments == call.arguments)
        .ok_or_else(missing)?;
    let matching: Vec<_> = tools
        .iter()
        .filter(|tool| tool.declared.as_str() == saved.call.tool_id.as_str())
        .collect();
    let [tool] = matching.as_slice() else {
        return Err(missing());
    };
    if call.origin != crate::application::agent::ToolCallOrigin::Agent(tool.origin)
        || tool.target != (AgentToolTarget::Memory { tool: call.tool })
        || tool.name.as_str() != call.tool.name()
        || tool.input_schema != memory_tool_schemas(call.tool).input
    {
        return Err(missing());
    }
    let snapshot = selection
        .tools()
        .iter()
        .find(|entry| entry.binding.id == tool.origin.binding)
        .ok_or_else(missing)?;
    let expected =
        crate::application::workflow::agent_contract::AgentSelectionIdentity::from_selection(
            selection,
        )
        .tool_origin(snapshot.binding.id);
    if tool.origin != expected
        || !matches!(
            snapshot.binding.source,
            crate::entities::agent_capability::AgentBindingSource::Builtin { tool }
                if tool == call.tool.builtin()
        )
    {
        return Err(missing());
    }
    let contract = crate::application::workflow::agent_contract::agent_action_snapshot(
        selection.company_id(),
        snapshot,
    )?;
    if trusted_memory_tool(&contract)? != Some(call.tool) {
        return Err(missing());
    }
    match saved.state {
        InvocationState::Completed if saved.result.is_some() => Ok(saved),
        InvocationState::Prepared | InvocationState::Ready | InvocationState::Indeterminate
            if checkpoint
                .pending_call()
                .is_some_and(|pending| pending.invocation_id == id) =>
        {
            Ok(saved)
        }
        _ => Err(missing()),
    }
}

pub(super) fn missing() -> AppError {
    AppError::NotFound("Memory tool invocation".into())
}
