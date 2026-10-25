use super::*;
use crate::application::workflow::{
    WorkflowActor,
    batch::validate_completion,
    completion::prepare_io,
    decisions::{
        DecisionCommandKey, DecisionId, SubmitDecision, hex_digest, prepare_submission,
        reviewer_deadline,
    },
    registry::ModelStep,
};
use crate::domain::workflow::RouteSelection;

impl Session {
    pub(super) fn step(
        &mut self,
        bundle: &Arc<PublishedBundle>,
        frame: &Frame,
        activation: &ActivatedExecution,
        path: &SamplePath,
    ) -> AppResult<PureCompletion> {
        if let Some(done) = execute_pure(bundle, activation)? {
            return Ok(done);
        }
        let kind = bundle.compiled().graph().definition().steps[&activation.step]
            .step_type
            .as_str();
        if matches!(kind, "workflow.call" | "flow.repeat") {
            return self.child_step(bundle, frame, activation, path);
        }
        if kind == "tool.call"
            && let Some(tool) = self.step_tool(bundle, &activation.inputs)
            && matches!(
                tool.kind(),
                super::super::publication::ToolKind::Workflow(_)
            )
        {
            return self.workflow_tool_step(bundle, frame, activation, path, &tool);
        }
        let response = self.response(bundle, &activation.step)?;
        if let Some(model) = ModelStep::of(kind) {
            return self.model(bundle, frame, activation, path, model, response);
        }
        if !response.tools.is_empty() || response.tokens != 0 {
            return Err(refused(
                "Only model scripts may declare tool calls or token usage",
            ));
        }
        match kind {
            "decision.human" => human(bundle, activation, &response.output),
            "wait.event" | "wait.timer" => {
                validate_completion(bundle, activation, response.output, RouteSelection::Success)
            }
            "context.history" | "memory.load" | "memory.save" | "message.send"
            | "message.reply" | "http.request" | "mcp.call" | "tool.call" => {
                self.external_step(bundle, activation, path, kind, response)
            }
            _ => Err(refused(format!(
                "Unsupported sample step {kind}; no production fallback exists"
            ))),
        }
    }

