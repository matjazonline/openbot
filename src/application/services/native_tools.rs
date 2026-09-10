//! Our own tools, assembled for one run and offered to whichever harness is executing it.
//!
//! This is the application's implementation of [`HarnessToolHost`]. It exists so that "which of
//! our tools can this run actually serve" is answered once, here, from the contexts the run was
//! given -- and not a second time inside each harness adapter from whatever handles it happens to
//! hold.
//!
//! Registration is not a grant. A tool being available here only means the run *could* serve it;
//! whether the agent may call it is decided by the capability spec's grant list. Availability
//! bounds that list rather than adding to it -- a native grant this run cannot serve is dropped
//! and reported, because offering the model a tool that fails on use is worse than not offering
//! it. `docs/custom_tools.md` says the same thing from the tool author's side.

use async_trait::async_trait;
use serde_json::Value;

use crate::app_error::{AppError, AppResult};
use crate::entities::{
    tool_catalogue::{
        AGENT_DIRECTORY_TOOL_ID, CREATE_AGENT_CHANNEL_TOOL_ID, OUTREACH_TOOL_ID,
        REQUEST_APPROVAL_TOOL_ID, TASK_OWNERSHIP_TOOL_ID,
    },
    value_objects::ToolId,
};
use crate::services::{
    agent_channel_tool::CreateAgentChannelTool,
    agent_directory_tool::ListCompanyAgentsTool,
    approval_tool::RequestApprovalTool,
    harness::{HarnessToolHost, NativeToolDeclaration, ToolInvocation},
    outreach_tool::OutreachAndAwaitQuorumTool,
    task_ownership_tool::TaskOwnershipTool,
};

/// The native tools one run can serve.
///
/// Each field is `None` when the run was not given the context that tool needs -- no durable task
/// means no outreach, no provisioning port means no new channels. That is the same coupling the
/// runner has always had; what changes is that it is now stated once, as data, instead of being
/// re-derived by whoever wires a runtime.
#[derive(Default)]
pub struct NativeToolHost {
    outreach: Option<OutreachAndAwaitQuorumTool>,
    directory: Option<ListCompanyAgentsTool>,
    channels: Option<CreateAgentChannelTool>,
    ownership: Option<TaskOwnershipTool>,
    approval: Option<RequestApprovalTool>,
    /// Built once at construction, in catalogue order, because a harness reads it per run and
    /// each entry carries a generated JSON schema.
    declarations: Vec<NativeToolDeclaration>,
}

impl NativeToolHost {
    pub fn with_approval_checkpoint(mut self, tool: RequestApprovalTool) -> Self {
        self.approval = Some(tool);
        self.rebuild_declarations();
        self
    }
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_outreach(mut self, tool: OutreachAndAwaitQuorumTool) -> Self {
        self.outreach = Some(tool);
        self.rebuild_declarations();
        self
    }

    pub fn with_directory(mut self, tool: ListCompanyAgentsTool) -> Self {
        self.directory = Some(tool);
        self.rebuild_declarations();
        self
    }

    pub fn with_agent_channels(mut self, tool: CreateAgentChannelTool) -> Self {
        self.channels = Some(tool);
        self.rebuild_declarations();
        self
    }

    pub fn with_task_ownership(mut self, tool: TaskOwnershipTool) -> Self {
        self.ownership = Some(tool);
        self.rebuild_declarations();
        self
    }

    /// Whether this run has any native tool at all. The runner skips attaching the host when not,
    /// so a harness is never handed an empty one to reason about.
    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }

    /// The declaration order is the catalogue's, so a compiled tool grant reads the same way
    /// twice for the same run.
    fn rebuild_declarations(&mut self) {
        let mut declarations = Vec::new();
        if self.outreach.is_some() {
            declarations.push(OutreachAndAwaitQuorumTool::declaration());
        }
        if self.directory.is_some() {
            declarations.push(ListCompanyAgentsTool::declaration());
        }
        if self.channels.is_some() {
            declarations.push(CreateAgentChannelTool::declaration());
        }
        if self.ownership.is_some() {
            declarations.push(TaskOwnershipTool::declaration());
        }
        if self.approval.is_some() {
            declarations.push(RequestApprovalTool::declaration());
        }
        self.declarations = declarations;
    }
}

