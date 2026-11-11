use super::{CloudroomProjectId, MachineId, ModelId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalProvider {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "acp-opencode")]
    OpenCode,
}

/// These choices are frozen on admission; project defaults cannot change accepted work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSelection {
    pub machine_id: MachineId,
    pub project_id: CloudroomProjectId,
    pub provider: LocalProvider,
    pub model: ModelId,
    pub policy: ExecutionPolicy,
}

/// Only explicit local workspace reuse is enabled until the pinned SDK proves other choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPolicy {
    pub workspace: LocalWorkspace,
    pub permissions: LocalPermissions,
    pub reasoning: LocalReasoningLevel,
}

/// The pinned public SDK's closed reasoning ladder. Admission checks the exact model's catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalReasoningLevel {
    None,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Ultra,
    Ultracode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalWorkspace {
    ProjectDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalPermissions {
    Full,
    Auto,
    AcceptEdits,
}
