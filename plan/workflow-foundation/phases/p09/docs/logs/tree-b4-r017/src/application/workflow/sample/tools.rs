use super::*;
use crate::application::workflow::compiler::{Schema, SourceSpan};
use crate::application::workflow::publication::{AgentKey, ToolKind, ToolSnapshot};

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
        calls: Vec<ScriptedToolCall>,
    ) -> AppResult<()> {
        let key = activation
            .inputs
            .get("agent")
            .and_then(Value::as_str)
            .and_then(AgentKey::parse)
            .ok_or_else(|| refused("Invalid frozen agent selection"))?;
        let agent = bundle.resolve_agent(bundle.company_id(), &key)?;
        for (index, call) in calls.into_iter().enumerate() {
            if self.report.usage.direct_tool_calls >= MAX_TOOL_CALLS {
                return Err(refused("Sample exceeds 64 direct tool calls"));
            }
            self.report.usage.direct_tool_calls += 1;
            let tool = agent
                .tools()
                .iter()
                .find(|tool| tool.contract.name == call.name)
                .ok_or_else(|| refused("Scripted tool is not granted to this frozen agent"))?;
            self.tool(
                bundle,
                frame,
                &path.child(format!("tool-{}:{}", index + 1, call.name.as_str())),
                tool,
                call.input,
                call.output,
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
                let child = bundle
                    .children()
                    .get(&pin.version_id)
                    .ok_or_else(|| refused("Workflow tool child is not frozen"))?
                    .clone();
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
            ToolKind::Message | ToolKind::Provisioning | ToolKind::Slot(_) | ToolKind::Http => {
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
