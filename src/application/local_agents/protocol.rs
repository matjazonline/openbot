//! Logical protocol contracts; transport framing and SQL stay in adapters.
use super::{ConnectionFence, LocalCatalog, LocalMachineStore};
use crate::application::transport::bounded::BoundedVec;
use crate::{
    app_error::{AppError, AppResult},
    domain::local_agents::*,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const MAX_LOGICAL_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CONNECTIONS: usize = 32;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const POLL_MILLIS: u64 = 500;
pub const IO_SECONDS: u64 = 5;
pub const MAX_MAPPING_RETIREMENTS: usize = 100;
pub type MappingRetirementSessions = BoundedVec<SessionId, MAX_MAPPING_RETIREMENTS>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingRetirementRequest {
    pub nonce: Uuid,
    pub sessions: MappingRetirementSessions,
}

impl MappingRetirementRequest {
    pub fn validate(&self) -> AppResult<()> {
        let unique: std::collections::HashSet<_> = self.sessions.iter().collect();
        if self.nonce.is_nil() || self.sessions.is_empty() || unique.len() != self.sessions.len() {
            return Err(AppError::BadRequest(
                "Invalid mapping retirement page".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionIdentity {
    pub installation_id: InstallationId,
    pub credential_generation: CredentialGeneration,
    pub connection_generation: ConnectionGeneration,
}

impl ConnectionIdentity {
    pub fn from_fence(fence: ConnectionFence) -> AppResult<Self> {
        if fence.machine.credential_generation.get() > MAX_SAFE_INTEGER
            || fence.generation.get() > MAX_SAFE_INTEGER
        {
            return Err(AppError::Conflict("connection generation exhausted".into()));
        }
        Ok(Self {
            installation_id: fence.machine.installation_id,
            credential_generation: fence.machine.credential_generation,
            connection_generation: fence.generation,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalHello {
    pub version: u16,
    pub installation_id: InstallationId,
    pub journal_id: Uuid,
    pub journal_sequence: u64,
}

impl JournalHello {
    pub fn validate(&self, fence: ConnectionFence) -> AppResult<()> {
        if self.version != super::bounds::PROTOCOL_VERSION
            || self.installation_id != fence.machine.installation_id
            || self.journal_id.is_nil()
            || self.journal_sequence > MAX_SAFE_INTEGER
        {
            return Err(AppError::BadRequest(
                "unsupported local-machine hello".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Welcome {
    pub identity: ConnectionIdentity,
    pub acknowledged_sequence: u64,
    pub recovery_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    Reconcile,
    Dispatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDelivery {
    pub operation: LocalAgentOperation,
    pub mode: DeliveryMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlEvidence {
    Snapshot { state: TurnState },
    StopAccepted {},
    ArchiveAccepted {},
}

/// The canonical typed journal payload, excluding every connection/transfer field.
/// Catalogs remain application-owned; the core agent evidence remains domain-owned.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JournalObservation {
    Agent {
        observation: LocalAgentObservation,
    },
    Catalog {
        sequence: EventSequence,
        operation_id: OperationId,
        catalog: LocalCatalog,
    },
    Control {
        sequence: EventSequence,
        operation_id: OperationId,
        thread_id: CloudroomThreadId,
        evidence: ControlEvidence,
    },
}

impl JournalObservation {
    pub fn sequence(&self) -> EventSequence {
        match self {
            Self::Agent { observation } => observation.sequence,
            Self::Catalog { sequence, .. } | Self::Control { sequence, .. } => *sequence,
        }
    }
    pub fn operation_id(&self) -> OperationId {
        match self {
            Self::Agent { observation } => observation.operation_id,
            Self::Catalog { operation_id, .. } | Self::Control { operation_id, .. } => {
                *operation_id
            }
        }
    }
    pub fn canonical_digest(&self) -> AppResult<PayloadDigest> {
        let data = serde_json::to_vec(self)
            .map_err(|_| AppError::BadRequest("invalid observation".into()))?;
        if data.len() > MAX_LOGICAL_BYTES || self.sequence().get() > MAX_SAFE_INTEGER {
            return Err(AppError::BadRequest(
                "observation exceeds protocol bounds".into(),
            ));
        }
        PayloadDigest::try_from(format!("{:x}", Sha256::digest(data)))
            .map_err(|e| AppError::Internal(e.into()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Hello {
        hello: JournalHello,
    },
    Ready {
        identity: ConnectionIdentity,
    },
    Observation {
        identity: ConnectionIdentity,
        observation: JournalObservation,
    },
    Heartbeat {
        identity: ConnectionIdentity,
    },
    RetireMappings {
        identity: ConnectionIdentity,
        request: MappingRetirementRequest,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerMessage {
    Welcome {
        welcome: Welcome,
    },
    Command {
        identity: ConnectionIdentity,
        // Bound the state held across asynchronous transport waits.
        delivery: Box<CommandDelivery>,
    },
    Acknowledged {
        identity: ConnectionIdentity,
        through: u64,
    },
    Ready {
        identity: ConnectionIdentity,
        ready: bool,
    },
    MappingsRetired {
        identity: ConnectionIdentity,
        nonce: Uuid,
        sessions: MappingRetirementSessions,
    },
}

#[derive(Debug, Clone)]
pub struct OperationAdmission {
    pub company_id: Uuid,
    pub actor_id: Uuid,
    pub machine_id: MachineId,
    pub logical_key: LogicalInvocationKey,
    pub operation: LocalAgentOperation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalInvocationKey(String);
impl LogicalInvocationKey {
    pub fn parse(value: String) -> AppResult<Self> {
        if value.is_empty() || value.len() > 256 || value.contains('\0') {
            return Err(AppError::BadRequest(
                "invalid logical invocation key".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Operational diagnostics only; these values never grant delivery or continuation authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionIssue {
    IncompatibleProtocol,
    RecoveryRequired,
    ConnectionFailed,
}
impl ConnectionIssue {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IncompatibleProtocol => "incompatible_protocol",
            Self::RecoveryRequired => "recovery_required",
            Self::ConnectionFailed => "connection_failed",
        }
    }
}

#[async_trait]
pub trait LocalProtocolStore: LocalMachineStore {
    async fn retired_mappings(
        &self,
        fence: ConnectionFence,
        request: &MappingRetirementRequest,
    ) -> AppResult<MappingRetirementSessions>;
    async fn record_connection_issue(
        &self,
        fence: ConnectionFence,
        issue: ConnectionIssue,
    ) -> AppResult<bool>;
    async fn connect_journal(
        &self,
        fence: ConnectionFence,
        hello: &JournalHello,
    ) -> AppResult<Welcome>;
    async fn finish_reconciliation(&self, fence: ConnectionFence) -> AppResult<bool>;
    async fn pending_commands(&self, fence: ConnectionFence) -> AppResult<Vec<CommandDelivery>>;
    async fn accept_observation(
        &self,
        fence: ConnectionFence,
        observation: &JournalObservation,
    ) -> AppResult<u64>;
    async fn enqueue_operation(
        &self,
        admission: &OperationAdmission,
    ) -> AppResult<LocalAgentOperation>;
    async fn expire_operations(&self, limit: u16) -> AppResult<u16>;
}
