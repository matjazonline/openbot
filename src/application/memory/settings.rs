//! Per-company opt-in (U1) and lazy namespace provisioning (E2), as the application sees them.
//! The store implements the owner-row lock protocol; this layer only adds what the process knows:
//! whether the service is configured and under which provisioner credential.
use crate::app_error::{AppError, AppResult};
use crate::application::workflow::{CompanyId, WorkflowActor};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::NamespaceExternalId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTenantState {
    Enabled,
    Disabled,
}

/// The remote lifecycle as settings show it (E1 step 2: refusals are visible here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryLifecycleSummary {
    pub resource: String,
    pub desired: String,
    pub observed: String,
    pub state: String,
    pub attempts: i32,
    pub outcome_code: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryNamespaceCounts {
    pub ready: i64,
    pub provisioning: i64,
    pub removing: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryTenantView {
    pub state: MemoryTenantState,
    pub revision: i64,
    pub provisioner_credential_id: Uuid,
    pub generation_cap: Option<i64>,
    pub changed_by: Uuid,
    pub changed_at: DateTime<Utc>,
    pub tenant: Option<MemoryLifecycleSummary>,
    pub namespaces: MemoryNamespaceCounts,
    /// Unsettled namespace lifecycles with an outcome (failed, refused or retrying), at most 20.
    pub problems: Vec<MemoryLifecycleSummary>,
    pub more_problems: bool,
    /// The company's agents (at most `MAX_CAPTURE_AGENTS`, by name) with both capture policies.
    pub capture: Vec<MemoryAgentCapture>,
    pub more_agents: bool,
}

/// Which conversations a capture policy covers (E8): an agent's private sessions (into the
/// session creator's own memory) or its channel threads (into the agent's memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryCaptureKind {
    PrivateSessions,
    Threads,
}

impl MemoryCaptureKind {
    pub const ALL: [Self; 2] = [Self::PrivateSessions, Self::Threads];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrivateSessions => "private_sessions",
            Self::Threads => "threads",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

pub const MAX_CAPTURE_AGENTS: i64 = 100;
pub const MAX_CAPTURE_ALLOWANCE_TOKENS: i64 = 1_000_000;
/// The service's provider bound on one generation's output (E8).
pub const MAX_CAPTURE_OUTPUT_TOKENS: i32 = 8_192;
/// Inside the service's 7-day idempotency window (D5).
pub const MAX_CAPTURE_EXPIRY_HOURS: i32 = 168;
pub const MAX_CAPTURE_REASON_CHARS: usize = 500;

/// What one capture spends and how long its request stays valid, frozen into each intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureLimits {
    pub allowance_tokens: i64,
    pub max_output_tokens: i32,
    pub expiry_hours: i32,
}

impl CaptureLimits {
    /// What a policy that was never set shows.
    pub const DEFAULT: Self = Self {
        allowance_tokens: 32_768,
        max_output_tokens: 2_048,
        expiry_hours: 24,
    };

    pub fn validate(self) -> AppResult<Self> {
        if !(1..=MAX_CAPTURE_ALLOWANCE_TOKENS).contains(&self.allowance_tokens) {
            return Err(AppError::BadRequest(format!(
                "The allowance must be 1 to {MAX_CAPTURE_ALLOWANCE_TOKENS} tokens"
            )));
        }
        if !(1..=MAX_CAPTURE_OUTPUT_TOKENS).contains(&self.max_output_tokens) {
            return Err(AppError::BadRequest(format!(
                "The output limit must be 1 to {MAX_CAPTURE_OUTPUT_TOKENS} tokens"
            )));
        }
        if !(1..=MAX_CAPTURE_EXPIRY_HOURS).contains(&self.expiry_hours) {
            return Err(AppError::BadRequest(format!(
                "The expiry must be 1 to {MAX_CAPTURE_EXPIRY_HOURS} hours"
            )));
        }
        Ok(self)
    }
}

/// Who last changed a policy, why and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryCaptureChangeRecord {
    pub by: Uuid,
    pub reason: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryCapturePolicy {
    pub kind: MemoryCaptureKind,
    pub enabled: bool,
    pub limits: CaptureLimits,
    /// 0 until the policy is first set.
    pub revision: i64,
    pub changed: Option<MemoryCaptureChangeRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryAgentCapture {
    pub agent: Uuid,
    pub name: String,
    /// Both kinds, in `MemoryCaptureKind::ALL` order.
    pub policies: Vec<MemoryCapturePolicy>,
}

/// An owner's or admin's change of one capture policy. `expected_revision` is the policy
/// revision the form showed (0 before it was first set); a stale one is a `Conflict`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryCaptureChange {
    pub agent: Uuid,
    pub kind: MemoryCaptureKind,
    pub enabled: bool,
    pub limits: CaptureLimits,
    pub reason: String,
    pub expected_revision: i64,
}

