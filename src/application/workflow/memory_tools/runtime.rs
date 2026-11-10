use super::*;
use crate::{
    app_error::{AppError, AppResult},
    application::{
        memory::{
            MemoryClient, MemoryDetail, MemoryHit, MemorySearch, MemoryServiceError,
            MemoryTimeline, ScopedSearch,
        },
        workflow::{agent::AgentToolObservation, lease::WorkflowFence, tools::tool_error},
    },
    entities::access_level::AccessLevel,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// The four memory tools, composed only while the memory service is configured. The service is
/// called outside any transaction; every answer is committed with its classification before it
/// is returned toward the model (E6).
pub struct MemoryToolRuntime {
    persistence: Arc<dyn MemoryToolPersistence>,
    client: Arc<dyn MemoryClient>,
}

impl MemoryToolRuntime {
    pub fn new(persistence: Arc<dyn MemoryToolPersistence>, client: Arc<dyn MemoryClient>) -> Self {
        Self {
            persistence,
            client,
        }
    }
}

#[async_trait]
impl MemoryToolDispatch for MemoryToolRuntime {
    async fn call_memory_tool(
        &self,
        fence: WorkflowFence,
        call: MemoryToolCall,
    ) -> AppResult<AgentToolObservation> {
        // p06 A2-2: the execution's reads share one correlation id with its recall.
        let correlation = crate::application::memory::read_correlation(
            fence.scope.run.as_uuid(),
            fence.scope.execution.as_uuid(),
        );
        crate::application::memory::with_correlation(correlation, self.call_tool_reads(fence, call))
            .await
    }
}

enum Answer {
    Observed(Value),
    Refused(Value),
}

impl MemoryToolRuntime {
    async fn call_tool_reads(
        &self,
        fence: WorkflowFence,
        call: MemoryToolCall,
    ) -> AppResult<AgentToolObservation> {
        let access = match self.persistence.memory_tool_context(fence, &call).await? {
            MemoryToolContext::Recorded(result) => {
                return Ok(AgentToolObservation::Completed(result));
            }
            MemoryToolContext::NotEnabled => {
                return self.refuse(fence, &call, not_enabled()).await;
            }
            MemoryToolContext::Pending(access) => access,
        };
        let outcome = match call.tool {
            MemoryTool::Search => self.search(fence, &call, access).await,
            MemoryTool::Timeline => self.timeline(fence, &call, access).await,
            MemoryTool::Details => self.details(fence, &call, access).await,
            MemoryTool::Save => self.save(fence, &call, &access).await,
        };
        match outcome {
            Ok(Answer::Observed(result)) => Ok(AgentToolObservation::Completed(result)),
            Ok(Answer::Refused(result)) => self.refuse(fence, &call, result).await,
            Err(error) => Err(error),
        }
    }

    async fn refuse(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        result: Value,
    ) -> AppResult<AgentToolObservation> {
        self.persistence
            .refuse_memory_tool(fence, call, result)
            .await
            .map(AgentToolObservation::Completed)
    }

    /// Eligible scopes the call names (all eligible when omitted); not-ready ones are
    /// provisioned in their own transaction and left out of this call.
    async fn chosen<'a>(
        &self,
        fence: WorkflowFence,
        access: &'a MemoryToolAccess,
        requested: Option<Vec<MemoryToolScope>>,
    ) -> Result<Vec<&'a ScopeAccess>, Value> {
        let wanted = |scope: MemoryToolScope| {
            requested
                .as_ref()
                .is_none_or(|requested| requested.contains(&scope))
        };
        for scope in access
            .not_ready
            .iter()
            .copied()
            .filter(|scope| wanted(*scope))
        {
            if let Err(error) = self.persistence.provision_memory_scope(fence, scope).await {
                tracing::warn!(error = %error, "Memory namespace provisioning request failed");
            }
        }
        let chosen: Vec<&ScopeAccess> = access
            .scopes
            .iter()
            .filter(|entry| wanted(entry.scope))
            .collect();
        if chosen.is_empty() {
            let any_not_ready = access.not_ready.iter().any(|scope| wanted(*scope));
            return Err(if any_not_ready {
                tool_error(
                    "memory.not_ready",
                    "Memory is being prepared for this scope; try again shortly",
                )
            } else {
                tool_error(
                    "memory.scope_unavailable",
                    "That memory scope is not available in this conversation",
                )
            });
        }
        Ok(chosen)
    }

    async fn search(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        access: MemoryToolAccess,
    ) -> AppResult<Answer> {
        let Some(input) = SearchInput::parse(&call.arguments) else {
            return Ok(Answer::Refused(invalid_input(call.tool)));
        };
        let chosen = match self.chosen(fence, &access, input.scopes.clone()).await {
            Ok(chosen) => chosen,
            Err(refusal) => return Ok(Answer::Refused(refusal)),
        };
        let ceiling = ceiling_u8(access.read_ceiling);
        // Every chosen scope in one interactive operation: one permit, concurrent requests (E1).
        let searches: Vec<ScopedSearch<'_>> = chosen
            .iter()
            .map(|entry| ScopedSearch {
                credential: &entry.credential,
                search: MemorySearch {
                    namespace: entry.namespace,
                    read_ceiling: ceiling,
                    query: input.query.clone(),
                    limit: input.limit,
                },
            })
            .collect();
        let results = match self.client.search_scopes(&searches).await {
            Ok(results) => results,
            Err(error) => return Ok(Answer::Refused(service_refusal(&error))),
        };
        if results.len() != chosen.len() {
            return Err(AppError::Internal(
                "The memory service answered a different number of searches".into(),
            ));
        }
        let answers: Vec<_> = chosen
            .iter()
            .map(|entry| entry.scope)
            .zip(results)
            .collect();
        // Interleave per scope so one scope cannot crowd out another, then keep `limit`.
        let mut merged: Vec<(MemoryToolScope, MemoryHit)> = Vec::new();
        let mut exposed = AccessLevel::PUBLIC;
        let mut queues: Vec<_> = answers
            .into_iter()
            .map(|(scope, result)| (scope, result.exposed_level, result.items.into_iter()))
            .collect();
        for (_, level, _) in &queues {
            exposed = exposed.max(service_level(*level)?);
        }
        while merged.len() < input.limit as usize {
            let mut progressed = false;
            for (scope, _, items) in queues.iter_mut() {
                if merged.len() == input.limit as usize {
                    break;
                }
                if let Some(hit) = items.next() {
                    merged.push((*scope, hit));
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        let exposed = merged.iter().try_fold(exposed, |level, (_, hit)| {
            service_level(hit.access_level).map(|item| level.max(item))
        })?;
        let (result, refs) = bounded_hits(&merged)?;
        self.observe(fence, call, &access, result, exposed, refs)
            .await
    }

    async fn timeline(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        access: MemoryToolAccess,
    ) -> AppResult<Answer> {
        let Some(input) = TimelineInput::parse(&call.arguments) else {
            return Ok(Answer::Refused(invalid_input(call.tool)));
        };
        let chosen = match self.chosen(fence, &access, Some(vec![input.scope])).await {
            Ok(chosen) => chosen,
            Err(refusal) => return Ok(Answer::Refused(refusal)),
        };
        let [entry] = chosen.as_slice() else {
            return Ok(Answer::Refused(invalid_input(call.tool)));
        };
        let answer = match self
            .client
            .timeline(
                &entry.credential,
                &MemoryTimeline {
                    namespace: entry.namespace,
                    read_ceiling: ceiling_u8(access.read_ceiling),
                    anchor: input.id,
                    before: input.before,
                    after: input.after,
                },
            )
            .await
        {
            Ok(answer) => answer,
            Err(MemoryServiceError::NotFound) => return Ok(Answer::Refused(not_found())),
            Err(error) => return Ok(Answer::Refused(service_refusal(&error))),
        };
        let exposed = answer
            .items
            .iter()
            .try_fold(service_level(answer.exposed_level)?, |level, hit| {
                service_level(hit.access_level).map(|item| level.max(item))
            })?;
        let hits: Vec<(MemoryToolScope, MemoryHit)> = answer
            .items
            .into_iter()
            .map(|hit| (entry.scope, hit))
            .collect();
        let (result, refs) = bounded_hits(&hits)?;
        self.observe(fence, call, &access, result, exposed, refs)
            .await
    }

    async fn details(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        access: MemoryToolAccess,
    ) -> AppResult<Answer> {
        let Some(input) = DetailsInput::parse(&call.arguments) else {
            return Ok(Answer::Refused(invalid_input(call.tool)));
        };
        let scopes: Vec<MemoryToolScope> = input.items.iter().map(|(scope, _)| *scope).collect();
        let chosen = match self.chosen(fence, &access, Some(scopes)).await {
            Ok(chosen) => chosen,
            Err(refusal) => return Ok(Answer::Refused(refusal)),
        };
        let ceiling = ceiling_u8(access.read_ceiling);
        let mut fetched: Vec<(MemoryToolScope, MemoryDetail)> = Vec::new();
        let mut exposed = AccessLevel::PUBLIC;
        for entry in &chosen {
            let ids: Vec<Uuid> = input
                .items
                .iter()
                .filter(|(scope, _)| *scope == entry.scope)
                .map(|(_, id)| *id)
                .collect();
            if ids.is_empty() {
                continue;
            }
            let answer = match self
                .client
                .fetch(&entry.credential, entry.namespace, ceiling, &ids)
                .await
            {
                Ok(answer) => answer,
                Err(error) => return Ok(Answer::Refused(service_refusal(&error))),
            };
            exposed = exposed.max(service_level(answer.exposed_level)?);
            for detail in answer.items {
                exposed = exposed.max(service_level(detail.access_level)?);
                fetched.push((entry.scope, detail));
            }
        }
        // Requested order; ids the reader may not see are simply absent (no existence oracle).
        fetched.sort_by_key(|(scope, detail)| {
            input
                .items
                .iter()
                .position(|item| *item == (*scope, detail.observation_id))
        });
        let max = result_bound();
        let mut kept: Vec<Value> = Vec::new();
        let mut refs = Vec::new();
        for (scope, detail) in &fetched {
            kept.push(detail_json(*scope, detail));
            let candidate = json!({"items": kept, "omitted": fetched.len() - kept.len()});
            if candidate.to_string().len() > max {
                kept.pop();
                break;
            }
            refs.push(MemoryRecallRef::detail(*scope, detail)?);
        }
        let omitted = fetched.len() - kept.len();
        let result = json!({"items": kept, "omitted": omitted});
        self.observe(fence, call, &access, result, exposed, refs)
            .await
    }

    async fn save(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        access: &MemoryToolAccess,
    ) -> AppResult<Answer> {
        let Some(input) = SaveInput::parse(&call.arguments) else {
            return Ok(Answer::Refused(invalid_input(call.tool)));
        };
        if !access.scopes.iter().any(|entry| entry.scope == input.scope)
            && !access.not_ready.contains(&input.scope)
        {
            return Ok(Answer::Refused(tool_error(
                "memory.scope_unavailable",
                "That memory scope is not available in this conversation",
            )));
        }
        match self
            .persistence
            .save_memory(fence, call, input.scope, input.facts)
            .await?
        {
            MemorySaveOutcome::Accepted(receipt) => Ok(Answer::Observed(receipt)),
            MemorySaveOutcome::NotEnabled => Ok(Answer::Refused(not_enabled())),
            MemorySaveOutcome::ScopeNotAllowed => Ok(Answer::Refused(tool_error(
                "memory.scope_unavailable",
                "That memory scope is not available in this conversation",
            ))),
            MemorySaveOutcome::NotReady(scope) => {
                if let Err(error) = self.persistence.provision_memory_scope(fence, scope).await {
                    tracing::warn!(error = %error, "Memory namespace provisioning request failed");
                }
                Ok(Answer::Refused(tool_error(
                    "memory.not_ready",
                    "Memory is being prepared for this scope; try again shortly",
                )))
            }
        }
    }

    async fn observe(
        &self,
        fence: WorkflowFence,
        call: &MemoryToolCall,
        access: &MemoryToolAccess,
        result: Value,
        exposed: AccessLevel,
        items: Vec<MemoryRecallRef>,
    ) -> AppResult<Answer> {
        let result = self
            .persistence
            .observe_memory_read(
                fence,
                call,
                MemoryReadObservation {
                    result,
                    exposed,
                    read_ceiling: access.read_ceiling,
                    items,
                },
            )
            .await?;
        Ok(Answer::Observed(result))
    }
}

struct SearchInput {
    query: String,
    scopes: Option<Vec<MemoryToolScope>>,
    limit: u32,
}

impl SearchInput {
    fn parse(arguments: &Value) -> Option<Self> {
        let object = arguments.as_object()?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "query" | "scopes" | "limit"))
        {
            return None;
        }
        let query = object.get("query")?.as_str()?;
        if query.trim().is_empty() || query.chars().count() as u64 > MAX_QUERY_CHARS {
            return None;
        }
        let scopes = match object.get("scopes") {
            None => None,
            Some(scopes) => {
                let mut parsed = Vec::new();
                for scope in scopes.as_array()? {
                    let scope = MemoryToolScope::parse(scope.as_str()?)?;
                    if parsed.contains(&scope) {
                        return None;
                    }
                    parsed.push(scope);
                }
                if parsed.is_empty() {
                    return None;
                }
                Some(parsed)
            }
        };
        let limit = match object.get("limit") {
            None => 5,
            Some(limit) => limit
                .as_u64()
                .filter(|limit| (1..=MAX_SEARCH_LIMIT).contains(limit))?,
        };
        Some(Self {
            query: query.to_owned(),
            scopes,
            limit: limit as u32,
        })
    }
}

