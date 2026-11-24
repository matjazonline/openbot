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
        use super::super::agent::{McpErrorPolicy, McpResultSource};
        let policy = McpErrorPolicy::from_inputs(&activation.inputs)?;
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
            if snapshot.kind() == ToolKind::ChannelCall {
                self.channel_call(
                    &snapshot,
                    frame,
                    &path.child(format!("tool-{}:{}", index + 1, call.name.as_str())),
                    reference.agent_id,
                    call,
                )?;
                continue;
            }
            if call.approval.is_some() {
                return Err(refused(
                    "Only a channel.call may script a final-review approval",
                ));
            }
            let source = match tool.binding.source {
                crate::entities::agent_capability::AgentBindingSource::Mcp { .. } => {
                    snapshot.contract.output_schema =
                        super::super::registry::mcp_tool_result(&snapshot.contract.output_schema);
                    McpResultSource::Mcp
                }
                _ => McpResultSource::Other,
            };
            let output = self.tool(
                bundle,
                frame,
                &path.child(format!("tool-{}:{}", index + 1, call.name.as_str())),
                &snapshot,
                call.input.clone(),
                call.output.clone(),
            )?;
            policy.check_result(source, &output)?;
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
            ToolKind::LocalAgent => {
                return Err(refused(
                    "Local agents are workflow steps, not selectable tools",
                ));
            }
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
            ToolKind::ChannelCall => {
                return Err(refused(
                    "channel.call is an agent tool simulated from its frozen targets",
                ));
            }
            ToolKind::Message
            | ToolKind::Escalation
            | ToolKind::Provisioning
            | ToolKind::Knowledge
            | ToolKind::GoalStatus
            | ToolKind::Memory
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
            label: None,
        })?;
        Ok(output)
    }
    /// One scripted `channel.call` (gate AWC-P05-D2, D3): the live launch checks over the
    /// frozen target, then the author's fixture as the call's answer. Nothing is launched; no
    /// destination admission, level release, tree limit or unit spend is simulated.
    fn channel_call(
        &mut self,
        snapshot: &ToolSnapshot,
        frame: &Frame,
        path: &SamplePath,
        agent: crate::entities::agent_capability::AgentId,
        call: &ScriptedToolCall,
    ) -> AppResult<Value> {
        use crate::application::workflow::channel_call::{
            ChannelCallError, ChannelCallOutcome, ChannelCallRequest, DeskFacts, REVIEW_REJECTED,
            TargetDecision, check_intake, decide_target,
        };
        // Live judges the arguments only by `parse`; schema-invalid ones get its typed error.
        let now = chrono::Utc::now();
        let deadline = frame
            .metadata
            .deadline
            .unwrap_or(now + chrono::Duration::hours(24));
        let (output, label) = 'answer: {
            // 1. The arguments, as live parses them.
            let request = match ChannelCallRequest::parse(&call.input, now, deadline) {
                Ok(request) => request,
                Err(error) => break 'answer (error.to_value(), LAUNCH_LABEL),
            };
            // 2. Only a declared, frozen desk.
            let target = self.call_targets.get(&request.channel).ok_or_else(|| {
                refused(
                    "channel_call.sample_target_undeclared: declare this desk among the sample's call targets",
                )
            })?;
            // 3. The frozen launch facts, judged as live judges them. A sample has no ancestry.
            match decide_target(Some(DeskFacts {
                writable: target.writable_agents.contains(&agent),
                in_ancestry: false,
                unit_archived: target.unit_archived,
                enabled: target.enabled,
                responder: target.responder.is_some(),
            })) {
                TargetDecision::Proceed => {}
                TargetDecision::Error(code) => {
                    break 'answer (
                        ChannelCallError::new(code, "This desk is unavailable").to_value(),
                        LAUNCH_LABEL,
                    );
                }
                TargetDecision::Denied(code) => {
                    break 'answer (ChannelCallOutcome::refused(code).to_value()?, LAUNCH_LABEL);
                }
            }
            // 4. The intake.
            if let Err(error) = check_intake(target.intake.as_ref(), request.fields.as_ref())? {
                break 'answer (error.to_value(), LAUNCH_LABEL);
            }
            // 5. The desk's final review.
            match (target.review_required, call.approval) {
                (true, None) => {
                    return Err(refused(
                        "channel_call.sample_review_undeclared: this desk reviews its answers; script approval true or false",
                    ));
                }
                (false, Some(_)) => {
                    return Err(refused(
                        "This desk does not review its answers; remove the scripted approval",
                    ));
                }
                (true, Some(false)) => {
                    break 'answer (
                        ChannelCallError::new(
                            REVIEW_REJECTED,
                            "The desk's reviewer rejected the answer",
                        )
                        .to_value(),
                        REVIEW_LABEL,
                    );
                }
                (true, Some(true)) | (false, None) => {}
            }
            // 6. The author's answer.
            let output = call.output.clone().ok_or_else(|| {
                refused(
                    "channel_call.sample_result_undeclared: script the desk's answer as this call's output",
                )
            })?;
            validate(&snapshot.contract.output_schema, &output)?;
            if !matches!(
                output.get("closure").and_then(Value::as_str),
                Some("done" | "denied" | "handed_off" | "no_reply")
            ) {
                return Err(refused(
                    "A scripted channel.call answer is a done, denied, handed_off or no_reply closure",
                ));
            }
            // Result fields as live passes them: checked against the responder's declared
            // `fields` sub-schema when it has one, otherwise any object (gate finding F9).
            if let (Some(schema), Some(fields)) = (&target.result_fields, output.get("fields")) {
                validate(schema, fields)?;
            }
            (output, FIXTURE_LABEL)
        };
        // An answer the desk never gives is a script error, not an ignored fixture.
        if label != FIXTURE_LABEL && call.output.is_some() {
            return Err(refused(
                "This call is answered without the desk's answer; remove the scripted output",
            ));
        }
        self.effect(SampleEffect {
            path: path.clone(),
            operation: snapshot.contract.name.clone(),
            kind: SampleEffectKind::SimulatedCall,
            input: call.input.clone(),
            output: output.clone(),
            label: Some(label),
        })?;
        Ok(output)
    }
}

const LAUNCH_LABEL: &str = "Simulated call: answered at launch";
const REVIEW_LABEL: &str = "Simulated call: review rejected";
const FIXTURE_LABEL: &str = "Simulated call result (author fixture)";

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
