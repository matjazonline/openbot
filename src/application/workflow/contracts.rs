use super::{RelatedAssociation, WorkflowActor, binding::ConfiguredBinding};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::entities::correlation::CorrelationId;
use crate::domain::workflow::{
    ExecutionId, RunCausality, RunId, RunState, StepCausality, StepId, TriggerRef, VersionId,
    WorkflowBindingId, WorkflowId,
};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new(value: Uuid) -> Self {
                Self(value)
            }
            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }
    };
}

uuid_id!(WorkerId);
uuid_id!(FenceToken);
uuid_id!(OwnershipReceipt);
pub use crate::domain::workflow::CompanyId;

pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdempotencyKey(String);
impl IdempotencyKey {
    pub fn parse(value: impl AsRef<str>) -> AppResult<Self> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > MAX_IDEMPOTENCY_KEY_BYTES
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b':'))
        {
            return Err(AppError::BadRequest(
                "invalid workflow idempotency key".into(),
            ));
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The idempotency key identifies a logical admission within one company;
/// it is independent of correlation IDs and proposed random run IDs.
pub struct AdmitWorkflowRequest {
    pub company_id: CompanyId,
    pub actor: WorkflowActor,
    pub association: RelatedAssociation,
    pub trigger: TriggerRef,
    /// Observability only. It is neither authority nor an idempotency or fence key.
    pub correlation_id: CorrelationId,
    pub binding_id: WorkflowBindingId,
    pub idempotency_key: IdempotencyKey,
    pub input: Value,
}

/// Input validated against the pinned bundle and aggregate input/parameter bound.
pub(super) struct AdmissionSnapshots {
    pub(super) input: Value,
}

/// Constructed only after authorization, checked binding selection and bounded input.
/// Retains the exact immutable configuration; adapters persist it with the run and
/// first job. Replay equivalence uses logical binding, input, association and stable
/// trigger under company/key, excluding revision, actor, correlation and random IDs.
#[derive(Clone)]
pub struct PreparedAdmission {
    company_id: CompanyId,
    actor: WorkflowActor,
    association: RelatedAssociation,
    causality: RunCausality,
    idempotency_key: IdempotencyKey,
    binding: Arc<ConfiguredBinding>,
    first_step: StepCausality,
    input: Value,
}

impl PreparedAdmission {
    pub(super) fn new(
        request: AdmitWorkflowRequest,
        binding: Arc<ConfiguredBinding>,
        run_id: RunId,
        execution_id: ExecutionId,
        snapshots: AdmissionSnapshots,
    ) -> AppResult<Self> {
        let causality = RunCausality::new(run_id, request.trigger, request.correlation_id)
            .map_err(|error| AppError::BadRequest(error.to_string()))?;
        let first_step = StepCausality::new(
            &causality,
            crate::domain::workflow::ExecutionRef::new(
                request.company_id,
                run_id,
                execution_id,
                binding
                    .bundle()
                    .compiled()
                    .graph()
                    .definition()
                    .entry
                    .clone(),
            ),
        )
        .map_err(|error| AppError::Internal(error.to_string()))?;
        Ok(Self {
            company_id: request.company_id,
            actor: request.actor,
            association: request.association,
            causality,
            idempotency_key: request.idempotency_key,
            binding,
            first_step,
            input: snapshots.input,
        })
    }
    pub fn company_id(&self) -> CompanyId {
        self.company_id
    }
    /// Retained so the atomic persistence owner can recheck current authority.
    pub fn actor(&self) -> WorkflowActor {
        self.actor
    }
    pub fn association(&self) -> RelatedAssociation {
        self.association
    }
    pub fn idempotency_key(&self) -> &IdempotencyKey {
        &self.idempotency_key
    }
    pub fn proposed_run_id(&self) -> RunId {
        self.causality.run_id()
    }
    pub fn causality(&self) -> &RunCausality {
        &self.causality
    }
    pub fn trigger(&self) -> &TriggerRef {
        self.causality.trigger()
    }
    pub fn first_step(&self) -> &StepCausality {
        &self.first_step
    }
    pub fn workflow_id(&self) -> WorkflowId {
        self.binding
            .bundle()
            .compiled()
            .graph()
            .definition()
            .workflow_id
    }
    pub fn version_id(&self) -> VersionId {
        self.binding
            .bundle()
            .compiled()
            .graph()
            .definition()
            .version_id
    }
    /// The run owns this exact binding revision and complete frozen bundle.
    pub fn binding(&self) -> &Arc<ConfiguredBinding> {
        &self.binding
    }
    pub fn entry(&self) -> &StepId {
        self.first_step.execution().step_id()
    }
    pub fn first_execution_id(&self) -> ExecutionId {
        self.first_step.execution().execution_id()
    }
    pub fn input(&self) -> &Value {
        &self.input
    }
    pub fn params(&self) -> &Value {
        self.binding.params()
    }
}

/// `Replayed` returns the originally committed run; `Conflict` means the same
/// company/key was used with a different logical binding, related association,
/// stable trigger identity/source, or input. Configuration changes alone are not conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionResult {
    Created(RunId),
    Replayed(RunId),
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RunRevision(pub u64);

/// Company-scoped authoritative run identity, logical state, and revision.
/// The scope is a lookup boundary, not proof of caller authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunHead {
    pub causality: RunCausality,
    pub workflow_id: WorkflowId,
    pub version_id: VersionId,
    pub association: RelatedAssociation,
    pub state: RunState,
    pub revision: RunRevision,
}
impl RunHead {
    pub fn company_id(&self) -> CompanyId {
        self.causality.company_id()
    }
    pub fn run_id(&self) -> RunId {
        self.causality.run_id()
    }
}