struct TimelineInput {
    scope: MemoryToolScope,
    id: Uuid,
    before: u32,
    after: u32,
}

impl TimelineInput {
    fn parse(arguments: &Value) -> Option<Self> {
        let object = arguments.as_object()?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "scope" | "id" | "before" | "after"))
        {
            return None;
        }
        let side = |key: &str| match object.get(key) {
            None => Some(MAX_TIMELINE_SIDE),
            Some(value) => value.as_u64().filter(|side| *side <= MAX_TIMELINE_SIDE),
        };
        Some(Self {
            scope: MemoryToolScope::parse(object.get("scope")?.as_str()?)?,
            id: Uuid::parse_str(object.get("id")?.as_str()?).ok()?,
            before: side("before")? as u32,
            after: side("after")? as u32,
        })
    }
}

struct DetailsInput {
    items: Vec<(MemoryToolScope, Uuid)>,
}

impl DetailsInput {
    fn parse(arguments: &Value) -> Option<Self> {
        let object = arguments.as_object()?;
        if object.keys().any(|key| key != "items") {
            return None;
        }
        let raw = object.get("items")?.as_array()?;
        if raw.is_empty() || raw.len() as u64 > MAX_DETAILS {
            return None;
        }
        let mut items = Vec::new();
        for item in raw {
            let item = item.as_object()?;
            if item
                .keys()
                .any(|key| !matches!(key.as_str(), "scope" | "id"))
            {
                return None;
            }
            let entry = (
                MemoryToolScope::parse(item.get("scope")?.as_str()?)?,
                Uuid::parse_str(item.get("id")?.as_str()?).ok()?,
            );
            if !items.contains(&entry) {
                items.push(entry);
            }
        }
        Some(Self { items })
    }
}

