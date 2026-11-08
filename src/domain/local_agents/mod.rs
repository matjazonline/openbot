//! Company-owned local execution. Cloudroom owns processes and canonical transcripts.
mod identities;
mod observations;
mod operations;
mod selection;

pub use identities::*;
pub use observations::*;
pub(crate) use operations::canonical_deadline;
pub use operations::*;
pub use selection::*;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineState {
    Enabled,
    Disabled,
    Revoked,
}

/// Server-owned admission authority. It is deliberately absent from the native wire payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationOrigin {
    Direct,
    Workflow,
}
impl OperationOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Workflow => "workflow",
        }
    }
}
impl TryFrom<&str> for OperationOrigin {
    type Error = &'static str;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "direct" => Ok(Self::Direct),
            "workflow" => Ok(Self::Workflow),
            _ => Err("invalid operation origin"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalMachine {
    pub id: MachineId,
    pub company_id: Uuid,
    pub installation_id: InstallationId,
    pub name: String,
    pub state: MachineState,
    pub credential_generation: CredentialGeneration,
    pub last_seen_at: Option<DateTime<Utc>>,
    /// Last committed capability observation, scoped to this installation.
    pub capabilities_sequence: Option<EventSequence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAgentProfile {
    pub id: ProfileId,
    pub company_id: Uuid,
    pub name: String,
    pub selection: ExecutionSelection,
}

/// Read/control authority follows this original association, never machine membership alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionAssociation {
    Conversation { conversation_id: Uuid },
    Workflow { run_id: Uuid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAgentSession {
    pub id: SessionId,
    pub profile_id: ProfileId,
    pub company_id: Uuid,
    pub installation_id: InstallationId,
    pub selection: ExecutionSelection,
    pub association: SessionAssociation,
    pub thread_id: Option<CloudroomThreadId>,
}

/// Delivery state is separate from connectivity and from the native turn's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Queued,
    AcceptedLocally,
    Dispatching,
    Observed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationOutcome {
    Succeeded,
    Failed,
    Cancelled,
    Expired,
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Queued,
    Running,
    WaitingForInput,
    Completed,
    Failed,
    Stopped,
    Missing,
}
