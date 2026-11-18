use super::*;
use crate::{
    app_error::{AppError, AppResult},
    application::{
        memory::{
            MemoryClient, MemoryDetail, MemoryHit, MemorySearch, MemorySearchResult,
            MemoryServiceError, MemoryTimeline, NamespaceExternalId, ScopedSearch,
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
        let context = match self.persistence.memory_tool_context(fence, &call).await {
            Ok(context) => context,
            Err(AppError::BadRequest(code)) if code == "memory.scope_unavailable" => {
                return self.refuse(fence, &call, scope_unavailable()).await;
            }
            Err(error) => return Err(error),
        };
        let access = match context {
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
            MemoryTool::Save => self.save(fence, &call).await,
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
        for scope in access
            .not_ready
            .iter()
            .filter(|scope| scope.matches(requested.as_deref()))
        {
            if let Err(error) = self
                .persistence
                .provision_memory_scope(fence, scope.owner)
                .await
            {
                tracing::warn!(error = %error, "Memory namespace provisioning request failed");
            }
        }
        let chosen: Vec<&ScopeAccess> = access
            .scopes
            .iter()
            .filter(|entry| entry.scope.matches(requested.as_deref()))
            .collect();
        if chosen.is_empty() {
            let any_not_ready = access
                .not_ready
                .iter()
                .any(|scope| scope.matches(requested.as_deref()));
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
        let namespaces = chosen.iter().map(|entry| entry.namespace).collect();
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
        let answers: Vec<_> = chosen.iter().copied().zip(results).collect();
        let (merged, exposed) = merge_searches(answers, input.limit)?;
        let (result, refs) = bounded_hits(&merged)?;
        self.persistence
            .observe_memory_read(
                fence,
                call,
                MemoryReadObservation {
                    result,
                    namespaces,
                    exposed,
                    read_ceiling: access.read_ceiling,
                    items: refs,
                },
            )
            .await
            .map(Answer::Observed)
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
        let chosen = match self.chosen(fence, &access, input.scopes).await {
            Ok(chosen) => chosen,
            Err(refusal) => return Ok(Answer::Refused(refusal)),
        };
        let namespaces = chosen.iter().map(|entry| entry.namespace).collect();
        let mut hits = Vec::new();
        let mut exposed = AccessLevel::PUBLIC;
        for entry in chosen {
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
                Err(MemoryServiceError::NotFound) => continue,
                Err(error) => return Ok(Answer::Refused(service_refusal(&error))),
            };
            exposed = exposed.max(service_level(answer.exposed_level)?);
            for hit in answer.items {
                exposed = exposed.max(service_level(hit.access_level)?);
                hits.push(LabeledHit {
                    namespace: entry.namespace,
                    label: entry.scope.label.clone(),
                    hit,
                });
            }
        }
        if hits.is_empty() {
            return Ok(Answer::Refused(not_found()));
        }
        let (result, refs) = bounded_hits(&hits)?;
        self.persistence
            .observe_memory_read(
                fence,
                call,
                MemoryReadObservation {
                    result,
                    namespaces,
                    exposed,
                    read_ceiling: access.read_ceiling,
                    items: refs,
                },
            )
            .await
            .map(Answer::Observed)
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
        let chosen: Vec<_> = access
            .scopes
            .iter()
            .filter(|entry| {
                input
                    .items
                    .iter()
                    .any(|(namespace, _)| *namespace == entry.namespace)
            })
            .collect();
        if input
            .items
            .iter()
            .any(|(namespace, _)| !chosen.iter().any(|entry| entry.namespace == *namespace))
        {
            return Ok(Answer::Refused(scope_unavailable()));
        }
        let namespaces = chosen.iter().map(|entry| entry.namespace).collect();
        let ceiling = ceiling_u8(access.read_ceiling);
        let mut fetched: Vec<LabeledDetail> = Vec::new();
        let mut exposed = AccessLevel::PUBLIC;
        for entry in &chosen {
            let ids: Vec<Uuid> = input
                .items
                .iter()
                .filter(|(namespace, _)| *namespace == entry.namespace)
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
                fetched.push(LabeledDetail {
                    namespace: entry.namespace,
                    label: entry.scope.label.clone(),
                    detail,
                });
            }
        }
        let (result, refs) = bounded_details(&mut fetched, &input.items)?;
        self.persistence
            .observe_memory_read(
                fence,
                call,
                MemoryReadObservation {
                    result,
                    namespaces,
                    exposed,
                    read_ceiling: access.read_ceiling,
                    items: refs,
                },
            )
            .await
            .map(Answer::Observed)
    }

    async fn save(&self, fence: WorkflowFence, call: &MemoryToolCall) -> AppResult<Answer> {
        let Some(input) = SaveInput::parse(&call.arguments) else {
            return Ok(Answer::Refused(invalid_input(call.tool)));
        };
        match self.persistence.save_memory(fence, call, input).await? {
            MemorySaveOutcome::Accepted(receipt) => Ok(Answer::Observed(receipt)),
            MemorySaveOutcome::NotEnabled => Ok(Answer::Refused(not_enabled())),
            MemorySaveOutcome::ScopeNotAllowed => Ok(Answer::Refused(tool_error(
                "memory.scope_unavailable",
                "That memory scope is not available in this conversation",
            ))),
            MemorySaveOutcome::SubjectNotAllowed => Ok(Answer::Refused(tool_error(
                "memory.subject_unavailable",
                "That subject is not visible in this conversation",
            ))),
            MemorySaveOutcome::NotReady(scope) => {
                if let Err(error) = self
                    .persistence
                    .provision_memory_scope(fence, scope.owner)
                    .await
                {
                    tracing::warn!(error = %error, "Memory namespace provisioning request failed");
                }
                Ok(Answer::Refused(tool_error(
                    "memory.not_ready",
                    "Memory is being prepared for this scope; try again shortly",
                )))
            }
        }
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
        let scopes = parse_scopes(object.get("scopes"))?;
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
    scopes: Option<Vec<MemoryToolScope>>,
    id: Uuid,
    before: u32,
    after: u32,
}

impl TimelineInput {
    fn parse(arguments: &Value) -> Option<Self> {
        let object = arguments.as_object()?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "scopes" | "id" | "before" | "after"))
        {
            return None;
        }
        let side = |key: &str| match object.get(key) {
            None => Some(MAX_TIMELINE_SIDE),
            Some(value) => value.as_u64().filter(|side| *side <= MAX_TIMELINE_SIDE),
        };
        Some(Self {
            scopes: parse_scopes(object.get("scopes"))?,
            id: Uuid::parse_str(object.get("id")?.as_str()?).ok()?,
            before: side("before")? as u32,
            after: side("after")? as u32,
        })
    }
}