struct SaveInput {
    scope: MemoryToolScope,
    facts: Vec<String>,
}

impl SaveInput {
    fn parse(arguments: &Value) -> Option<Self> {
        let object = arguments.as_object()?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "facts" | "scope"))
        {
            return None;
        }
        let raw = object.get("facts")?.as_array()?;
        if raw.is_empty() || raw.len() as u64 > MAX_FACTS {
            return None;
        }
        let mut facts = Vec::new();
        for fact in raw {
            let fact = fact.as_str()?;
            if fact.trim().is_empty() || fact.chars().count() as u64 > MAX_FACT_CHARS {
                return None;
            }
            facts.push(fact.to_owned());
        }
        let scope = match object.get("scope") {
            None => MemoryToolScope::Agent,
            Some(scope) => match MemoryToolScope::parse(scope.as_str()?)? {
                MemoryToolScope::Company => return None,
                scope => scope,
            },
        };
        Some(Self { scope, facts })
    }
}

fn hit_json(scope: MemoryToolScope, hit: &MemoryHit) -> Value {
    json!({"scope": scope.label(), "id": hit.observation_id, "revision": hit.revision,
        "kind": hit.kind, "title": hit.title, "snippet": hit.snippet,
        "occurred_at": hit.occurred_at.to_rfc3339(), "access_level": hit.access_level})
}

