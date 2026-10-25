use super::*;

/// Authoring help is derived from an actual specialized registration.
/// `example` is executable v1 source syntax, but execution handlers do not exist.
pub struct AuthoringHelp {
    pub type_name: TypeName,
    pub input_schema: Value,
    pub output_schema: Value,
    pub literal_fields: Vec<String>,
    pub routes: RouteContract,
    pub control: ControlKind,
    pub constraints: Value,
    pub notes: &'static str,
    pub example: String,
}
/// Get help for a supplied example in the same context used for compilation.
/// Compiling it here prevents documentation from returning invalid examples.
pub fn authoring_help(
    decoded: DecodedSource,
    version_id: VersionId,
    step: &StepId,
    facts: &CatalogueFacts,
    dependencies: &BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
) -> Result<AuthoringHelp, Diagnostic> {
    let compiled = super::compile(decoded.clone(), version_id, facts, dependencies)?;
    let parsed = compiler::parse_workflow(&decoded, version_id)?;
    if !parsed.definition.steps.contains_key(step) {
        return Err(Diagnostic::at(
            "step.unknown",
            "Select a declared step for authoring help",
            "/steps",
            parsed.locations[""],
        ));
    }
    let registration = Registration::new(&parsed, step, facts).resolve()?;
    Ok(AuthoringHelp {
        type_name: registration.descriptor.type_name,
        input_schema: registration.descriptor.input_schema,
        output_schema: registration.descriptor.output_schema,
        literal_fields: registration.facts["literal_fields"]
            .as_array()
            .expect("registration fields")
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        routes: registration.descriptor.routes,
        control: registration.descriptor.control,
        constraints: registration.facts["constraints"].clone(),
        notes: notes(parsed.definition.steps[step].step_type.as_str()),
        example: compiled.source().to_owned(),
    })
}
fn notes(name: &str) -> &'static str {
    match name {
        "context.history" => {
            "The run's thread history frozen at admission (internal notes excluded), newest messages that fit both max_tokens (one serialized byte per token) and the run's max_context_bytes, returned oldest first. Each item keeps its role, audience and entry kind; omitted counts older messages left out and truncated marks a body cut at 16384 characters. Runs without a thread return no items, and a child run's history is always empty: map history into its input explicitly."
        }
        "memory.load" | "memory.save" => {
            "Not available in this release (memory is out of scope); publication refuses it. Scoped capability required; at most 256 items/facts, each text at most 16384 characters. Saves require logical idempotency."
        }
        "ai.classify" => {
            "The selected agent's model and instructions classify the context into a finite label set; no tools or skills. Invalid labels get bounded repair, then fail terminally."
        }
        "agent.run" => {
            "Runs the selected agent with exactly its saved tools and skills, frozen at publication; current revocation still applies. Context is untrusted data; output follows output_schema with bounded repair."
        }
        "decision.rule" => {
            "Ordered pure rule with required default; all choices must be explicit routes. Data follows data_schema."
        }
        "decision.human" => {
            "Durable review deadline required; never automatically accepts. Reviewers are an explicit static list of company users; first valid authorized submission wins. Timeout fails Terminal decision.timed_out through the declared error route; optional feedback requirements name declared choices."
        }
        "decision.agent" => {
            "The selected agent's model and instructions choose one declared choice, with no tools or skills; data follows data_schema. The model cannot name another step or add a route."
        }
        "data.map" => {
            "Pure typed construction, no scripts; output is value itself and follows output_schema."
        }
        "http.request" => {
            "Declared http slot, relative path (2048 characters), at most 64 headers. HTTP verbs never establish replay safety. Shared action receipts/reconciliation required."
        }
        "tool.call" | "mcp.call" => {
            "Literal selected tool needs supplied argument/result facts; MCP requires a declared mcp slot. Shared action receipts/reconciliation required; isError is mandatory, never fabricated."
        }
        "message.send" | "message.reply" => {
            "Canonical content up to 16384 characters, at most 20 scoped destinations. Accepted means provider acceptance/local commit, never read receipt. Replies require an explicit source message."
        }
        "workflow.call" => {
            "Runs the pinned child version with explicit with.input and static with.params (literals or /params). The child gets only those, the parent's same-named resources, its deadline and root budget; never its history. Output is the child's output; child failure or cancellation takes the error route."
        }
        "flow.repeat" => {
            "Runs the pinned child in sequential rounds, 1..1000 (max_iterations). After each round repeat.exit_when decides with /steps/<self>/output = {result, rounds}; otherwise repeat.next_input is the next round's input. Routes: done (exit true, also on the last round) and exhausted (bound reached, never approval). One repetition is charged per round."
        }
        "wait.event" | "wait.timer" => {
            "RFC3339 deadline required; future-relative validation needs runtime clock. Durable suspension, no sleeping worker. Events are run-scoped by literal name and correlation, consumed once and atomically; repeated waits must carry the iteration in the correlation."
        }
        _ => unreachable!("registered help"),
    }
}
