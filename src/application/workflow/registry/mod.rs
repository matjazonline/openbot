//! Pure, bounded compiler catalogue. Requirements are not authorization grants;
//! execution handlers live outside the catalogue.
mod actions;
mod budget;
mod checks;
mod examples;
mod facts;
mod families;
mod help;
pub mod jev;
pub(crate) mod mcp;
pub(crate) use mcp::tool_result as mcp_tool_result;
mod metadata;
mod schemas;
pub use examples::{AuthoringExample, example};

use crate::application::workflow::compiler::{
    self, CompiledWorkflow, ControlKind, DecodedSource, Diagnostic, ParsedWorkflow, RouteContract,
    Schema, SourceSpan, SpecializedDescriptor, StepDescriptor,
};
use crate::domain::workflow::{
    Binding, ResourceName, Routes, StepId, TypeName, VersionId, WorkflowId,
};
pub(crate) use actions::message_tool_schemas;
pub(crate) use checks::InputCheck;
pub use facts::{CatalogueFacts, ChildContract, ToolContract};
pub use help::{AuthoringHelp, authoring_help};
pub use metadata::{CapabilityRequirement, EffectClass, ExecutionConstraints, RecoveryMode};
use schemas::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_REVISION: &str = "workflow-catalogue-v1.6";
pub const MAX_FACTS: usize = 256;
pub const MAX_SELECTIONS: usize = 128;
/// Explicit reviewers of one `decision.human` (user decision Q1, 05-D2 §2): at most this many
/// reviewer rows and notice deliveries are written by one park transaction.
pub const MAX_REVIEWERS: usize = crate::domain::workflow::TokenAllowance::MAX_REVIEWERS;
/// Destinations of one `message.send`: the established per-commit fan-out bound
/// (`transport::ingress::MAX_THREAD_ASSOCIATIONS`). One send writes at most this many threads,
/// messages and deliveries in one transaction, and a receipt locks at most this many parents.
pub const MAX_MESSAGE_DESTINATIONS: usize = 20;
pub const MAX_ITEMS: usize = 256;
pub const MAX_TEXT: usize = 16_384;
pub const MAX_HEADERS: usize = 64;
pub const MAX_CONTEXT_TOKENS: usize = 131_072;
pub const MAX_IDENTIFIER: usize = 128;
pub const MAX_PATH: usize = 2048;
pub const MAX_DEADLINE: usize = 64;
pub const MAX_CORRELATION: usize = 256;
pub const TYPES: [&str; 19] = [
    "context.history",
    "memory.load",
    "memory.save",
    "ai.classify",
    "jev.evaluate",
    "agent.run",
    "decision.rule",
    "decision.human",
    "decision.agent",
    "data.map",
    "http.request",
    "tool.call",
    "mcp.call",
    "message.send",
    "message.reply",
    "workflow.call",
    "flow.repeat",
    "wait.event",
    "wait.timer",
];

/// The step kinds that run a selected agent's model (06-D5, 06-D6): the one list that
/// publication, preparation and the runtime share. Only `AgentRun` uses tools and skills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStep {
    AgentRun,
    Classify,
    Decide,
}
impl ModelStep {
    pub fn of(kind: &str) -> Option<Self> {
        match kind {
            "agent.run" => Some(Self::AgentRun),
            "ai.classify" => Some(Self::Classify),
            "decision.agent" => Some(Self::Decide),
            _ => None,
        }
    }
    pub fn kind(self) -> &'static str {
        match self {
            Self::AgentRun => "agent.run",
            Self::Classify => "ai.classify",
            Self::Decide => "decision.agent",
        }
    }
    pub fn uses_capabilities(self) -> bool {
        self == Self::AgentRun
    }
}

