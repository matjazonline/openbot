//! Offering our own tools to the `ai-agents` runtime.
//!
//! One generic shim, parameterised by the tool's [`NativeToolDeclaration`], rather than three
//! near-identical `impl Tool` blocks. Each of our tools is described once, beside its
//! implementation, and everything the runtime needs -- the id it answers to, the schema the model
//! is shown, the safety policy the executor applies -- is derived from that description here.
//!
//! Nothing in this file decides anything. Adding a fourth native tool means adding a declaration
//! and a `HarnessToolHost` arm; it does not mean touching this module.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ai_agents::{
    Tool, ToolResult,
    tools::{ToolExecutionContext, ToolOperationKind, ToolSafetyMetadata, ToolSideEffectLevel},
};
use async_trait::async_trait;
use serde_json::Value;

use crate::services::harness::{HarnessToolHost, NativeToolDeclaration, NativeToolSafety};

/// One of our tools, as the runtime sees it.
pub struct NativeToolShim {
    declaration: NativeToolDeclaration,
    /// Shared with the run rather than owned: the host is what holds the persistence handles and
    /// the task context, and every tool of one run dispatches through the same one.
    host: Arc<dyn HarnessToolHost>,
    suspended: Arc<AtomicBool>,
}

impl NativeToolShim {
    pub fn new(
        declaration: NativeToolDeclaration,
        host: Arc<dyn HarnessToolHost>,
        suspended: Arc<AtomicBool>,
    ) -> Self {
        Self {
            declaration,
            host,
            suspended,
        }
    }
}

/// Our safety vocabulary in the runtime's.
///
/// The four fields this platform does not model are stated here once, explicitly, rather than
/// left to a `Default` that is documented as "conservative unknown". Each is a fact about every
/// native tool rather than a guess: none waits on a terminal user, none can be cancelled
/// mid-flight, none has a schema large enough to defer, and all of them need this host to run at
/// all. A tool that changes one of those is a field to add to [`NativeToolSafety`], not a value to
/// override here.
fn safety_metadata(safety: NativeToolSafety) -> ToolSafetyMetadata {
    ToolSafetyMetadata {
        read_only: safety.read_only,
        concurrency_safe: safety.concurrency_safe,
        operation: if safety.read_only {
            ToolOperationKind::Read
        } else {
            ToolOperationKind::Write
        },
        side_effect_level: match (safety.destructive, safety.has_external_effect) {
            (true, _) => ToolSideEffectLevel::Destructive,
            (false, true) => ToolSideEffectLevel::ExternalWrite,
            (false, false) if safety.read_only => ToolSideEffectLevel::None,
            (false, false) => ToolSideEffectLevel::LocalWrite,
        },
        requires_network: safety.requires_network,
        destructive: safety.destructive,
        open_world: safety.open_world,
        host_dependent: true,
        requires_user_interaction: false,
        supports_cancellation: false,
        default_requires_approval: safety.requires_approval_by_default,
        should_defer_schema: false,
        max_output_chars: Some(safety.max_output_chars),
        max_result_size_chars: Some(safety.max_result_chars),
    }
}

#[async_trait]
impl Tool for NativeToolShim {
    fn id(&self) -> &str {
        self.declaration.id.as_str()
    }

    fn name(&self) -> &str {
        self.declaration.name
    }

    fn description(&self) -> &str {
        self.declaration.description
    }

    fn input_schema(&self) -> Value {
        self.declaration.input_schema.clone()
    }

    fn safety_metadata(&self) -> ToolSafetyMetadata {
        safety_metadata(self.declaration.safety)
    }