/// What an owner or admin sees in company settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySettingsView {
    /// The process has `MEMORY_SERVICE_URL`; its provisioner credential id when it does.
    pub configured: Option<Uuid>,
    pub tenant: Option<MemoryTenantView>,
}

/// The owner/admin commands. Every method authorizes the actor again under the E2 locks and
/// answers a concealed `NotFound` to anyone else. `expected_revision` is the tenant revision the
/// form showed (0 before the first enable); a stale one is a `Conflict`.
#[async_trait]
pub trait MemorySettingsStore: Send + Sync {
    async fn memory_tenant(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
    ) -> AppResult<Option<MemoryTenantView>>;
    async fn enable_memory(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        expected_revision: i64,
        provisioner_credential_id: Uuid,
    ) -> AppResult<()>;
    async fn disable_memory(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        expected_revision: i64,
    ) -> AppResult<()>;
    /// The owner's re-drive (review p7b r2 m3): every failed `present`/`disabled` lifecycle of an
    /// enabled company is due again with a fresh budget. Changes neither `revision` nor
    /// `disable_epoch`, so no pending work is failed. Answers how many rows were re-driven.
    async fn retry_memory_provisioning(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        expected_revision: i64,
    ) -> AppResult<u64>;
    /// Records one capture policy change. Turning a policy on needs memory enabled; turning it
    /// off is always allowed while the tenant exists.
    async fn set_memory_capture(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        change: &MemoryCaptureChange,
    ) -> AppResult<()>;
}

/// The settings use case: the store plus the process's configuration.
pub struct MemorySettings<'a> {
    store: &'a dyn MemorySettingsStore,
    configured: Option<Uuid>,
}

impl<'a> MemorySettings<'a> {
    pub fn new(store: &'a dyn MemorySettingsStore, configured: Option<Uuid>) -> Self {
        Self { store, configured }
    }

    pub async fn view(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
    ) -> AppResult<MemorySettingsView> {
        Ok(MemorySettingsView {
            configured: self.configured,
            tenant: self.store.memory_tenant(company, actor).await?,
        })
    }

    /// Refused while the service is unconfigured: enabling records a tenant that only the
    /// configured provisioner credential can create.
    pub async fn enable(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        expected_revision: i64,
    ) -> AppResult<()> {
        let Some(provisioner) = self.configured else {
            return Err(AppError::BadRequest(
                "The memory service is not configured for this deployment".into(),
            ));
        };
        self.store
            .enable_memory(company, actor, expected_revision, provisioner)
            .await
    }

    pub async fn retry(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        expected_revision: i64,
    ) -> AppResult<u64> {
        if self.configured.is_none() {
            return Err(AppError::BadRequest(
                "The memory service is not configured for this deployment".into(),
            ));
        }
        self.store
            .retry_memory_provisioning(company, actor, expected_revision)
            .await
    }

    /// Turning capture on is refused while the service is unconfigured; turning it off is not.
    pub async fn set_capture(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        mut change: MemoryCaptureChange,
    ) -> AppResult<()> {
        if change.enabled && self.configured.is_none() {
            return Err(AppError::BadRequest(
                "The memory service is not configured for this deployment".into(),
            ));
        }
        change.limits = change.limits.validate()?;
        change.reason = change.reason.trim().to_owned();
        if change.reason.is_empty() || change.reason.chars().count() > MAX_CAPTURE_REASON_CHARS {
            return Err(AppError::BadRequest(format!(
                "Give a reason of 1 to {MAX_CAPTURE_REASON_CHARS} characters"
            )));
        }
        self.store.set_memory_capture(company, actor, &change).await
    }

    /// Allowed while unconfigured: disabling only stops work and records the remote intent.
    pub async fn disable(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        expected_revision: i64,
    ) -> AppResult<()> {
        self.store
            .disable_memory(company, actor, expected_revision)
            .await
    }
}

/// Who a namespace belongs to. Company scope has no owner beyond the company itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceOwner {
    Company,
    Agent(Uuid),
    User(Uuid),
}

/// The answer of lazy provisioning (E2). Only `Ready` carries a namespace; a run seeing anything
/// else continues with `not_ready`, `not_enabled` or a refusal, never a wider scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceReadiness {
    Ready(NamespaceExternalId),
    /// Recorded (now or earlier); the lifecycle worker has not issued its credential yet.
    NotReady,
    NotEnabled,
    /// The owner is missing, deleted, or not in this company.
    OwnerUnavailable,
}

/// Lazy provisioning in its own short transaction, never inside a fenced run transaction.
#[async_trait]
pub trait MemoryNamespaces: Send + Sync {
    async fn ensure_namespace(
        &self,
        company: CompanyId,
        owner: NamespaceOwner,
    ) -> AppResult<NamespaceReadiness>;
}