    fn external_step(
        &mut self,
        bundle: &PublishedBundle,
        activation: &ActivatedExecution,
        path: &SamplePath,
        kind: &str,
        response: ScriptedResponse,
    ) -> AppResult<PureCompletion> {
        approval(&activation.inputs, response.approval)?;
        let output = response.output;
        if kind == "mcp.call" && super::super::mcp::mcp_result_is_error(&output) {
            return Err(refused("Scripted MCP tool error; sample stopped"));
        }
        if matches!(kind, "message.send" | "message.reply")
            && (output.get("status").and_then(Value::as_str) != Some("accepted")
                || output
                    .get("messages")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("status").and_then(Value::as_str) != Some("accepted")
                        })
                    }))
        {
            return Err(refused("Scripted message delivery failed; sample stopped"));
        }
        let completion = prepare_io(bundle, activation, output.clone())?;
        let effect_kind = match kind {
            "context.history" | "memory.load" => SampleEffectKind::FixtureRead,
            "mcp.call" | "tool.call" => self
                .step_tool(bundle, &activation.inputs)
                .map(|tool| effect_kind(tool.policy.effect))
                .ok_or_else(|| refused("Tool is absent from the frozen sample bundle"))?,
            _ => SampleEffectKind::InterceptedWrite,
        };
        self.effect(SampleEffect {
            path: path.clone(),
            operation: bundle.compiled().graph().definition().steps[&activation.step]
                .step_type
                .clone(),
            kind: effect_kind,
            input: activation.inputs.clone(),
            output,
        })?;
        Ok(completion)
    }

    fn workflow_tool_step(
        &mut self,
        bundle: &Arc<PublishedBundle>,
        frame: &Frame,
        activation: &ActivatedExecution,
        path: &SamplePath,
        tool: &super::super::publication::ToolSnapshot,
    ) -> AppResult<PureCompletion> {
        if activation.inputs.get("approval").is_some() {
            let response = self.response(bundle, &activation.step)?;
            approval(&activation.inputs, response.approval)?;
            if !response.output.is_null() || !response.tools.is_empty() || response.tokens != 0 {
                return Err(refused(
                    "A workflow tool approval response must have null output and no tool calls or tokens",
                ));
            }
        }
        let output = self.tool(
            bundle,
            frame,
            path,
            tool,
            activation
                .inputs
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Null),
            None,
        )?;
        prepare_io(bundle, activation, output)
    }

    fn model(
        &mut self,
        bundle: &Arc<PublishedBundle>,
        frame: &Frame,
        activation: &ActivatedExecution,
        path: &SamplePath,
        model: ModelStep,
        response: ScriptedResponse,
    ) -> AppResult<PureCompletion> {
        self.charge(BudgetResource::ModelCall)?;
        let tokens = self
            .report
            .usage
            .scripted_tokens
            .checked_add(response.tokens)
            .ok_or_else(|| refused("Sample token usage overflow"))?;
        if tokens
            > self
                .budget
                .tokens()
                .map_or(1_000_000, |allowance| allowance.max_tokens().min(1_000_000))
        {
            return Err(refused("Scripted token allowance exhausted"));
        }
        self.report.usage.scripted_tokens = tokens;
        if !response.tools.is_empty() {
            if !model.uses_capabilities() {
                return Err(refused("This model step is tool-free"));
            }
            approval(&activation.inputs, response.approval)?;
            self.agent_tools(bundle, frame, activation, path, response.tools)?;
            self.charge(BudgetResource::ModelCall)?;
        }
        prepare_io(bundle, activation, response.output)
    }
}

fn human(
    bundle: &PublishedBundle,
    activation: &ActivatedExecution,
    output: &Value,
) -> AppResult<PureCompletion> {
    if !output.as_object().is_some_and(|object| {
        object.len() == 3
            && ["choice", "data", "feedback"]
                .iter()
                .all(|key| object.contains_key(*key))
    }) {
        return Err(refused(
            "Human responses contain exactly choice, data and feedback",
        ));
    }
    let (reviewers, deadline) = reviewer_deadline(&activation.inputs)?;
    if deadline <= chrono::Utc::now() {
        return Err(refused("The scripted human decision deadline has passed"));
    }
    let command = SubmitDecision {
        company: bundle.company_id(),
        actor: WorkflowActor::authenticated(reviewers[0])?,
        decision: DecisionId(Uuid::new_v4()),
        command_key: DecisionCommandKey::parse("sample")?,
        expected_revision: 1,
        expected_artifact: hex_digest(activation.inputs.get("proposal").unwrap_or(&Value::Null))?,
        choice: output
            .get("choice")
            .and_then(Value::as_str)
            .ok_or_else(|| refused("Scripted human decision needs a choice"))?
            .into(),
        feedback: output
            .get("feedback")
            .and_then(Value::as_str)
            .ok_or_else(|| refused("Scripted human decision needs feedback"))?
            .into(),
        data: output
            .get("data")
            .cloned()
            .ok_or_else(|| refused("Scripted human decision needs data"))?,
    };
    prepare_submission(bundle, activation, &command)
}
fn approval(inputs: &Value, approved: Option<bool>) -> AppResult<()> {
    if inputs.get("approval").is_some() && approved != Some(true) {
        return Err(refused(
            "This protected request needs an explicit approval:true in its sample response",
        ));
    }
    Ok(())
}
pub(super) fn effect_kind(effect: super::super::publication::ActionEffect) -> SampleEffectKind {
    match effect {
        super::super::publication::ActionEffect::Read => SampleEffectKind::FixtureRead,
        super::super::publication::ActionEffect::Write => SampleEffectKind::InterceptedWrite,
    }
}
