//! `memory.load` and `memory.save` (phase 05 decision E4). Authority, ceilings and labels are
//! derived by the host from the claimed run: a step names only its scope, query or facts.
//! `memory.load` authorizes under the fence, reads outside any transaction, then commits the
//! classification rise before its output can reach anything; `memory.save` freezes one atomic
//! request with its delivery intent and parks on it (marker-owned: the intent is its only
//! effect path, E8).
use super::lease::{ClaimedWorkflow, SuspendedWorkflow, WorkflowFailure, WorkflowHandlerOutcome};
use super::polling::WorkflowStepKind;
use super::worker::WorkflowHandler;
use crate::app_error::{AppError, AppResult};
use crate::application::memory::{
    ClientCredential, MemoryClient, MemoryDetail, MemorySearch, MemoryServiceError,
    NamespaceExternalId, NamespaceOwner,
};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure, WaitId};
use crate::entities::access_level::AccessLevel;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

pub const LOAD: &str = "memory.load";
pub const SAVE: &str = "memory.save";

/// Which namespace a step names; an agent scope is resolved to the bundle's agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryStepScope {
    Company,
    Agent(Uuid),
}
impl MemoryStepScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Company => "company",
            Self::Agent(_) => "agent",
        }
    }
    pub fn owner(self) -> NamespaceOwner {
        match self {
            Self::Company => NamespaceOwner::Company,
            Self::Agent(agent) => NamespaceOwner::Agent(agent),
        }
    }
}

/// What the host authorized for one `memory.load` attempt (rolled back; nothing committed).
pub struct LoadAuthority {
    pub scope: MemoryStepScope,
    pub namespace: NamespaceExternalId,
    pub credential: ClientCredential,
    /// `min(run ceiling, destination audience, grant)`, never a model-supplied value (E3).
    pub read_ceiling: AccessLevel,
    pub query: String,
    pub limit: u32,
}

pub enum LoadAuthorization {
    Ready(Box<LoadAuthority>),
    /// Provisioning outside the run transaction is the caller's next step (E2).
    NotReady(NamespaceOwner),
    NotEnabled,
}

pub enum SavePark {
    Parked(WaitId),
    NotReady(NamespaceOwner),
    NotEnabled,
    TooLarge,
}

/// The persistence the memory steps need. Every method is a fenced, live-checked transition.
#[async_trait]
pub trait MemoryStepStore: Send + Sync {
    async fn authorize_memory_load(&self, claim: &ClaimedWorkflow) -> AppResult<LoadAuthorization>;
    /// One transaction: the exposed level checked against the ceiling used, the run's level
    /// raised and the consumption recorded, before the output is returned to the engine.
    async fn observe_memory_load(
        &self,
        claim: &ClaimedWorkflow,
        read_ceiling: AccessLevel,
        exposed: AccessLevel,
        items: &[MemoryDetail],
    ) -> AppResult<()>;
    /// One transaction: the frozen request, its intent and the effect wait.
    async fn park_memory_save(&self, claim: &ClaimedWorkflow) -> AppResult<SavePark>;
    /// Lazy provisioning in its own transaction, never inside the run's (P7b obligation).
    async fn provision_memory(
        &self,
        claim: &ClaimedWorkflow,
        owner: NamespaceOwner,
    ) -> AppResult<()>;
}

pub fn failure(class: FailureClass, code: &'static str) -> WorkflowFailure {
    WorkflowFailure {
        failure: StepFailure::new(
            class,
            FailureCode::parse(code).expect("static memory failure code"),
            None,
        )
        .expect("bounded memory failure"),
        safety: RetrySafety::SafeToRetry,
    }
}

/// A terminal intent's outcome as the step's failure (E2, E8).
pub fn save_failed(outcome: &str) -> WorkflowFailure {
    match outcome {
        "disabled" => failure(FailureClass::Terminal, "memory.not_enabled"),
        "expired" => failure(FailureClass::Terminal, "memory.save_expired"),
        "cancelled" | "owner_removed" | "run_cancelled" => {
            failure(FailureClass::Terminal, "memory.save_cancelled")
        }
        _ => failure(FailureClass::Terminal, "memory.save_failed"),
    }
}

fn service_failure(error: &MemoryServiceError) -> WorkflowFailure {
    match error {
        MemoryServiceError::Unavailable
        | MemoryServiceError::Capacity { .. }
        | MemoryServiceError::Ambiguous => failure(FailureClass::Retryable, "memory.unavailable"),
        MemoryServiceError::Protocol => failure(FailureClass::Terminal, "memory.invalid_query"),
        MemoryServiceError::Unauthorized | MemoryServiceError::Forbidden { .. } => {
            failure(FailureClass::Terminal, "memory.forbidden")
        }
        _ => failure(FailureClass::Terminal, "memory.refused"),
    }
}

fn host_failure(error: &AppError) -> WorkflowFailure {
    match error {
        AppError::KnowledgeRelease { .. } => failure(
            FailureClass::Terminal,
            crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT,
        ),
        AppError::BadRequest(_) | AppError::NotFound(_) => {
            failure(FailureClass::Terminal, "memory.rejected")
        }
        _ => failure(FailureClass::Retryable, "memory.unavailable"),
    }
}

/// The level a load's answer carries (review F11): the service's `exposed_level` or any returned
/// item's level, whichever is higher. An under-reported `exposed_level` therefore cannot
/// under-account the output. `None` when an item carries no valid level.
pub fn carried_level(exposed: AccessLevel, items: &[MemoryDetail]) -> Option<AccessLevel> {
    items.iter().try_fold(exposed, |level, item| {
        AccessLevel::try_from(i16::from(item.access_level))
            .ok()
            .map(|item| level.max(item))
    })
}