/// Compile with the built-in type catalogue and explicit, frozen external facts.
/// The dependency graph remains caller-owned; no facts are discovered or fetched.
pub fn compile(
    decoded: DecodedSource,
    version_id: VersionId,
    facts: &CatalogueFacts,
    dependencies: &BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
) -> Result<CompiledWorkflow, Diagnostic> {
    let parsed = compiler::parse_workflow(&decoded, version_id)?;
    budget::preflight(&parsed, facts)?;
    facts.validate(parsed.locations[""])?;
    let mut descriptors = BTreeMap::new();
    let mut budget = compiler::FactBudget::new(parsed.locations[""]);
    budget.punctuation(5)?;
    for (index, id) in parsed.definition.steps.keys().enumerate() {
        let descriptor = Registration::new(&parsed, id, facts).resolve()?;
        budget.punctuation(usize::from(index > 0))?;
        compiler::charge_descriptor(&mut budget, id, &descriptor.descriptor, &descriptor.facts)?;
        descriptors.insert(id.clone(), descriptor);
    }
    compiler::check_resolved_budget(&parsed, &descriptors, dependencies, &facts.tool_workflows)?;
    validate_declarations(&parsed)?;
    validate_jev_resources(&parsed)?;
    compiler::compile_resolved(
        decoded,
        parsed,
        &descriptors,
        dependencies,
        &facts.tool_workflows,
    )
}

