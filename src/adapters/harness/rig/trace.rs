//! The single Rig-to-application trace boundary. All labels are closed or catalogue-owned.
use crate::{
    app_error::{AppError, AppResult},
    entities::value_objects::ToolId,
    services::harness::{
        HarnessTrace, ToolInvocation, ToolTraceOutcome, ToolTraceRecord, ToolTraceSource,
    },
};
use futures::FutureExt;
use std::{future::Future, panic::AssertUnwindSafe, sync::Arc, time::Duration};

// Optional instrumentation cannot hold a lease indefinitely or turn a committed result into retry.
pub(super) async fn optional(callback: impl Future<Output = ()>) {
    let _ = tokio::time::timeout(
        Duration::from_millis(10),
        AssertUnwindSafe(callback).catch_unwind(),
    )
    .await;
}

pub(super) struct ToolTrace {
    trace: Option<Arc<dyn HarnessTrace>>,
    tool: ToolId,
    call_id: String,
    started: std::time::Instant,
    pub executed: bool,
    pub approval: Option<&'static str>,
    pub output_truncated: bool,
    finished: bool,
}
impl ToolTrace {
    pub fn new(trace: Option<Arc<dyn HarnessTrace>>, tool: ToolId, call_id: &str) -> Self {
        Self {
            trace,
            tool,
            // Correlation can also enter through the standalone bridge API. Never trust its text.
            call_id: uuid::Uuid::parse_str(call_id)
                .map_or_else(|_| "unavailable".into(), |id| id.to_string()),
            started: std::time::Instant::now(),
            executed: false,
            approval: None,
            output_truncated: false,
            finished: false,
        }
    }
    pub async fn started(&mut self, argument_count: usize) {
        if let Some(trace) = &self.trace {
            optional(trace.tool_started(&self.tool, argument_count)).await;
        }
        // Cancellation while delivering the start callback has not entered the implementation.
        self.executed = true;
    }
    pub async fn approval_requested(&self) {
        if let Some(trace) = &self.trace {
            optional(trace.approval_requested(&self.call_id)).await;
        }
    }
    pub async fn finish(mut self, result: &AppResult<ToolInvocation>) {
        // Mark first: dropping an optional callback is not cancellation of the actual tool.
        self.finished = true;
        let outcome = match result {
            Err(AppError::Timeout(_)) => ToolTraceOutcome::TimedOut,
            Ok(value) if value.suspends_run() => ToolTraceOutcome::Suspended,
            _ if !self.executed => ToolTraceOutcome::NotExecuted,
            Ok(value) if value.success => ToolTraceOutcome::Success,
            _ => ToolTraceOutcome::Failed,
        };
        if let Some(trace) = &self.trace {
            optional(trace.tool_finished(ToolTraceRecord {
                tool: &self.tool,
                call_id: &self.call_id,
                source: ToolTraceSource::Model,
                outcome,
                executed: self.executed,
                duration_ms: self.started.elapsed().as_millis() as u64,
                output_bytes: result.as_ref().map_or(0, |result| result.render().len()),
                output_truncated: self.output_truncated,
                policy: Some(if self.executed {
                    "allowed"
                } else {
                    "not_executed"
                }),
                approval: self.approval,
            }))
            .await;
        }
    }
}
impl Drop for ToolTrace {
    fn drop(&mut self) {
        if !self.finished && self.trace.is_some() {
            // Cancellation cannot await a callback. Emit into the caller's correlation span;
            // never detach work merely to deliver telemetry after lease loss.
            tracing::warn!(target: "trace::tool", tool = %self.tool, call_id = %self.call_id,
                executed = self.executed, outcome = "cancelled", "Rig tool call cancelled");
        }
    }
}