fn detail_json(scope: MemoryToolScope, detail: &MemoryDetail) -> Value {
    json!({"scope": scope.label(), "id": detail.observation_id, "revision": detail.revision,
        "kind": detail.kind, "title": detail.title, "text": detail.body,
        "occurred_at": detail.occurred_at.to_rfc3339(), "access_level": detail.access_level})
}

/// The hits that fit the tool-result bound, trailing ones dropped, with a reference for each
/// kept one. The run is still raised to the level of everything the service returned
/// (conservative, like `memory.load`).
fn bounded_hits(hits: &[(MemoryToolScope, MemoryHit)]) -> AppResult<(Value, Vec<MemoryRecallRef>)> {
    let max = result_bound();
    let mut items: Vec<Value> = hits
        .iter()
        .map(|(scope, hit)| hit_json(*scope, hit))
        .collect();
    let mut result = json!({"items": items});
    while result.to_string().len() > max && !items.is_empty() {
        items.pop();
        result = json!({"items": items});
    }
    let refs = hits[..items.len()]
        .iter()
        .map(|(scope, hit)| MemoryRecallRef::hit(*scope, hit))
        .collect::<AppResult<_>>()?;
    Ok((result, refs))
}

pub(super) fn result_bound() -> usize {
    crate::application::workflow::tools::MAX_TOOL_RESULT_BYTES
        .min(crate::services::harness::runs::MAX_RESULT_BYTES)
}