struct Registration<'a> {
    parsed: &'a ParsedWorkflow,
    id: &'a StepId,
    facts: &'a CatalogueFacts,
    checks: Vec<InputCheck>,
    effective: Value,
    static_fields: Vec<&'static str>,
}
impl<'a> Registration<'a> {
    fn new(parsed: &'a ParsedWorkflow, id: &'a StepId, facts: &'a CatalogueFacts) -> Self {
        Self {
            parsed,
            id,
            facts,
            checks: Vec::new(),
            effective: json!({}),
            static_fields: Vec::new(),
        }
    }
    fn name(&self) -> &str {
        self.parsed.definition.steps[self.id].step_type.as_str()
    }
    fn path(&self, field: &str) -> String {
        format!(
            "/steps/{}/with/{}",
            self.id,
            field.replace('~', "~0").replace('/', "~1")
        )
    }
    fn error(&self, code: &'static str, message: &str, field: &str) -> Diagnostic {
        let path = self.path(field);
        let span = self
            .parsed
            .locations
            .get(&path)
            .copied()
            .unwrap_or(self.parsed.locations[""]);
        Diagnostic::at(code, message, &path, span)
    }
    fn literal(&mut self, field: &'static str) -> Result<Value, Diagnostic> {
        self.static_fields.push(field);
        match self.parsed.definition.steps[self.id].inputs.get(field) {
            Some(Binding::Literal(value)) => Ok(value.clone()),
            _ => Err(self.error(
                "registry.literal",
                "This declaration requires a literal binding",
                field,
            )),
        }
    }
    /// Reviewer assignment is deterministic: only literals and admin-configured binding
    /// parameters may supply it, never run input, step output or run context (05-D2 §4.2).
    /// A child's parameters follow the same rule, since they are its reviewers' source.
    fn static_roots(&self, field: &'static str, refusal: StaticRefusal) -> Result<(), Diagnostic> {
        let Some(binding) = self.parsed.definition.steps[self.id].inputs.get(field) else {
            return Ok(());
        };
        if binding
            .references()
            .iter()
            .all(|reference| reference.root_name() == "params")
        {
            Ok(())
        } else {
            Err(self.error(refusal.code, refusal.message, field))
        }
    }
    /// A run-scoped event is identified by its literal name; every `wait.event` declaring that
    /// name shares one payload schema, so ingress can validate before any wait exists (05-D1 §11).
    fn event_identity(&mut self, schema: &Value) -> Result<(), Diagnostic> {
        let name = self.literal("event")?;
        let conflicting = self.parsed.definition.steps.iter().any(|(id, step)| {
            id != self.id
                && step.step_type.as_str() == "wait.event"
                && matches!(step.inputs.get("event"), Some(Binding::Literal(other)) if *other == name)
                && !matches!(step.inputs.get("payload_schema"), Some(Binding::Literal(other)) if other == schema)
        });
        if conflicting {
            return Err(self.error(
                "registry.event_schema",
                "Every wait for one event name must declare the same payload_schema",
                "payload_schema",
            ));
        }
        Ok(())
    }
    /// A `message.reply` wait receives a reply delivered by ingress (p08 D1.5): its payload is
    /// the registry's `MessageTriggerInput`, and its correlation must name the outbound message
    /// of a message step in this graph, the only correlation a reply can ever carry.
    fn message_reply_wait(&mut self, schema: &Value) -> Result<(), Diagnostic> {
        use crate::application::workflow::triggers::{MESSAGE_REPLY_EVENT, MessageTriggerInput};
        // `event_identity` already required (and recorded) the literal.
        let reply = Value::String(MESSAGE_REPLY_EVENT.into());
        if !matches!(self.parsed.definition.steps[self.id].inputs.get("event"),
            Some(Binding::Literal(event)) if *event == reply)
        {
            return Ok(());
        }
        if *schema != MessageTriggerInput::schema() {
            return Err(self.error(
                "registry.message_reply_schema",
                "A message.reply wait must declare the standard message payload_schema \
                 (source_message, message, subject, sender)",
                "payload_schema",
            ));
        }
        let steps = &self.parsed.definition.steps;
        let awaited = match steps[self.id].inputs.get("correlation") {
            Some(Binding::Reference(reference)) => reference.step_id().filter(|step| {
                matches!(reference.tail(), [messages, index, field]
                    if messages == "messages" && field == "message_id"
                        && !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()))
                    && steps.get(*step).is_some_and(|sent| {
                        matches!(sent.step_type.as_str(), "message.send" | "message.reply")
                    })
            }),
            _ => None,
        };
        if awaited.is_none() {
            return Err(self.error(
                "registry.message_reply_correlation",
                "A message.reply wait must correlate on /steps/<message step>/output/messages/<n>/message_id",
                "correlation",
            ));
        }
        Ok(())
    }
    fn declared_schema(&mut self, field: &'static str) -> Result<Value, Diagnostic> {
        let value = self.literal(field)?;
        Ok(value)
    }
    fn choices(&self) -> Result<Vec<String>, Diagnostic> {
        let Routes::Choices(choices) = &self.parsed.definition.steps[self.id].routes else {
            return Err(self.error(
                "registry.choices",
                "Declare named routes.choices for this decision",
                "",
            ));
        };
        Ok(choices.iter().map(|(name, _)| name.to_string()).collect())
    }
    fn check(&mut self, check: InputCheck) -> Result<(), Diagnostic> {
        let inputs: serde_json::Map<_, _> = self.parsed.definition.steps[self.id]
            .inputs
            .iter()
            .filter_map(|(name, binding)| {
                if let Binding::Literal(value) = binding {
                    Some((name.clone(), value.clone()))
                } else {
                    None
                }
            })
            .collect();
        check
            .validate(
                &Value::Object(inputs),
                &format!("/steps/{}/with", self.id),
                self.parsed.locations[""],
            )
            .map_err(|mut error| {
                error.span = compiler::input_span(&self.parsed.locations, &error.field_path);
                error
            })?;
        self.checks.push(check);
        Ok(())
    }
    fn resolve(mut self) -> Result<SpecializedDescriptor, Diagnostic> {
        if !TYPES.contains(&self.name()) {
            return Err(Diagnostic::at(
                "step.type",
                "Unknown registered step type",
                &format!("/steps/{}/type", self.id),
                self.parsed
                    .locations
                    .get(&format!("/steps/{}/type", self.id))
                    .copied()
                    .unwrap_or(self.parsed.locations[""]),
            ));
        }
        let contract = match self.name() {
            "http.request" | "tool.call" | "mcp.call" | "message.send" | "message.reply" => {
                self.action()?
            }
            "workflow.call" | "flow.repeat" => self.child()?,
            _ => self.basic()?,
        };
        let name = self.name();
        let choices = if name.starts_with("decision.") {
            Some(self.choices()?)
        } else {
            None
        };
        let choices = if name == "flow.repeat" {
            Some(vec!["done".to_owned(), "exhausted".to_owned()])
        } else {
            choices
        };
        let routes = choices.map_or(RouteContract::Success, |names| {
            RouteContract::Choices(
                names
                    .iter()
                    .map(|n| crate::domain::workflow::ChoiceName::parse(n).expect("parsed choice"))
                    .collect(),
            )
        });
        let descriptor = StepDescriptor {
            type_name: TypeName::parse(name).expect("registered name"),
            input_schema: contract.input,
            output_schema: contract.output,
            routes,
            control: match name {
                "workflow.call" => ControlKind::ChildCall,
                "flow.repeat" => ControlKind::Repeat,
                _ => ControlKind::None,
            },
            ordered_rule: name == "decision.rule",
        };
        let metadata = metadata::constraints(name);
        Ok(SpecializedDescriptor {
            descriptor,
            facts: json!({"revision":CONTRACT_REVISION,"constraints":metadata,"effective":self.effective,"literal_fields":self.static_fields,"checks":self.checks}),
            checks: self.checks,
        })
    }
}
/// The diagnostic for a field that must be literal or come from binding parameters.
#[derive(Clone, Copy)]
pub(super) struct StaticRefusal {
    code: &'static str,
    message: &'static str,
}
pub(super) const STATIC_REVIEWERS: StaticRefusal = StaticRefusal {
    code: "registry.static_reviewers",
    message: "Reviewers must be literal or come from binding parameters",
};
pub(super) const STATIC_PARAMS: StaticRefusal = StaticRefusal {
    code: "registry.static_params",
    message: "Child parameters must be literal or come from binding parameters",
};

