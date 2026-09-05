//! The `ai-agents` side of [`HarnessTrace`].
//!
//! Every hook the runtime fires becomes one call on the port, in the port's own vocabulary. The
//! only genuinely runtime-shaped thing being translated is *which path* asked for a tool call, and
//! that is restated as [`ToolTraceSource`] so the label set stays bounded across harnesses -- these
//! become metric labels, and a per-run string would be a cardinality explosion.

use std::sync::Arc;

use ai_agents::{
    AgentHooks,
    tools::{ToolCallSource, ToolExecutionRecord, ToolResult},
};
use async_trait::async_trait;
use serde_json::Value;

use crate::entities::value_objects::ToolId;
use crate::services::harness::{HarnessTrace, ToolTraceOutcome, ToolTraceRecord, ToolTraceSource};

pub struct AiAgentsTraceShim {
    trace: Arc<dyn HarnessTrace>,
}

impl AiAgentsTraceShim {
    pub fn new(trace: Arc<dyn HarnessTrace>) -> Self {
        Self { trace }
    }
}

fn trace_source(source: &ToolCallSource) -> ToolTraceSource {
    match source {
        ToolCallSource::Model => ToolTraceSource::Model,
        ToolCallSource::Skill { .. } => ToolTraceSource::Skill,
        ToolCallSource::StateAction { .. } => ToolTraceSource::StateAction,
        ToolCallSource::Plan { .. } => ToolTraceSource::Plan,
        ToolCallSource::Orchestration => ToolTraceSource::Orchestration,
        ToolCallSource::Spawner => ToolTraceSource::Spawner,
        // A path this port does not name yet. Degrading to a known label is what stops a runtime
        // upgrade turning into a release here.
        _ => ToolTraceSource::Other,
    }
}

/// How a finished call ended, in the order the runtime's own fields have to be read: a cancelled
/// call may also be marked unsuccessful, so the more specific reason wins.
fn trace_outcome(record: &ToolExecutionRecord) -> ToolTraceOutcome {
    if record.cancelled {
        ToolTraceOutcome::Cancelled
    } else if record.timed_out {
        ToolTraceOutcome::TimedOut
    } else if !record.executed {
        // Blocked before the implementation ran: policy refused it, or approval did.
        ToolTraceOutcome::NotExecuted
    } else if record.success {
        ToolTraceOutcome::Success
    } else {
        ToolTraceOutcome::Failed
    }
}

#[async_trait]
impl AgentHooks for AiAgentsTraceShim {
    async fn on_tool_start(&self, tool: &str, args: &Value) {
        self.trace.tool_started(&ToolId::from(tool), args).await;
    }

    async fn on_tool_complete(&self, _tool: &str, _result: &ToolResult, _duration_ms: u64) {
        // Intentionally empty: `on_tool_execution_record` describes the same logical request with
        // strictly more evidence, and firing both would double every tool line in the log.
    }

    async fn on_tool_execution_record(&self, record: &ToolExecutionRecord) {
        let tool = ToolId::from(record.canonical_id.as_str());
        let policy = format!("{:?}", record.policy.outcome);
        let approval = record
            .approval
            .as_ref()
            .map(|approval| format!("{:?}", approval.status));
        self.trace
            .tool_finished(ToolTraceRecord {
                tool: &tool,
                call_id: &record.call_id,
                source: trace_source(&record.source),
                outcome: trace_outcome(record),
                executed: record.executed,
                duration_ms: record.duration_ms,
                output_bytes: record.output.len(),
                output_truncated: record.output_truncated,
                policy: Some(policy.as_str()),
                approval: approval.as_deref(),
                cancellation_reason: record.cancellation_reason.as_deref(),
            })
            .await;
    }

    async fn on_approval_requested(&self, request: &ai_agents::hitl::ApprovalRequest) {
        self.trace.approval_requested(&request.id).await;
    }

    async fn on_error(&self, error: &ai_agents::AgentError) {
        self.trace.run_failed(&error.to_string()).await;
    }

    async fn on_handoff(&self, from: &str, to: &str, reason: &str) {
        self.trace.handoff(from, to, reason).await;
    }

    async fn on_delegate_complete(&self, agent_id: &str, state: &str, duration_ms: u64) {
        self.trace
            .delegate_finished(agent_id, state, duration_ms)
            .await;
    }
}
