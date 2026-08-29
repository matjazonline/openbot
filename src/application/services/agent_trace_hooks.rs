//! Per-action tracing for an agent run.
//!
//! The task span says a run happened; this says what the run *did*. Every tool the agent reaches
//! for -- ours, the runtime's built-ins, and anything reached over MCP -- passes through
//! [`ai_agents::AgentHooks`], so one implementation covers them all without each tool having to
//! remember to log itself.
//!
//! [`AgentHooks::on_tool_execution_record`] is the authoritative callback: it fires once per
//! logical executor request, with retries folded in, and `executed` is the only trustworthy
//! statement that the tool implementation actually ran. `on_tool_start` may be skipped entirely,
//! so nothing here depends on having seen it.
//!
//! # What is deliberately not logged
//!
//! Arguments and outputs. They carry whatever the sender wrote -- an address book, an invoice, the
//! body of somebody's email -- and `src/AGENTS.md` rules out putting message bodies in spans. The
//! agent config says the same thing to the runtime's own observability
//! (`include_tool_args: false`, `include_tool_outputs: false`), so logging them here would be a
//! privacy hole opened behind a setting that says it is closed. What is recorded instead is the
//! shape: which tool, whether it ran, whether it succeeded, how long it took, how much it
//! returned, and how policy and approval treated it.

use std::sync::Arc;

use ai_agents::{
    AgentHooks,
    tools::{ToolCallSource, ToolExecutionRecord, ToolResult},
};
use async_trait::async_trait;
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::monitoring::MonitoringService;
use crate::entities::correlation::CorrelationId;

/// Which run an action belongs to. Every field is an identifier or a count, so the whole struct is
/// safe to attach to a log line.
#[derive(Debug, Clone)]
pub struct AgentTraceContext {
    pub correlation_id: CorrelationId,
    pub task_id: Option<Uuid>,
    pub company_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
}

pub struct AgentTraceHooks {
    context: AgentTraceContext,
    monitoring: Option<Arc<dyn MonitoringService>>,
}

impl AgentTraceHooks {
    pub fn new(context: AgentTraceContext, monitoring: Option<Arc<dyn MonitoringService>>) -> Self {
        Self {
            context,
            monitoring,
        }
    }

    /// Counter labels. Bounded on purpose: tool ids come from the registry and outcomes from a
    /// closed set, so this cannot become a per-message cardinality explosion the way a task id or
    /// an error string would.
    fn count(&self, metric: &str, labels: &[(&str, &str)]) {
        if let Some(monitoring) = self.monitoring.as_ref() {
            monitoring.increment_counter(metric, 1, labels);
        }
    }
}

/// The runtime path that asked for a call, as a bounded label.
fn source_label(source: &ToolCallSource) -> &'static str {
    match source {
        ToolCallSource::Model => "model",
        ToolCallSource::Skill { .. } => "skill",
        ToolCallSource::StateAction { .. } => "state_action",
        ToolCallSource::Plan { .. } => "plan",
        ToolCallSource::Orchestration => "orchestration",
        ToolCallSource::Spawner => "spawner",
        _ => "other",
    }
}

/// How a finished call ended, in one word, for metric labels and for reading a log at a glance.
fn outcome_label(record: &ToolExecutionRecord) -> &'static str {
    if record.cancelled {
        "cancelled"
    } else if record.timed_out {
        "timed_out"
    } else if !record.executed {
        // Blocked before the implementation ran: policy refused it, or approval did.
        "not_executed"
    } else if record.success {
        "success"
    } else {
        "failed"
    }
}

