use super::*;
use serde::{Deserialize, Serialize};

pub const MAX_SAMPLE_BYTES: usize = 256 * 1024;
pub const MAX_SAMPLE_STEPS: u32 = 128;
pub(super) const MAX_REPORT_BYTES: usize = 1024 * 1024;
pub(super) const MAX_DEPTH: usize = 8;
pub(super) const MAX_TOOL_CALLS: usize = 64;

/// Only inert data crosses the sample boundary. No production binding or continuation is accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRequest {
    pub input: Value,
    pub params: Value,
    #[serde(default)]
    pub scripts: Vec<StepScript>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepScript {
    pub workflow_id: WorkflowId,
    pub step: StepId,
    pub responses: Vec<ScriptedResponse>,
}
/// For model steps, optional calls are one scripted tool turn followed by the final output turn.
/// Other external steps consume one output fixture; human output is {choice,data,feedback}.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptedResponse {
    pub output: Value,
    #[serde(default)]
    pub tools: Vec<ScriptedToolCall>,
    /// Synthetic usage supplied by the author; never billed or sent to a provider.
    #[serde(default)]
    pub tokens: u32,
    #[serde(default)]
    pub approval: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptedToolCall {
    pub name: crate::domain::workflow::TypeName,
    pub input: Value,
    /// Required for intercepted external tools; forbidden for a workflow tool, whose child runs.
    pub output: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct SamplePath(pub(super) String);
impl SamplePath {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub(super) fn child(&self, segment: impl std::fmt::Display) -> Self {
        Self(format!("{}/{segment}", self.0))
    }
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SampleStatus {
    Completed,
    Stopped,
}
#[derive(Debug, Clone, Serialize)]
pub struct SampleStep {
    pub path: SamplePath,
    pub workflow_id: WorkflowId,
    pub step: StepId,
    pub kind: crate::domain::workflow::TypeName,
    pub inputs: Value,
    pub output: Value,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SampleEffectKind {
    FixtureRead,
    InterceptedWrite,
    WorkflowChild,
}
#[derive(Debug, Clone, Serialize)]
pub struct SampleEffect {
    pub path: SamplePath,
    pub operation: crate::domain::workflow::TypeName,
    pub kind: SampleEffectKind,
    pub input: Value,
    pub output: Value,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct SampleUsage {
    pub activations: u32,
    pub model_calls: u32,
    pub repetitions: u32,
    pub scripted_tokens: u32,
    pub direct_tool_calls: usize,
}
#[derive(Debug, Serialize)]
pub struct SampleReport {
    pub content_hash: super::super::publication::ContentHash,
    pub status: SampleStatus,
    pub output: Option<Value>,
    pub steps: Vec<SampleStep>,
    pub effects: Vec<SampleEffect>,
    pub usage: SampleUsage,
    pub unused_responses: usize,
    pub diagnostic: Option<String>,
}
