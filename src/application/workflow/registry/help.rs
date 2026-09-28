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
        "context.load" => {
            "Execution unavailable. Context cutoff comes from frozen run facts. At most 128 sources, 256 items and 131072 tokens; provenance is mandatory."
        }
        "memory.load" | "memory.save" => {
            "Execution unavailable. Scoped capability required; at most 256 items/facts, each text at most 16384 characters. Saves require logical idempotency."
        }
        "ai.classify" => {
            "Execution unavailable. Finite labels/profile selection only, no tools. Model configuration comes from frozen facts."
        }
        "agent.run" => {
            "Execution unavailable. Omitted profile retains agent_defaults; explicit empty tools/skills selects none. Authorization and saved-agent loading remain future work."
        }
        "decision.rule" => {
            "Execution unavailable. Ordered pure rule with required default; all choices must be explicit routes. Data follows data_schema."
        }
        "decision.human" => {
            "Execution unavailable. Durable review deadline required; never automatically accepts. Reviewer is a user/group selector; optional feedback requirements name declared choices."
        }
        "decision.agent" => {
            "Execution unavailable. Model can select only declared eligible choices; data follows data_schema."
        }
        "data.map" => {
            "Execution unavailable. Pure typed construction, no scripts; output is value itself and follows output_schema."
        }
        "http.request" => {
            "Execution unavailable. Declared http slot, relative path (2048 characters), at most 64 headers. HTTP verbs never establish replay safety. Shared action receipts/reconciliation required."
        }
        "tool.call" | "mcp.call" => {
            "Execution unavailable. Literal selected tool needs supplied argument/result facts; MCP requires a declared mcp slot. Shared action receipts/reconciliation required; isError is mandatory, never fabricated."
        }
        "message.send" | "message.reply" => {
            "Execution unavailable. Canonical content up to 16384 characters, at most 128 scoped destinations. Accepted means provider acceptance/local commit, never read receipt. Replies require an explicit source message."
        }
        "workflow.call" => {
            "Execution unavailable. Pinned child contracts, inherited budgets/authorization requirements and durable continuation identity are mandatory."
        }
        "flow.repeat" => {
            "Execution unavailable. Sequential pinned child, inherited budgets; 1..10000 rounds. Next-round mapping, exit predicate and exhaustion control belong to phase07."
        }
        "wait.event" | "wait.timer" => {
            "Execution unavailable. RFC3339 deadline required; future-relative validation needs runtime clock. Durable suspension, no sleeping worker. Event consumption must be scoped and atomic."
        }
        _ => unreachable!("registered help"),
    }
}
