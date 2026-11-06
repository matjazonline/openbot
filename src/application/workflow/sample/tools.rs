use super::*;
use crate::application::agent::AgentKey;
use crate::application::workflow::compiler::{Schema, SourceSpan};
use crate::application::workflow::publication::{ToolKind, ToolSnapshot};

impl Session {
    pub(super) fn step_tool(
        &self,
        bundle: &PublishedBundle,
        inputs: &Value,
    ) -> Option<ToolSnapshot> {
        let name = inputs.get("tool")?.as_str()?;
        let slot = inputs.get("connection").and_then(Value::as_str);
        bundle
            .snapshots()
            .tools
            .iter()
            .find(|tool| {
                tool.contract.name.as_str() == name
                    && tool.contract.connection.as_ref().map(|slot| slot.as_str()) == slot
            })
            .cloned()
    }
    pub(super) fn agent_tools(
        &mut self,
        bundle: &Arc<PublishedBundle>,
        frame: &Frame,
        activation: &ActivatedExecution,
        path: &SamplePath,
        response: &ScriptedResponse,
    ) -> AppResult<()> {
        let key = activation
            .inputs
            .get("agent")
            .and_then(Value::as_str)
            .and_then(AgentKey::parse)
            .ok_or_else(|| refused("Invalid frozen agent selection"))?;
        let reference = bundle.agent_reference(bundle.company_id(), &key)?;
        let agent = self
            .agents
            .get(&reference.agent_id)
            .ok_or_else(|| refused("Sample agent definition is missing"))?
            .clone();
        for (index, call) in response.tools.iter().enumerate() {
            if self.report.usage.direct_tool_calls >= MAX_TOOL_CALLS {
                return Err(refused("Sample exceeds 64 direct tool calls"));
            }
            self.report.usage.direct_tool_calls += 1;
            let tool = agent
                .tools
                .iter()
                .find(|tool| tool.binding.alias.as_str() == call.name.as_str())
                .ok_or_else(|| refused("Scripted tool is not granted to this frozen agent"))?;
            if tool.approval_required {
                super::dispatch::require_approval(response.approval)?;
            }
            let mut snapshot = crate::application::workflow::agent_contract::agent_action_snapshot(
                bundle.company_id(),
                tool,
            )?;
            if matches!(
                tool.binding.source,
                crate::entities::agent_capability::AgentBindingSource::Mcp { .. }
            ) {
                snapshot.contract.output_schema =
                    super::super::registry::mcp_tool_result(&snapshot.contract.output_schema);
            }
            self.tool(
                bundle,
                frame,
                &path.child(format!("tool-{}:{}", index + 1, call.name.as_str())),
                &snapshot,
                call.input.clone(),
                call.output.clone(),
            )?;
        }
        Ok(())
    }
    pub(super) fn tool(
        &mut self,
        bundle: &Arc<PublishedBundle>,
        frame: &Frame,
        path: &SamplePath,
        tool: &ToolSnapshot,
        input: Value,
        fixture: Option<Value>,
    ) -> AppResult<Value> {
        validate(&tool.contract.input_schema, &input)?;
        let (output, kind) = match tool.kind() {
            ToolKind::Workflow(pin) => {
                if fixture.is_some() {
                    return Err(refused(
                        "Workflow tool output comes from its sample child, not a fixture",
                    ));
                }
                let child = definitions::workflow_from(
                    &self.workflows,
                    bundle.company_id(),
                    pin.workflow_id,
                    pin.version_id,
                )?;
                let output = self.run(
                    child,
                    frame.child(
                        path.child("workflow-tool"),
                        input.clone(),
                        pin.params.clone(),
                    ),
                )?;
                (output, SampleEffectKind::WorkflowChild)
            }
            ToolKind::Message
            | ToolKind::Provisioning
            | ToolKind::Knowledge
            | ToolKind::Slot(_)
            | ToolKind::Http => {
                let output = fixture.ok_or_else(|| {
                    refused("External tool needs a fixture output; no production fallback exists")
                })?;
                (output, super::dispatch::effect_kind(tool.policy.effect))
            }
        };
        let schema = match tool.kind() {
            ToolKind::Slot(_) => {
                super::super::registry::mcp_tool_result(&tool.contract.output_schema)
            }
            _ => tool.contract.output_schema.clone(),
        };
        validate(&schema, &output)?;
        self.effect(SampleEffect {
            path: path.clone(),
            operation: tool.contract.name.clone(),
            kind,
            input,
            output: output.clone(),
        })?;
        Ok(output)
    }
}
fn validate(raw: &Value, value: &Value) -> AppResult<()> {
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
    Schema::compile(raw.clone(), "/sample/tool", span)
        .and_then(|schema| schema.validate(value, "/sample/tool", span))
        .map_err(diagnostic)
}