struct DetailsInput {
    items: Vec<(NamespaceExternalId, Uuid)>,
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
                .any(|key| !matches!(key.as_str(), "namespace" | "id"))
            {
                return None;
            }
            let entry = (
                NamespaceExternalId(Uuid::parse_str(item.get("namespace")?.as_str()?).ok()?),
                Uuid::parse_str(item.get("id")?.as_str()?).ok()?,
            );
            if !items.contains(&entry) {
                items.push(entry);
            }
        }
        Some(Self { items })
    }
}

type SaveInput = MemoryToolSave;

impl SaveInput {
    fn parse(arguments: &Value) -> Option<Self> {
        let object = arguments.as_object()?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "facts" | "scope" | "subject"))
        {
            return None;
        }
        let raw = object.get("facts")?.as_array()?;
        if raw.is_empty() || raw.len() as u64 > MAX_FACTS {
            return None;
        }
        let facts = raw
            .iter()
            .map(|fact| {
                let fact = fact.as_str()?;
                (!fact.trim().is_empty() && fact.chars().count() as u64 <= MAX_FACT_CHARS)
                    .then(|| fact.to_owned())
            })
            .collect::<Option<Vec<_>>>()?;
        let target = match object.get("scope") {
            None => MemorySaveTarget::Default,
            Some(value) => match value.as_str()? {
                "unit" => MemorySaveTarget::Unit,
                "personal" => MemorySaveTarget::Personal,
                _ => return None,
            },
        };
        let subject = match object.get("subject") {
            None => None,
            Some(value) => Some(crate::entities::transport::PrincipalId::new(
                Uuid::parse_str(value.as_str()?).ok()?,
            )),
        };
        Some(Self {
            target,
            facts,
            subject,
        })
    }
}

fn parse_scopes(value: Option<&Value>) -> Option<Option<Vec<MemoryToolScope>>> {
    let Some(value) = value else {
        return Some(None);
    };
    let values = value.as_array()?;
    if values.is_empty() || values.len() > MemoryToolScope::ALL.len() {
        return None;
    }
    let mut scopes = Vec::new();
    for value in values {
        let scope = MemoryToolScope::parse(value.as_str()?)?;
        if scopes.contains(&scope) {
            return None;
        }
        scopes.push(scope);
    }
    Some(Some(scopes))
}

