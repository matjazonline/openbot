//! Projection into the existing action protocol; agent aliases never choose a remote operation.
use crate::app_error::{AppError, AppResult};
use crate::application::agent::{AgentToolOrigin, ResolvedAgentTool};
use crate::application::workflow::publication::{ToolSnapshot, WorkflowToolPin};
use crate::application::workflow::registry::ToolContract;
use crate::domain::workflow::{CompanyId, TypeName};
use crate::entities::agent_capability::{
    AgentBindingSource, AgentId, AgentRevisionId, DependencyManifestId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSelectionIdentity {
    pub manifest: DependencyManifestId,
    pub agent: AgentId,
    pub revision: AgentRevisionId,
    pub digest: [u8; 32],
    pub mode: crate::application::workflow::publication::AgentUseMode,
}

impl AgentSelectionIdentity {
    pub fn from_selection(
        selection: &crate::application::workflow::publication::ResolvedAgent,
    ) -> Self {
        Self {
            manifest: selection.manifest_id(),
            agent: selection.agent().agent_id,
            revision: selection.revision().id,
            digest: selection.revision().digest,
            mode: selection.mode(),
        }
    }
    pub fn tool_origin(
        &self,
        binding: crate::entities::agent_capability::AgentBindingId,
    ) -> AgentToolOrigin {
        AgentToolOrigin {
            manifest: self.manifest,
            agent: self.agent,
            revision: self.revision,
            binding,
        }
    }
}

/// The native effect target is derived from the binding, never from its advertised alias.
pub fn agent_action_target(
    company: CompanyId,
    tool: &ResolvedAgentTool,
) -> AppResult<super::actions::ActionTarget> {
    use super::actions::ActionTarget;
    let kind = |name| {
        TypeName::parse(name).map_err(|_| AppError::Internal("Invalid agent target kind".into()))
    };
    Ok(match &tool.binding.source {
        AgentBindingSource::Http { connection, .. } => ActionTarget::Local {
            resource: *connection,
            resource_kind: kind("http")?,
        },
        AgentBindingSource::Mcp { connection, .. } => ActionTarget::Connection {
            resource: *connection,
            resource_kind: kind("mcp")?,
        },
        AgentBindingSource::Builtin { tool } => ActionTarget::Local {
            resource: company.as_uuid(),
            resource_kind: kind(match tool {
                crate::entities::agent_capability::AgentBuiltinTool::MessageSend
                | crate::entities::agent_capability::AgentBuiltinTool::MessageReply => "messaging",
                crate::entities::agent_capability::AgentBuiltinTool::ListCompanyAgents
                | crate::entities::agent_capability::AgentBuiltinTool::CreateAgentChannel => {
                    "provisioning"
                }
                crate::entities::agent_capability::AgentBuiltinTool::SearchKnowledge => "knowledge",
                crate::entities::agent_capability::AgentBuiltinTool::ReadEscalation
                | crate::entities::agent_capability::AgentBuiltinTool::RespondToEscalation => {
                    "escalation"
                }
                crate::entities::agent_capability::AgentBuiltinTool::SearchMemory
                | crate::entities::agent_capability::AgentBuiltinTool::MemoryTimeline
                | crate::entities::agent_capability::AgentBuiltinTool::MemoryDetails
                | crate::entities::agent_capability::AgentBuiltinTool::SaveMemory => "memory",
            })?,
        },
        AgentBindingSource::Workflow { .. } => {
            return Err(AppError::BadRequest(
                "Workflow tools use the child runtime".into(),
            ));
        }
    })
}

/// Closed provenance check shared by preparation, reservation, recovery and reconciliation.
pub fn validate_agent_action(
    request: &super::actions::ActionRequest,
    selection: &super::publication::ResolvedAgent,
) -> AppResult<()> {
    let refused = || AppError::NotFound("Agent tool origin".into());
    let crate::application::agent::ToolCallOrigin::Agent(origin) = request.context.origin else {
        return Err(refused());
    };
    if selection.mode() != super::publication::AgentUseMode::Tools
        || selection.company_id() != request.scope.company
        || selection.manifest_id() != origin.manifest
        || selection.revision().id != origin.revision
        || selection.agent().agent_id != origin.agent
    {
        return Err(refused());
    }
    let tool = selection
        .tools()
        .iter()
        .find(|tool| tool.binding.id == origin.binding)
        .ok_or_else(refused)?;
    let contract = agent_action_snapshot(selection.company_id(), tool)?;
    let target = agent_action_target(selection.company_id(), tool)?;
    if serde_json::to_value(&contract).map_err(|_| refused())?
        != serde_json::to_value(&request.contract).map_err(|_| refused())?
        || serde_json::to_value(&target).map_err(|_| refused())?
            != serde_json::to_value(&request.target).map_err(|_| refused())?
        || request.operation_key != contract.contract.name
        || (tool.approval_required && !request.context.approval_required)
        || !request
            .context
            .capability_ceiling
            .contains(&tool.policy.capability)
        || request.context.capability_ceiling.iter().any(|capability| {
            !selection
                .tools()
                .iter()
                .any(|tool| &tool.policy.capability == capability)
        })
    {
        return Err(refused());
    }
    Ok(())
}

pub fn agent_action_snapshot(
    company: CompanyId,
    tool: &ResolvedAgentTool,
) -> AppResult<ToolSnapshot> {
    let (name, workflow) = match &tool.binding.source {
        AgentBindingSource::Builtin { tool } => (TypeName::parse(tool.name()), None),
        AgentBindingSource::Http { operation, .. } => (Ok(operation.clone()), None),
        AgentBindingSource::Mcp { remote_name, .. } => {
            (TypeName::parse(remote_name.as_str()), None)
        }
        AgentBindingSource::Workflow {
            registration,
            workflow,
            version,
            params,
        } => (
            Ok(registration.clone()),
            Some(WorkflowToolPin {
                workflow_id: *workflow,
                version_id: *version,
                params: params.clone(),
            }),
        ),
    };
    Ok(ToolSnapshot {
        company_id: company,
        contract: ToolContract {
            name: name
                .map_err(|_| AppError::Internal("Invalid frozen agent operation name".into()))?,
            connection: None,
            input_schema: tool.input_schema.clone(),
            output_schema: tool.output_schema.clone(),
        },
        policy: tool.policy.clone(),
        workflow,
    })
}