#[async_trait]
impl HarnessToolHost for NativeToolHost {
    fn available(&self) -> &[NativeToolDeclaration] {
        &self.declarations
    }

    async fn invoke(
        &self,
        id: &ToolId,
        call_id: &str,
        args: Value,
        invocation: Option<super::harness::runs::InvocationRef>,
    ) -> AppResult<ToolInvocation> {
        // An id this host never declared is a harness fault, not a model one: it means something
        // offered the model a tool nobody here can serve. It is an `Err` rather than a failed
        // invocation so it reads as the wiring bug it is instead of as advice to the model.
        let unavailable = || {
            AppError::Internal(format!(
                "Native tool '{id}' is not available in this agent run"
            ))
        };
        match id.as_str() {
            REQUEST_APPROVAL_TOOL_ID => match self.approval.as_ref() {
                Some(tool) => tool.call(call_id, args).await,
                None => Err(unavailable()),
            },
            OUTREACH_TOOL_ID => match self.outreach.as_ref() {
                Some(tool) => tool.call(args, invocation).await,
                None => Err(unavailable()),
            },
            AGENT_DIRECTORY_TOOL_ID => match self.directory.as_ref() {
                Some(tool) => tool.call(args).await,
                None => Err(unavailable()),
            },
            CREATE_AGENT_CHANNEL_TOOL_ID => match self.channels.as_ref() {
                Some(tool) => tool.call(args, invocation).await,
                None => Err(unavailable()),
            },
            TASK_OWNERSHIP_TOOL_ID => match self.ownership.as_ref() {
                Some(tool) => tool.call(call_id, args, invocation).await,
                None => Err(unavailable()),
            },
            _ => Err(unavailable()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_empty_host_offers_nothing_and_refuses_every_call() {
        let host = NativeToolHost::new();

        assert!(host.is_empty());
        assert!(host.available().is_empty());

        let error = host
            .invoke(
                &ToolId::from(OUTREACH_TOOL_ID),
                "call-1",
                serde_json::json!({}),
                None,
            )
            .await
            .expect_err("a tool this run cannot serve is a wiring fault");
        assert!(matches!(error, AppError::Internal(_)), "{error:?}");
    }

    #[tokio::test]
    async fn an_unknown_tool_id_is_refused_rather_than_reported_to_the_model() {
        let host = NativeToolHost::new();

        let error = host
            .invoke(
                &ToolId::from("command"),
                "call-1",
                serde_json::json!({}),
                None,
            )
            .await
            .expect_err("nothing outside the catalogue is dispatchable");
        assert!(error.to_string().contains("command"));
    }

    /// Each declaration must describe the tool it dispatches to, or a model is told about one tool
    /// and `invoke` runs another.
    #[test]
    fn every_declaration_names_a_catalogued_tool_and_carries_an_object_schema() {
        for declaration in [
            OutreachAndAwaitQuorumTool::declaration(),
            ListCompanyAgentsTool::declaration(),
            CreateAgentChannelTool::declaration(),
            TaskOwnershipTool::declaration(),
        ] {
            assert!(
                crate::entities::tool_catalogue::CatalogueTool::get(&declaration.id).is_some(),
                "{} is not in the tool catalogue",
                declaration.id
            );
            assert!(!declaration.name.trim().is_empty());
            assert!(!declaration.description.trim().is_empty());
            assert!(
                declaration.input_schema.is_object(),
                "{} has no JSON Schema object",
                declaration.id
            );
            assert!(declaration.safety.max_output_chars > 0);
            assert!(
                declaration.safety.max_result_chars >= declaration.safety.max_output_chars,
                "{} may store less than it shows",
                declaration.id
            );
        }
    }
}