    /// The runtime's execution context is deliberately dropped.
    ///
    /// Its `custom_config` is this runtime's rendering of the same tool policy the host was
    /// already built with, and reading it here would give one tool two sources of its own bounds
    /// -- which differ the moment a second harness compiles the policy differently. The host's is
    /// the one that applies whichever runtime is calling.
    async fn execute(&self, args: Value, _ctx: ToolExecutionContext) -> ToolResult {
        match self.host.invoke(&self.declaration.id, args).await {
            Ok(invocation) if invocation.success => {
                if invocation.suspends_run() {
                    self.suspended.store(true, Ordering::SeqCst);
                }
                ToolResult::ok(invocation.render())
            }
            // A tool that ran and said no: the model reads the reason and may try something else.
            Ok(invocation) => ToolResult::error(invocation.render()),
            // A tool that could not run at all. It is still reported to the model as a failed call
            // rather than propagated, because ending the whole run on one unavailable tool would
            // throw away everything the agent had already done.
            Err(error) => ToolResult::error(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::app_error::AppResult;

    use crate::services::agent_directory_tool::ListCompanyAgentsTool;
    use crate::services::outreach_tool::OutreachAndAwaitQuorumTool;

    /// A read-only tool with no effect outside this process, and a tool that mails strangers,
    /// must not land on the same policy. This is the mapping that decides which.
    #[test]
    fn a_read_only_tool_and_an_externally_effecting_one_map_to_different_policies() {
        let directory = safety_metadata(ListCompanyAgentsTool::declaration().safety);
        assert!(directory.read_only);
        assert!(directory.concurrency_safe);
        assert_eq!(directory.operation, ToolOperationKind::Read);
        assert_eq!(directory.side_effect_level, ToolSideEffectLevel::None);
        assert!(!directory.default_requires_approval);

        let outreach = safety_metadata(OutreachAndAwaitQuorumTool::declaration().safety);
        assert!(!outreach.read_only);
        assert!(!outreach.concurrency_safe);
        assert_eq!(outreach.operation, ToolOperationKind::Write);
        assert_eq!(
            outreach.side_effect_level,
            ToolSideEffectLevel::ExternalWrite
        );
        assert!(outreach.requires_network);
        assert!(outreach.open_world);
        assert!(outreach.default_requires_approval);
    }

    /// The bounds reach the executor rather than staying documentation.
    #[test]
    fn the_declared_output_bounds_are_the_ones_the_executor_applies() {
        for declaration in [
            OutreachAndAwaitQuorumTool::declaration(),
            ListCompanyAgentsTool::declaration(),
        ] {
            let metadata = safety_metadata(declaration.safety);
            assert_eq!(
                metadata.max_output_chars,
                Some(declaration.safety.max_output_chars)
            );
            assert_eq!(
                metadata.max_result_size_chars,
                Some(declaration.safety.max_result_chars)
            );
        }
    }

    /// A destructive tool would outrank an externally-effecting one, whichever order the two
    /// flags were set in. None of ours is; the arm exists so that the day one is, the policy is
    /// already right.
    #[test]
    fn a_destructive_tool_outranks_an_external_write() {
        let mut safety = OutreachAndAwaitQuorumTool::declaration().safety;
        safety.destructive = true;

        assert_eq!(
            safety_metadata(safety).side_effect_level,
            ToolSideEffectLevel::Destructive
        );
    }

    struct SuspendingHost {
        declarations: Vec<NativeToolDeclaration>,
    }

    #[async_trait]
    impl HarnessToolHost for SuspendingHost {
        fn available(&self) -> &[NativeToolDeclaration] {
            &self.declarations
        }

        async fn invoke(
            &self,
            _id: &crate::entities::value_objects::ToolId,
            _args: Value,
        ) -> AppResult<crate::services::harness::ToolInvocation> {
            Ok(crate::services::harness::ToolInvocation::suspended(
                serde_json::json!({ "status": "waiting" }),
            ))
        }
    }

    #[tokio::test]
    async fn a_suspending_native_tool_parks_the_adapter_run() {
        let declaration = ListCompanyAgentsTool::declaration();
        let suspended = Arc::new(AtomicBool::new(false));
        let host = Arc::new(SuspendingHost {
            declarations: vec![declaration.clone()],
        });
        let shim = NativeToolShim::new(declaration.clone(), host, suspended.clone());

        let result = shim
            .execute(
                Value::Null,
                ToolExecutionContext::test(declaration.id.as_str()),
            )
            .await;

        assert!(result.success);
        assert!(suspended.load(Ordering::SeqCst));
    }
}