/// A stable command key and caller-observed revision identify one authorized control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelCommand {
    pub actor: WorkflowActor,
    pub command_key: IdempotencyKey,
    pub company_id: CompanyId,
    pub run_id: RunId,
    pub expected_revision: RunRevision,
}

/// A revision conflict is distinct from absence and an already terminal run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelResult {
    Applied { revision: RunRevision },
    AlreadyTerminalOrApplied { revision: RunRevision },
    RevisionConflict { current_revision: RunRevision },
    NotFound,
}

pub struct CancelWorkflowRequest {
    pub expected_revision: RunRevision,
    pub command_key: IdempotencyKey,
    pub company_id: CompanyId,
    pub actor: WorkflowActor,
    pub run_id: RunId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryCommand {
    pub company_id: CompanyId,
    pub actor: WorkflowActor,
    pub run_id: RunId,
    pub expected_revision: RunRevision,
    pub command_key: IdempotencyKey,
}

pub struct RetryWorkflowRequest {
    pub company_id: CompanyId,
    pub actor: WorkflowActor,
    pub run_id: RunId,
    pub expected_revision: RunRevision,
    pub command_key: IdempotencyKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryResult {
    Applied { revision: RunRevision },
    Unsafe { revision: RunRevision },
    RevisionConflict { current_revision: RunRevision },
    NotFound,
}

pub const MAX_CLAIM_BATCH: u16 = 100;
pub const MAX_LEASE_SECONDS: u64 = 300;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimBatch(u16);
impl ClaimBatch {
    pub fn new(value: u16) -> AppResult<Self> {
        if value == 0 || value > MAX_CLAIM_BATCH {
            return Err(AppError::BadRequest("invalid workflow claim batch".into()));
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseDuration(Duration);
impl LeaseDuration {
    pub fn new(value: Duration) -> AppResult<Self> {
        if value < Duration::from_secs(1) || value > Duration::from_secs(MAX_LEASE_SECONDS) {
            return Err(AppError::BadRequest(
                "invalid workflow lease duration".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn get(self) -> Duration {
        self.0
    }
}

/// Batch and lease constructors enforce positive platform bounds.
#[derive(Debug, Clone, Copy)]
pub struct ClaimRequest {
    pub worker_id: WorkerId,
    pub batch: ClaimBatch,
    pub lease: LeaseDuration,
}

/// Persisted exclusive ownership proof. Later writes must present this fresh
/// fence while the lease is live; expiry/recovery accounting belongs to phase 03.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedExecution {
    causality: RunCausality,
    step: StepCausality,
    pub worker_id: WorkerId,
    pub fence: FenceToken,
    pub receipt: OwnershipReceipt,
    pub lease_expires_at: SystemTime,
}
impl ClaimedExecution {
    pub fn new(
        causality: RunCausality,
        step: StepCausality,
        worker_id: WorkerId,
        fence: FenceToken,
        receipt: OwnershipReceipt,
        lease_expires_at: SystemTime,
    ) -> AppResult<Self> {
        if step.execution().company_id() != causality.company_id()
            || step.execution().run_id() != causality.run_id()
        {
            return Err(AppError::Internal(
                "claimed execution causality mismatch".into(),
            ));
        }
        Ok(Self {
            causality,
            step,
            worker_id,
            fence,
            receipt,
            lease_expires_at,
        })
    }
    pub fn causality(&self) -> &RunCausality {
        &self.causality
    }
    pub fn step(&self) -> &StepCausality {
        &self.step
    }
    pub fn company_id(&self) -> CompanyId {
        self.causality.company_id()
    }
    pub fn run_id(&self) -> RunId {
        self.causality.run_id()
    }
    pub fn execution_id(&self) -> ExecutionId {
        self.step.execution().execution_id()
    }
    pub fn step_id(&self) -> &StepId {
        self.step.execution().step_id()
    }
}
