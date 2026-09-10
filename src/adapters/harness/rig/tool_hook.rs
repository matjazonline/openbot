//! Rig 0.42 exposes correlation IDs on hooks, not DynamicTool's callback context.
//! A single-use handoff binds the exact canonical ID and arguments to that correlator.
use super::tools::{MAX_TOOL_ARGUMENT_BYTES, ToolBridge, ToolCorrelationId, ToolStopReason};
use crate::entities::value_objects::ToolId;
use rig::{
    agent::{
        Agent, AgentBuilder, AgentHook, HookContext, ToolCall, ToolCallAction, ToolResultAction,
        ToolResultEvent,
    },
    tool::ToolExecutionError,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};

struct PendingCall {
    id: ToolId,
    args: Value,
    correlation: ToolCorrelationId,
}

#[derive(Default)]
pub(super) struct ToolHandoff(Mutex<Option<PendingCall>>);

impl ToolHandoff {
    pub(super) fn take(
        &self,
        id: &ToolId,
        args: &Value,
    ) -> Result<ToolCorrelationId, ToolExecutionError> {
        let pending = self
            .0
            .lock()
            .map_err(|_| ToolExecutionError::refused("Tool handoff unavailable"))?
            .take();
        match pending {
            Some(call) if call.id == *id && call.args == *args => Ok(call.correlation),
            _ => Err(ToolExecutionError::refused(
                "Missing or mismatched guarded tool call",
            )),
        }
    }
}

struct BridgeHook {
    bridge: Arc<ToolBridge>,
    handoff: Arc<ToolHandoff>,
}

impl AgentHook for BridgeHook {
    async fn on_invalid_tool_call(
        &self,
        _: &HookContext,
        _: &rig::agent::InvalidToolCallContext,
    ) -> Option<rig::agent::InvalidToolCallAction> {
        self.bridge.stop(ToolStopReason::Protocol).await;
        Some(rig::agent::InvalidToolCallAction::stop(
            "Invalid provider tool call",
        ))
    }
    async fn on_tool_call(&self, _: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        if self.bridge.stop_reason().await.is_some() {
            return ToolCallAction::stop("Tool execution stopped");
        }
        if event.args.len() > MAX_TOOL_ARGUMENT_BYTES {
            self.bridge.stop(ToolStopReason::Protocol).await;
            return ToolCallAction::stop("Tool argument byte budget exceeded");
        }
        let Ok(correlation) = ToolCorrelationId::parse(event.internal_call_id) else {
            self.bridge.stop(ToolStopReason::Protocol).await;
            return ToolCallAction::stop("Invalid tool correlation ID");
        };
        let Ok(args) = serde_json::from_str(event.args) else {
            return ToolCallAction::stop("Malformed tool arguments");
        };
        let Ok(mut pending) = self.handoff.0.lock() else {
            return ToolCallAction::stop("Tool handoff unavailable");
        };
        if pending.is_some() {
            return ToolCallAction::stop("Concurrent tool handoff is unsupported");
        }
        *pending = Some(PendingCall {
            id: event.tool_name.into(),
            args,
            correlation,
        });
        ToolCallAction::Run
    }

    async fn on_tool_result(&self, _: &HookContext, _: ToolResultEvent<'_>) -> ToolResultAction {
        if let Ok(mut pending) = self.handoff.0.lock() {
            pending.take();
        }
        if self.bridge.stop_reason().await.is_some() {
            ToolResultAction::stop("Tool execution stopped")
        } else {
            ToolResultAction::Keep
        }
    }
}

impl ToolBridge {
    /// Install the declarations and their mandatory hook together. Rig 0.42 defaults to
    /// sequential execution; callers must retain `tool_concurrency(1)` on prompt requests.
    pub fn build_agent(self: &Arc<Self>, builder: AgentBuilder) -> Agent {
        let handoff = Arc::new(ToolHandoff::default());
        let builder = builder.dynamic_tools(self.declarations(&handoff));
        builder
            .add_hook(BridgeHook {
                bridge: self.clone(),
                handoff,
            })
            .build()
    }
}
