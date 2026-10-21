//! Native workflow checkpoint ownership; no legacy task lease crosses this port.
use super::{
    decisions::DecisionNotices,
    lease::{LeasePolicy, WorkflowFence},
};
use crate::domain::workflow::WaitId;
use crate::{
    app_error::AppResult,
    services::harness::runs::{
        CheckpointRevision, InvocationId, ModelReservation, RigExecutionPolicy, RunCheckpoint,
        RunId, RunIdentity, SavedModelTurn, WriteOutcome,
    },
};
use async_trait::async_trait;
use serde_json::Value;

/// Prepared by the workflow runtime from the frozen publication and explicit inputs.
/// The document includes the frozen prompt/tool/target contract, without credentials.
pub struct WorkflowCheckpointSpec {
    pub identity: RunIdentity,
    pub initial_prompt: String,
    pub policy: RigExecutionPolicy,
    pub prepared: Value,
}

#[derive(Clone, Copy)]
pub struct WorkflowCheckpointWrite {
    pub fence: WorkflowFence,
    pub checkpoint: RunId,
    pub expected_revision: CheckpointRevision,
    pub lease_policy: LeasePolicy,
}

/// Native driver updates reuse the shared checkpoint state machine. Model
/// admission and settlement have their own accounting and ownership operations.
#[derive(Clone)]
pub enum WorkflowCheckpointUpdate {
    BindTools(String),
    PrepareInvocation(InvocationId),
    InvocationResult {
        invocation: InvocationId,
        result: Value,
    },
    FinalOutput(Box<crate::services::harness::AgentExecutionOutput>),
    InvalidOutput,
}

#[derive(Debug, Clone)]
pub struct WorkflowModelRequest {
    pub reservation: ModelReservation,
    /// Canonical provider request digest; excludes credentials.
    pub fingerprint: [u8; 32],
}
impl WorkflowModelRequest {
    pub fn saved(&self) -> Value {
        let fingerprint: String = self
            .fingerprint
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        serde_json::json!({"reservation": self.reservation, "fingerprint": fingerprint})
    }
}

#[derive(Debug)]
pub enum WorkflowModelSettlement {
    Refused(Box<RunCheckpoint>),
    ReviewUnavailable(Box<RunCheckpoint>),
    Saved(Box<WriteOutcome<RunCheckpoint>>),
    Parked {
        checkpoint: Box<RunCheckpoint>,
        wait: WaitId,
    },
}

impl WorkflowModelSettlement {
    pub(crate) fn saved(outcome: WriteOutcome<RunCheckpoint>) -> Self {
        Self::Saved(Box::new(outcome))
    }
}

/// A fresh durable admission is the only result that may enter a provider.
/// It is deliberately non-cloneable; replay and uncertainty cannot manufacture it.
#[derive(Debug)]
pub struct NewModelAdmission {
    checkpoint: Box<RunCheckpoint>,
}
impl NewModelAdmission {
    pub(crate) fn committed(checkpoint: RunCheckpoint) -> Self {
        Self {
            checkpoint: Box::new(checkpoint),
        }
    }
    pub fn checkpoint(&self) -> &RunCheckpoint {
        &self.checkpoint
    }
    pub fn consume(self) -> RunCheckpoint {
        *self.checkpoint
    }
}

#[derive(Debug)]
pub enum WorkflowModelAdmission {
    New(NewModelAdmission),
    PreviouslyAdmitted(Box<RunCheckpoint>),
    ActiveRequest,
    RevisionConflict,
    OwnershipLost,
    BudgetExhausted,
    Parked(WaitId),
    TokenRefused,
}

#[async_trait]
pub trait WorkflowCheckpointStore: Send + Sync {
    async fn update_workflow_checkpoint(
        &self,
        write: WorkflowCheckpointWrite,
        update: WorkflowCheckpointUpdate,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;

    async fn open_workflow_checkpoint(
        &self,
        fence: WorkflowFence,
        spec: &WorkflowCheckpointSpec,
        policy: LeasePolicy,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;

    async fn load_workflow_checkpoint(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;

    async fn reserve_workflow_model(
        &self,
        write: WorkflowCheckpointWrite,
        request: WorkflowModelRequest,
        notices: Option<&DecisionNotices>,
    ) -> AppResult<WorkflowModelAdmission>;

    async fn settle_workflow_model(
        &self,
        write: WorkflowCheckpointWrite,
        turn: SavedModelTurn,
        notices: Option<&DecisionNotices>,
    ) -> AppResult<WorkflowModelSettlement>;

    async fn reserve_workflow_execution(
        &self,
        write: WorkflowCheckpointWrite,
        allowance_ms: u64,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;

    /// The saved reservation of a request parked before I/O. A driver must present this
    /// exact identity again; admission verifies the approved proposal and fingerprint.
    async fn blocked_workflow_model(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
    ) -> AppResult<Option<ModelReservation>>;
}
