use super::{
    CloudroomThreadId, CloudroomTurnId, EventSequence, MAX_RESULT_BYTES, OperationId, TurnState,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LocalOutput(String);

impl LocalOutput {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for LocalOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocalOutput([redacted])")
    }
}

impl TryFrom<String> for LocalOutput {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() > MAX_RESULT_BYTES || value.contains('\0') {
            return Err("local output exceeds 256 KiB or contains NUL");
        }
        Ok(Self(value))
    }
}

impl From<LocalOutput> for String {
    fn from(value: LocalOutput) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasuredUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// An observation is evidence to validate, never permission to publish or resume a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAgentObservation {
    pub sequence: EventSequence,
    pub operation_id: OperationId,
    pub evidence: LocalEvidence,
}

/// Positive SDK thread identity before any native turn can be correlated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionState {
    Accepted,
    Queued,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalEvidence {
    Receipt {},
    SubmissionIntent {},
    Submission {
        thread_id: CloudroomThreadId,
        state: SubmissionState,
    },
    Refused {
        reason: LocalRefusal,
    },
    Turn {
        thread_id: CloudroomThreadId,
        turn_id: CloudroomTurnId,
        state: TurnState,
        output: Option<LocalOutput>,
        truncated: bool,
        usage: Option<MeasuredUsage>,
    },
    Uncertain {},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalRefusal {
    ProjectUnavailable,
    ProviderUnavailable,
    ModelUnavailable,
    ReasoningUnavailable,
    PermissionsUnavailable,
    Capacity,
    Expired,
    RecoveryRequired,
}