fn ceiling_u8(level: AccessLevel) -> u8 {
    level.as_i16() as u8
}

fn invalid_input(tool: MemoryTool) -> Value {
    tool_error(
        "memory.invalid_input",
        match tool {
            MemoryTool::Search => {
                "Give a query of 1–1024 characters, optional scopes from company, agent and user, and an optional limit of 1–10"
            }
            MemoryTool::Timeline => {
                "Give a scope, a memory id from search_memory, and optional before/after counts of 0–5"
            }
            MemoryTool::Details => "Give 1–4 items, each with a scope and a memory id",
            MemoryTool::Save => {
                "Give 1–8 facts of 1–4096 characters each, and optionally scope agent or user"
            }
        },
    )
}

fn not_enabled() -> Value {
    tool_error(
        "memory.not_enabled",
        "Memory is not enabled for this company",
    )
}

fn not_found() -> Value {
    tool_error("memory.not_found", "No memory with that id is available")
}

fn service_refusal(error: &MemoryServiceError) -> Value {
    match error {
        MemoryServiceError::NotFound => not_found(),
        MemoryServiceError::Unavailable
        | MemoryServiceError::Ambiguous
        | MemoryServiceError::Capacity { .. } => tool_error(
            "memory.unavailable",
            "Memory is temporarily unavailable; continue without it",
        ),
        _ => tool_error("memory.refused", "Memory refused this request"),
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