struct Contract {
    input: Value,
    output: Value,
}
fn contract(properties: Value, required: &[&str], output: Value) -> Contract {
    Contract {
        input: object(properties, required),
        output,
    }
}
/// Preserve native local reference roots when a caller schema is a nested contract.
fn embedded(mut schema: Value, field: &str) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object
            .entry("$id")
            .or_insert_with(|| json!(format!("urn:workflow:contract:{field}")));
    }
    schema
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod decision_tests;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod schema_tests;

#[cfg(test)]
mod review1_tests;

#[cfg(test)]
mod message_tests;

fn validate_declarations(parsed: &ParsedWorkflow) -> Result<(), Diagnostic> {
    for (id, step) in &parsed.definition.steps {
        for field in ["output_schema", "data_schema", "payload_schema"] {
            let Some(Binding::Literal(value)) = step.inputs.get(field) else {
                continue;
            };
            let path = format!("/steps/{id}/with/{field}");
            Schema::compile(
                value.clone(),
                &path,
                compiler::input_span(&parsed.locations, &path),
            )?;
        }
    }
    Ok(())
}

fn validate_jev_resources(parsed: &ParsedWorkflow) -> Result<(), Diagnostic> {
    for resource in parsed
        .definition
        .resources
        .iter()
        .filter(|r| r.kind.as_str() == "typesafe")
    {
        let used = parsed.definition.steps.values().any(|step| step.step_type.as_str() == "jev.evaluate"
            && matches!(step.inputs.get("connection"), Some(Binding::Literal(value)) if value.as_str() == Some(resource.slot.as_str())));
        if !used {
            return Err(Diagnostic::at(
                "registry.resource",
                "TypeSafe resource slots must be referenced by a Jev step",
                "/resources",
                parsed.locations[""],
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod jev_tests;

#[cfg(test)]
mod jev_box_tests;
