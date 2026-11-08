use super::{
    CloudroomThreadId, CloudroomTurnId, DeliveryState, EventSequence, ExecutionSelection,
    OperationId, OperationOutcome, SessionId,
};
use chrono::SubsecRound;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_PROMPT_BYTES: usize = 32 * 1024;
pub const MAX_RESULT_BYTES: usize = 256 * 1024;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LocalPrompt(String);

impl std::fmt::Debug for LocalPrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocalPrompt([redacted])")
    }
}

impl LocalPrompt {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for LocalPrompt {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() || value.len() > MAX_PROMPT_BYTES || value.contains('\0') {
            return Err("prompt must contain 1–32768 UTF-8 bytes without NUL");
        }
        Ok(Self(value))
    }
}

impl From<LocalPrompt> for String {
    fn from(value: LocalPrompt) -> Self {
        value.0
    }
}

/// Thread-wide controls have their own operation identity. They never settle another turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalCommand {
    Create {
        session_id: SessionId,
        selection: ExecutionSelection,
        prompt: LocalPrompt,
    },
    Send {
        session_id: SessionId,
        thread_id: CloudroomThreadId,
        prompt: LocalPrompt,
    },
    Inspect {
        session_id: SessionId,
        thread_id: CloudroomThreadId,
    },
    StopThread {
        session_id: SessionId,
        thread_id: CloudroomThreadId,
    },
    ArchiveThread {
        session_id: SessionId,
        thread_id: CloudroomThreadId,
    },
    RefreshCatalog {},
}

impl LocalCommand {
    pub fn is_effect(&self) -> bool {
        matches!(self, Self::Create { .. } | Self::Send { .. })
    }

    pub fn is_thread_control(&self) -> bool {
        matches!(
            self,
            Self::Inspect { .. } | Self::StopThread { .. } | Self::ArchiveThread { .. }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "OperationRecord")]
pub struct LocalAgentOperation {
    id: OperationId,
    command: LocalCommand,
    deadline: chrono::DateTime<chrono::Utc>,
    payload_digest: PayloadDigest,
    dispatch: DispatchEvidence,
    outcome: Option<OperationOutcome>,
}

/// Hashes the frozen command and deadline. Delivery state is deliberately excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PayloadDigest(String);

impl PayloadDigest {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn calculate(
        command: &LocalCommand,
        deadline: chrono::DateTime<chrono::Utc>,
    ) -> Result<Self, String> {
        let payload =
            serde_json::to_vec(&(command, deadline)).map_err(|error| error.to_string())?;
        Ok(Self(format!("{:x}", Sha256::digest(payload))))
    }
}

impl TryFrom<String> for PayloadDigest {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("payload digest must be lowercase SHA-256 hex");
        }
        Ok(Self(value))
    }
}

impl From<PayloadDigest> for String {
    fn from(value: PayloadDigest) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DispatchEvidence {
    NotDispatched {},
    AcceptedLocally {
        sequence: EventSequence,
    },
    SubmissionIntent {
        sequence: EventSequence,
    },
    NativeIdentity {
        sequence: EventSequence,
        thread_id: CloudroomThreadId,
        turn_id: Option<CloudroomTurnId>,
    },
    /// The accepted catalog observation; refresh has no native thread or turn.
    CatalogSnapshot {
        sequence: EventSequence,
    },
}

impl DispatchEvidence {
    pub fn delivery_state(&self) -> DeliveryState {
        match self {
            Self::NotDispatched {} => DeliveryState::Queued,
            Self::AcceptedLocally { .. } => DeliveryState::AcceptedLocally,
            Self::SubmissionIntent { .. } => DeliveryState::Dispatching,
            Self::NativeIdentity { .. } | Self::CatalogSnapshot { .. } => DeliveryState::Observed,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationRecord {
    id: OperationId,
    command: LocalCommand,
    deadline: chrono::DateTime<chrono::Utc>,
    payload_digest: PayloadDigest,
    dispatch: DispatchEvidence,
    outcome: Option<OperationOutcome>,
}

impl TryFrom<OperationRecord> for LocalAgentOperation {
    type Error = String;
    fn try_from(record: OperationRecord) -> Result<Self, Self::Error> {
        if canonical_deadline(record.deadline)? != record.deadline {
            return Err("operation deadline requires microsecond precision".into());
        }
        if record.payload_digest != PayloadDigest::calculate(&record.command, record.deadline)? {
            return Err("operation payload digest mismatch".into());
        }
        validate_dispatch(&record)?;
        Ok(Self {
            id: record.id,
            command: record.command,
            deadline: record.deadline,
            payload_digest: record.payload_digest,
            dispatch: record.dispatch,
            outcome: record.outcome,
        })
    }
}

pub(crate) fn canonical_deadline(
    deadline: chrono::DateTime<chrono::Utc>,
) -> Result<chrono::DateTime<chrono::Utc>, String> {
    if deadline.timestamp_subsec_nanos() >= 1_000_000_000 {
        return Err("leap-second operation deadlines are unsupported".into());
    }
    Ok(deadline.trunc_subsecs(6))
}

fn validate_dispatch(record: &OperationRecord) -> Result<(), String> {
    let catalog = matches!(record.command, LocalCommand::RefreshCatalog {});
    match (&record.dispatch, catalog) {
        (DispatchEvidence::NativeIdentity { .. }, true) => {
            return Err("catalog refresh cannot carry native thread evidence".into());
        }
        (DispatchEvidence::CatalogSnapshot { .. }, false) => {
            return Err("catalog evidence cannot settle a thread command".into());
        }
        _ => {}
    }
    if let DispatchEvidence::NativeIdentity {
        thread_id, turn_id, ..
    } = &record.dispatch
    {
        let expected = match &record.command {
            LocalCommand::Send { thread_id, .. }
            | LocalCommand::Inspect { thread_id, .. }
            | LocalCommand::StopThread { thread_id, .. }
            | LocalCommand::ArchiveThread { thread_id, .. } => Some(thread_id),
            _ => None,
        };
        if expected.is_some_and(|expected| expected != thread_id) {
            return Err("dispatch evidence names another thread".into());
        }
        if record.outcome == Some(OperationOutcome::Succeeded)
            && record.command.is_effect()
            && turn_id.is_none()
        {
            return Err("successful prompt requires a native turn identity".into());
        }
    } else if record.outcome == Some(OperationOutcome::Succeeded)
        && !matches!(record.dispatch, DispatchEvidence::CatalogSnapshot { .. })
    {
        return Err("success requires command-specific observation evidence".into());
    }
    Ok(())
}

impl LocalAgentOperation {
    pub fn queued(
        id: OperationId,
        command: LocalCommand,
        deadline: chrono::DateTime<chrono::Utc>,
    ) -> Result<Self, String> {
        let deadline = canonical_deadline(deadline)?;
        let payload_digest = PayloadDigest::calculate(&command, deadline)?;
        Ok(Self {
            id,
            command,
            deadline,
            payload_digest,
            dispatch: DispatchEvidence::NotDispatched {},
            outcome: None,
        })
    }

    pub fn id(&self) -> OperationId {
        self.id
    }
    pub fn command(&self) -> &LocalCommand {
        &self.command
    }
    pub fn deadline(&self) -> chrono::DateTime<chrono::Utc> {
        self.deadline
    }
    pub fn payload_digest(&self) -> &PayloadDigest {
        &self.payload_digest
    }
    pub fn dispatch(&self) -> &DispatchEvidence {
        &self.dispatch
    }
    pub fn outcome(&self) -> Option<OperationOutcome> {
        self.outcome
    }
}