/// The argument *names* an object-shaped call carries. Names come from the tool's own JSON schema,
/// not from the sender, so they say which variant of a call this was without quoting anybody.
fn argument_keys(args: &Value) -> Vec<&str> {
    args.as_object()
        .map(|map| map.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

#[async_trait]
impl AgentHooks for AgentTraceHooks {
    async fn on_tool_start(&self, tool: &str, args: &Value) {
        info!(
            target: "trace::tool",
            correlation_id = %self.context.correlation_id,
            task_id = ?self.context.task_id,
            agent_id = ?self.context.agent_id,
            tool = %tool,
            argument_keys = ?argument_keys(args),
            "Tool call started"
        );
    }

    async fn on_tool_complete(&self, _tool: &str, _result: &ToolResult, _duration_ms: u64) {
        // Intentionally empty: `on_tool_execution_record` describes the same logical request with
        // strictly more evidence, and firing both would double every tool line in the log.
    }

    async fn on_tool_execution_record(&self, record: &ToolExecutionRecord) {
        let outcome = outcome_label(record);
        let source = source_label(&record.source);
        self.count(
            "agent_tool_calls_total",
            &[
                ("tool", record.canonical_id.as_str()),
                ("outcome", outcome),
                ("source", source),
            ],
        );

        // A call that never reached the tool is an operational event, not routine chatter: it
        // means policy or an approval stopped the agent doing what it decided to do.
        if record.executed && record.success {
            info!(
                target: "trace::tool",
                correlation_id = %self.context.correlation_id,
                task_id = ?self.context.task_id,
                company_id = ?self.context.company_id,
                channel_id = ?self.context.channel_id,
                agent_id = ?self.context.agent_id,
                tool = %record.canonical_id,
                call_id = %record.call_id,
                source = %source,
                outcome = %outcome,
                duration_ms = record.duration_ms,
                output_bytes = record.output.len(),
                output_truncated = record.output_truncated,
                "Tool call finished"
            );
        } else {
            warn!(
                target: "trace::tool",
                correlation_id = %self.context.correlation_id,
                task_id = ?self.context.task_id,
                company_id = ?self.context.company_id,
                channel_id = ?self.context.channel_id,
                agent_id = ?self.context.agent_id,
                tool = %record.canonical_id,
                call_id = %record.call_id,
                source = %source,
                outcome = %outcome,
                executed = record.executed,
                duration_ms = record.duration_ms,
                policy = ?record.policy.outcome,
                approval = ?record.approval.as_ref().map(|approval| &approval.status),
                cancellation_reason = ?record.cancellation_reason,
                "Tool call did not succeed"
            );
        }
    }

    async fn on_approval_requested(&self, request: &ai_agents::hitl::ApprovalRequest) {
        info!(
            target: "trace::approval",
            correlation_id = %self.context.correlation_id,
            task_id = ?self.context.task_id,
            request_id = %request.id,
            "Agent run parked awaiting human approval"
        );
    }

    async fn on_error(&self, error: &ai_agents::AgentError) {
        warn!(
            target: "trace::agent",
            correlation_id = %self.context.correlation_id,
            task_id = ?self.context.task_id,
            agent_id = ?self.context.agent_id,
            error = %error,
            "Agent run reported an error"
        );
    }

    async fn on_handoff(&self, from: &str, to: &str, reason: &str) {
        info!(
            target: "trace::agent",
            correlation_id = %self.context.correlation_id,
            task_id = ?self.context.task_id,
            from = %from,
            to = %to,
            reason = %reason,
            "Control handed to another agent"
        );
    }

    async fn on_delegate_complete(&self, agent_id: &str, state: &str, duration_ms: u64) {
        info!(
            target: "trace::agent",
            correlation_id = %self.context.correlation_id,
            task_id = ?self.context.task_id,
            delegate_agent = %agent_id,
            state = %state,
            duration_ms = duration_ms,
            "Delegated step finished"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn argument_names_are_reported_but_values_never_are() {
        let args = json!({ "target_emails": ["someone@example.com"], "subject": "Invoice 41" });
        // Order is serde_json's business, not ours; what matters is which names appear.
        assert_eq!(
            argument_keys(&args).into_iter().collect::<HashSet<_>>(),
            HashSet::from(["target_emails", "subject"])
        );
        // The values are the sender's content, and nothing here can reach them.
        let rendered = format!("{:?}", argument_keys(&args));
        assert!(!rendered.contains("someone@example.com"));
        assert!(!rendered.contains("Invoice 41"));
    }

    #[test]
    fn a_non_object_argument_yields_no_names_rather_than_its_contents() {
        assert!(argument_keys(&json!("a bare string")).is_empty());
        assert!(argument_keys(&json!(null)).is_empty());
    }
}
