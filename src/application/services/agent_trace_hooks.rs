//! Per-action tracing for an agent run.
//!
//! The task span says a run happened; this says what the run *did*. Every tool the agent reaches
//! for -- ours, the runtime's built-ins, and anything reached over MCP -- passes through
//! [`HarnessTrace`], so one implementation covers them all without each tool having to remember
//! to log itself.
//!
//! [`HarnessTrace::tool_finished`] is the authoritative callback: it fires once per logical
//! executor request, with retries folded in, and `executed` is the only trustworthy statement
//! that the tool implementation actually ran. [`HarnessTrace::tool_started`] may be skipped
//! entirely, so nothing here depends on having seen it.
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

use async_trait::async_trait;
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::monitoring::MonitoringService;
use crate::entities::{correlation::CorrelationId, value_objects::ToolId};
use crate::services::harness::{HarnessTrace, ToolTraceRecord};

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

/// The argument *names* an object-shaped call carries. Names come from the tool's own JSON schema,
/// not from the sender, so they say which variant of a call this was without quoting anybody.
fn argument_keys(args: &Value) -> Vec<&str> {
    args.as_object()
        .map(|map| map.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

#[async_trait]
impl HarnessTrace for AgentTraceHooks {
    async fn tool_started(&self, tool: &ToolId, args: &Value) {
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

    async fn tool_finished(&self, record: ToolTraceRecord<'_>) {
        let outcome = record.outcome.label();
        let source = record.source.label();
        self.count(
            "agent_tool_calls_total",
            &[
                ("tool", record.tool.as_str()),
                ("outcome", outcome),
                ("source", source),
            ],
        );

        // A call that never reached the tool is an operational event, not routine chatter: it
        // means policy or an approval stopped the agent doing what it decided to do.
        if record.executed && record.outcome.is_success() {
            info!(
                target: "trace::tool",
                correlation_id = %self.context.correlation_id,
                task_id = ?self.context.task_id,
                company_id = ?self.context.company_id,
                channel_id = ?self.context.channel_id,
                agent_id = ?self.context.agent_id,
                tool = %record.tool,
                call_id = %record.call_id,
                source = %source,
                outcome = %outcome,
                duration_ms = record.duration_ms,
                output_bytes = record.output_bytes,
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
                tool = %record.tool,
                call_id = %record.call_id,
                source = %source,
                outcome = %outcome,
                executed = record.executed,
                duration_ms = record.duration_ms,
                // Flattened rather than `?`-formatted: these are closed-set labels, and
                // `Some("Denied")` in a log line is a Rust rendering, not a fact about the call.
                policy = record.policy.unwrap_or("unreported"),
                approval = record.approval.unwrap_or("not_checked"),
                cancellation_reason = record.cancellation_reason.unwrap_or("none"),
                "Tool call did not succeed"
            );
        }
    }

    async fn approval_requested(&self, request_id: &str) {
        info!(
            target: "trace::approval",
            correlation_id = %self.context.correlation_id,
            task_id = ?self.context.task_id,
            request_id = %request_id,
            "Agent run parked awaiting human approval"
        );
    }

    async fn run_failed(&self, error: &str) {
        warn!(
            target: "trace::agent",
            correlation_id = %self.context.correlation_id,
            task_id = ?self.context.task_id,
            agent_id = ?self.context.agent_id,
            error = %error,
            "Agent run reported an error"
        );
    }

    async fn handoff(&self, from: &str, to: &str, reason: &str) {
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

    async fn delegate_finished(&self, agent: &str, state: &str, duration_ms: u64) {
        info!(
            target: "trace::agent",
            correlation_id = %self.context.correlation_id,
            task_id = ?self.context.task_id,
            delegate_agent = %agent,
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