struct LabeledHit {
    namespace: NamespaceExternalId,
    label: String,
    hit: MemoryHit,
}

struct LabeledDetail {
    namespace: NamespaceExternalId,
    label: String,
    detail: MemoryDetail,
}

fn hit_json(item: &LabeledHit) -> Value {
    let hit = &item.hit;
    json!({"scope": item.label,"namespace":item.namespace, "id": hit.observation_id, "revision": hit.revision,
        "kind": hit.kind, "title": hit.title, "snippet": hit.snippet,
        "occurred_at": hit.occurred_at.to_rfc3339(), "access_level": hit.access_level})
}

fn detail_json(item: &LabeledDetail) -> Value {
    let detail = &item.detail;
    json!({"scope": item.label,"namespace":item.namespace, "id": detail.observation_id, "revision": detail.revision,
        "kind": detail.kind, "title": detail.title, "text": detail.body,
        "occurred_at": detail.occurred_at.to_rfc3339(), "access_level": detail.access_level})
}

/// Interleave namespaces before applying the shared limit, preserving every
/// response's exposed level.
fn merge_searches(
    answers: Vec<(&ScopeAccess, MemorySearchResult)>,
    limit: u32,
) -> AppResult<(Vec<LabeledHit>, AccessLevel)> {
    let exposed = answers
        .iter()
        .try_fold(AccessLevel::PUBLIC, |level, (_, result)| {
            service_level(result.exposed_level).map(|value| level.max(value))
        })?;
    let mut queues: Vec<_> = answers
        .into_iter()
        .map(|(scope, result)| (scope, result.items.into_iter()))
        .collect();
    let mut merged = Vec::new();
    while merged.len() < limit as usize {
        let mut progressed = false;
        for (scope, items) in &mut queues {
            if merged.len() == limit as usize {
                break;
            }
            if let Some(hit) = items.next() {
                merged.push(LabeledHit {
                    namespace: scope.namespace,
                    label: scope.scope.label.clone(),
                    hit,
                });
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    let exposed = merged.iter().try_fold(exposed, |level, item| {
        service_level(item.hit.access_level).map(|value| level.max(value))
    })?;
    Ok((merged, exposed))
}

fn bounded_details(
    fetched: &mut [LabeledDetail],
    requested: &[(NamespaceExternalId, Uuid)],
) -> AppResult<(Value, Vec<MemoryRecallRef>)> {
    // Unreadable IDs are absent; preserve the caller's order without an existence oracle.
    fetched.sort_by_key(|item| {
        requested
            .iter()
            .position(|wanted| *wanted == (item.namespace, item.detail.observation_id))
    });
    let max = result_bound();
    let mut kept: Vec<Value> = Vec::new();
    let mut refs = Vec::new();
    for item in fetched.iter() {
        kept.push(detail_json(item));
        let candidate = json!({"items": kept, "omitted": fetched.len() - kept.len()});
        if candidate.to_string().len() > max {
            kept.pop();
            break;
        }
        refs.push(MemoryRecallRef::detail(item.namespace, &item.detail)?);
    }
    let omitted = fetched.len() - kept.len();
    let result = json!({"items": kept, "omitted": omitted});
    Ok((result, refs))
}

/// Keep whole hits within the result bound, with a reference for each retained hit.
fn bounded_hits(hits: &[LabeledHit]) -> AppResult<(Value, Vec<MemoryRecallRef>)> {
    let max = result_bound();
    let mut items: Vec<Value> = hits.iter().map(hit_json).collect();
    let mut result = json!({"items": items});
    while result.to_string().len() > max && !items.is_empty() {
        items.pop();
        result = json!({"items": items});
    }
    let refs = hits[..items.len()]
        .iter()
        .map(|item| MemoryRecallRef::hit(item.namespace, &item.hit))
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
                "Give a query of 1–1024 characters, optional scopes from personal, unit, ancestors and reports, and an optional limit of 1–10"
            }
            MemoryTool::Timeline => {
                "Give a memory id from search_memory, optional scopes, and optional before/after counts of 0–5"
            }
            MemoryTool::Details => "Give 1–4 items, each with a namespace and a memory id",
            MemoryTool::Save => {
                "Give 1–8 facts of 1–4096 characters each, and optionally scope unit or personal and a visible principal subject"
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

fn scope_unavailable() -> Value {
    tool_error(
        "memory.scope_unavailable",
        "That memory scope is not available in this conversation",
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