/// The `memory.load` output (E4): whole items in hit order until the step-output budget, the
/// rest counted in `omitted`. `exposed_level` is the level the run was raised to
/// (`carried_level`), which covers omitted items too.
pub fn load_output(
    scope: MemoryStepScope,
    items: &[MemoryDetail],
    exposed: AccessLevel,
    max_result_bytes: usize,
) -> Value {
    let item = |detail: &MemoryDetail| {
        json!({"id": detail.observation_id, "scope": scope.label(), "kind": detail.kind,
            "title": detail.title, "content": detail.body,
            "occurred_at": detail.occurred_at.to_rfc3339(),
            "access_level": detail.access_level, "revision": detail.revision})
    };
    let output = |kept: &[Value]| {
        json!({"items": kept, "exposed_level": exposed.as_i16(),
            "omitted": items.len() - kept.len()})
    };
    let mut kept = Vec::new();
    for detail in items {
        kept.push(item(detail));
        if output(&kept).to_string().len() > max_result_bytes {
            kept.pop();
            break;
        }
    }
    output(&kept)
}

/// The `memory.save` output from its delivered receipt (E4).
pub fn save_output(receipt: &crate::application::memory::MemorySaveReceipt) -> Value {
    let level = receipt
        .observations
        .iter()
        .map(|saved| saved.access_level)
        .max()
        .unwrap_or(0);
    json!({
        "saved": receipt.observations.iter()
            .map(|saved| json!({"id": saved.observation_id, "revision": saved.revision}))
            .collect::<Vec<_>>(),
        "access_level": level,
        "replayed": receipt.replayed,
    })
}

/// Both memory step kinds, composed only while the memory service is configured.
pub struct MemoryStepRuntime<P> {
    store: P,
    client: Arc<dyn MemoryClient>,
}

impl<P: MemoryStepStore> MemoryStepRuntime<P> {
    pub fn new(store: P, client: Arc<dyn MemoryClient>) -> Self {
        Self { store, client }
    }

    async fn load(&self, claim: &ClaimedWorkflow) -> Result<Value, WorkflowFailure> {
        let authority = match self.store.authorize_memory_load(claim).await {
            Ok(LoadAuthorization::Ready(authority)) => authority,
            Ok(LoadAuthorization::NotReady(owner)) => {
                return Err(self.not_ready(claim, owner).await);
            }
            Ok(LoadAuthorization::NotEnabled) => {
                return Err(failure(FailureClass::Terminal, "memory.not_enabled"));
            }
            Err(error) => return Err(host_failure(&error)),
        };
        let ceiling = authority.read_ceiling.as_i16() as u8;
        let hits = self
            .client
            .search(
                &authority.credential,
                &MemorySearch {
                    namespace: authority.namespace,
                    read_ceiling: ceiling,
                    query: authority.query.clone(),
                    limit: authority.limit,
                },
            )
            .await
            .map_err(|error| service_failure(&error))?;
        let ids: Vec<Uuid> = hits.items.iter().map(|hit| hit.observation_id).collect();
        let (items, exposed) = if ids.is_empty() {
            (Vec::new(), AccessLevel::PUBLIC)
        } else {
            let fetched = self
                .client
                .fetch(&authority.credential, authority.namespace, ceiling, &ids)
                .await
                .map_err(|error| service_failure(&error))?;
            let exposed = AccessLevel::try_from(i16::from(fetched.exposed_level))
                .map_err(|_| failure(FailureClass::Terminal, "memory.refused"))?;
            (fetched.items, exposed)
        };
        let carried = carried_level(exposed, &items)
            .ok_or_else(|| failure(FailureClass::Terminal, "memory.refused"))?;
        self.store
            .observe_memory_load(claim, authority.read_ceiling, exposed, &items)
            .await
            .map_err(|error| host_failure(&error))?;
        Ok(load_output(
            authority.scope,
            &items,
            carried,
            claim.max_result_bytes,
        ))
    }

    async fn not_ready(&self, claim: &ClaimedWorkflow, owner: NamespaceOwner) -> WorkflowFailure {
        if let Err(error) = self.store.provision_memory(claim, owner).await {
            tracing::warn!(error = %error, "Memory namespace provisioning request failed");
        }
        failure(FailureClass::Retryable, "memory.not_ready")
    }
}

#[async_trait]
impl<P: MemoryStepStore> WorkflowHandler for MemoryStepRuntime<P> {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        kind.0 == LOAD || kind.0 == SAVE
    }

    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        if kind.0 == LOAD {
            return WorkflowHandlerOutcome::Completed(self.load(claim).await);
        }
        match self.store.park_memory_save(claim).await {
            Ok(SavePark::Parked(wait)) => {
                WorkflowHandlerOutcome::Suspended(SuspendedWorkflow { wait })
            }
            Ok(SavePark::NotReady(owner)) => {
                WorkflowHandlerOutcome::Completed(Err(self.not_ready(claim, owner).await))
            }
            Ok(SavePark::NotEnabled) => WorkflowHandlerOutcome::Completed(Err(failure(
                FailureClass::Terminal,
                "memory.not_enabled",
            ))),
            Ok(SavePark::TooLarge) => WorkflowHandlerOutcome::Completed(Err(failure(
                FailureClass::Terminal,
                "memory.save_too_large",
            ))),
            Err(error) => WorkflowHandlerOutcome::Completed(Err(host_failure(&error))),
        }
    }
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
