//! Pure, bounded compiler catalogue. Requirements are not authorization grants;
//! no entry here has an execution handler.
mod actions;
mod budget;
mod checks;
mod examples;
mod facts;
mod families;
mod help;
mod mcp;
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
pub(crate) use checks::InputCheck;
pub use facts::{CapabilityProfile, CatalogueFacts, ChildContract, ToolContract};
pub use help::{AuthoringHelp, authoring_help};
pub use metadata::{
    CapabilityRequirement, EffectClass, ExecutionConstraints, ProfileSelection, RecoveryMode,
};
use schemas::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_REVISION: &str = "workflow-catalogue-v1.2";
pub const MAX_FACTS: usize = 256;
pub const MAX_SELECTIONS: usize = 128;
pub const MAX_ITEMS: usize = 256;
pub const MAX_TEXT: usize = 16_384;
pub const MAX_HEADERS: usize = 64;
pub const MAX_CONTEXT_TOKENS: usize = 131_072;
pub const MAX_IDENTIFIER: usize = 128;
pub const MAX_PATH: usize = 2048;
pub const MAX_DEADLINE: usize = 64;
pub const MAX_CORRELATION: usize = 256;
pub const TYPES: [&str; 18] = [
    "context.load",
    "memory.load",
    "memory.save",
    "ai.classify",
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
    compiler::check_resolved_budget(&parsed, &descriptors, dependencies)?;
    validate_declarations(&parsed)?;
    compiler::compile_resolved(decoded, parsed, &descriptors, dependencies)
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
        let selection = if name != "agent.run" {
            ProfileSelection::NotApplicable
        } else if self.parsed.definition.steps[self.id]
            .inputs
            .contains_key("capability_profile")
        {
            ProfileSelection::ExplicitSelection
        } else {
            ProfileSelection::AgentDefaults
        };
        let metadata = metadata::constraints(name, selection);
        Ok(SpecializedDescriptor {
            descriptor,
            facts: json!({"revision":CONTRACT_REVISION,"constraints":metadata,"effective":self.effective,"literal_fields":self.static_fields,"checks":self.checks}),
            checks: self.checks,
        })
    }
}
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
mod profile_tests;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod schema_tests;

#[cfg(test)]
mod review1_tests;

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
