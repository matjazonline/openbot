use super::*;
use serde::{Deserialize, Serialize};

pub const MAX_SAMPLE_BYTES: usize = 256 * 1024;
pub const MAX_SAMPLE_STEPS: u32 = 128;
pub(super) const MAX_REPORT_BYTES: usize = 1024 * 1024;
pub(super) const MAX_DEPTH: usize = 8;
pub(super) const MAX_TOOL_CALLS: usize = 64;

/// Only inert data crosses the sample boundary. No production binding or continuation is accepted.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRequest {
    #[serde(default)]
    pub response_contract: Option<super::super::channel_reply::ChannelReplyContract>,
    #[serde(default)]
    pub response_approval_policy: Option<super::super::channel_reply::ReplyApprovalPolicy>,
    pub agents: Vec<crate::application::agent::ResolvedAgentDefinition>,
    pub input: Value,
    pub params: Value,
    #[serde(default)]
    pub scripts: Vec<StepScript>,
    /// The desks a scripted `channel.call` may reach, at most [`MAX_CALL_TARGETS`]; capture
    /// freezes each one (gate AWC-P05-D2 r2, D3).
    #[serde(default)]
    pub call_targets: Vec<Uuid>,
}

/// The most desks one sample may declare as call targets.
pub const MAX_CALL_TARGETS: usize = 10;

/// What capture froze of a declared call target: the launch facts the simulation judges with,
/// the declared intake and result fields, and whether final review applies. The responder
/// binding is part of the identity only; the report never prints a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrozenCallTarget {
    pub desk: Uuid,
    pub enabled: bool,
    pub unit_archived: bool,
    pub responder: Option<FrozenResponder>,
    /// The sample's agents that may write to the desk (check 3).
    pub writable_agents: BTreeSet<crate::entities::agent_capability::AgentId>,
    pub review_required: bool,
    pub intake: Option<Value>,
    pub result_fields: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrozenResponder {
    pub binding: Uuid,
    pub revision: u64,
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
    /// `channel.call` only: the simulated final review of a desk that requires one.
    #[serde(default)]
    pub approval: Option<bool>,
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
    pub history: Option<SampleAgentHistory>,
}

/// Scripted, sample-local context. It never names a production conversation.
#[derive(Debug, Clone, Serialize)]
pub struct SampleAgentHistory {
    pub agent: crate::entities::agent_capability::AgentId,
    pub seed: crate::application::services::harness::runs::LogicalSeed,
    pub omitted_pairs: usize,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SampleEffectKind {
    FixtureRead,
    InterceptedWrite,
    WorkflowChild,
    /// A `channel.call` answered from the author's fixture, not by any desk (gate D4).
    SimulatedCall,
}
#[derive(Debug, Clone, Serialize)]
pub struct SampleEffect {
    pub path: SamplePath,
    pub operation: crate::domain::workflow::TypeName,
    pub kind: SampleEffectKind,
    pub input: Value,
    pub output: Value,
    /// Set for a simulated call: the output is untrusted author data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<&'static str>,
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
    pub response: Option<SampleResponse>,
    pub content_hash: super::super::publication::ContentHash,
    pub configuration_hash: super::SampleConfigurationHash,
    pub agent_selections: Vec<super::SampleAgentSelection>,
    pub workflow_selections: Vec<super::SampleWorkflowSelection>,
    pub status: SampleStatus,
    pub output: Option<Value>,
    pub steps: Vec<SampleStep>,
    pub effects: Vec<SampleEffect>,
    pub usage: SampleUsage,
    pub unused_responses: usize,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SampleResponse {
    AutomaticUnmapped,
    Automatic {
        content: SampleResponseContent,
        approval_required: bool,
    },
    Suppressed,
    Waiting {
        stage: SampleWaitStage,
        preview: Value,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleWaitStage {
    HumanReply,
    FormalReview,
    QuestionApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleResponseContent {
    pub subject: String,
    pub body: String,
}

impl From<crate::application::transport::CanonicalContent> for SampleResponseContent {
    fn from(content: crate::application::transport::CanonicalContent) -> Self {
        Self {
            subject: content.subject().to_owned(),
            body: content.body_text().to_owned(),
        }
    }
}
