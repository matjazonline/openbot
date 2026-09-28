use super::FactBudget;
use super::shape::check_operator_operands;
use super::syntax::child;
use super::{
    DecodedSource, Diagnostic, ParsedWorkflow, Schema, SourceSpan, check_availability,
    check_binding_shape, check_dependencies, parse_workflow,
};
use crate::domain::workflow::{
    Binding, ChoiceName, Context, ContextError, GraphError, OrderedRule, Routes, StepId, TypeName,
    ValidatedWorkflow, VersionId, WorkflowId, preflight_binding, resolve_inputs, validate,
    validate_context_value,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteContract {
    Success,
    Choices(BTreeSet<ChoiceName>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    None,
    ChildCall,
    Repeat,
}

#[derive(Debug, Clone)]
pub struct StepDescriptor {
    pub type_name: TypeName,
    pub input_schema: Value,
    pub output_schema: Value,
    pub routes: RouteContract,
    pub control: ControlKind,
    pub ordered_rule: bool,
}

/// A compiler input. Registry-owned descriptors and child dependency facts must
/// be supplied by the caller; this type does not grant execution authority.
pub struct CompileFacts<'a> {
    pub descriptors: &'a BTreeMap<TypeName, StepDescriptor>,
    pub dependencies: &'a BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
}

pub struct CompiledWorkflow {
    source: String,
    representation: Value,
    source_map: BTreeMap<String, SourceSpan>,
    content_hash: String,
    graph: ValidatedWorkflow,
    input: Schema,
    params: Schema,
    output: Schema,
    step_inputs: BTreeMap<StepId, Schema>,
    step_outputs: BTreeMap<StepId, Schema>,
    rules: BTreeMap<StepId, OrderedRule>,
    controls: BTreeMap<StepId, super::StepControl>,
    checks: BTreeMap<StepId, Vec<crate::application::workflow::registry::InputCheck>>,
}

pub struct PreparedInputs {
    value: Value,
}
impl PreparedInputs {
    pub fn value(&self) -> &Value {
        &self.value
    }
}

impl CompiledWorkflow {
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn representation(&self) -> &Value {
        &self.representation
    }
    pub fn source_map(&self) -> &BTreeMap<String, SourceSpan> {
        &self.source_map
    }
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
    pub fn graph(&self) -> &ValidatedWorkflow {
        &self.graph
    }
    pub fn rule(&self, step: &StepId) -> Option<&OrderedRule> {
        self.rules.get(step)
    }
    pub fn child_control(&self, step: &StepId) -> Option<&super::StepControl> {
        self.controls.get(step)
    }

    fn span(&self, path: &str) -> SourceSpan {
        self.source_map
            .get(path)
            .copied()
            .unwrap_or(self.source_map[""])
    }

    pub fn validate_input(&self, value: &Value) -> Result<(), Diagnostic> {
        validate_context_value(value, self.graph.context_limits()).map_err(|_| {
            Diagnostic::at(
                "context.limit",
                "Workflow input exceeds context limits",
                "/input",
                self.span("/input_schema"),
            )
        })?;
        self.input
            .validate(value, "/input", self.span("/input_schema"))
    }

    pub fn validate_params(&self, value: &Value) -> Result<(), Diagnostic> {
        validate_context_value(value, self.graph.context_limits()).map_err(|_| {
            Diagnostic::at(
                "context.limit",
                "Workflow parameters exceed context limits",
                "/params",
                self.span("/parameter_schema"),
            )
        })?;
        self.params
            .validate(value, "/params", self.span("/parameter_schema"))
    }

    pub fn validate_output(&self, value: &Value) -> Result<(), Diagnostic> {
        validate_context_value(value, self.graph.context_limits()).map_err(|_| {
            Diagnostic::at(
                "context.limit",
                "Workflow output exceeds context limits",
                "/output",
                self.span("/output_schema"),
            )
        })?;
        self.output
            .validate(value, "/output", self.span("/output_schema"))
    }

    /// Resolves all input bindings once into an owned immutable snapshot. The
    /// caller must durably persist it before dispatch; this API performs no I/O.
    pub fn prepare_step_inputs(
        &self,
        step: &StepId,
        context: &Context<'_>,
    ) -> Result<PreparedInputs, Diagnostic> {
        let Some(definition) = self.graph.definition().steps.get(step) else {
            return Err(Diagnostic::at(
                "step.unknown",
                "Unknown workflow step",
                "/steps",
                self.span("/steps"),
            ));
        };
        self.validate_input(context.input)?;
        self.validate_params(context.params)?;
        let root = format!("/steps/{step}/with");
        let value = resolve_inputs(&definition.inputs, context, self.graph.context_limits())
            .map_err(|(name, error)| {
                let path = name.map_or_else(|| root.clone(), |name| child(&root, name));
                runtime_binding_diagnostic(error, &path, self.span(&path))
            })?;
        self.step_inputs[step].validate(&value, &root, self.span(&root))?;
        for check in self.checks.get(step).into_iter().flatten() {
            check
                .validate(&value, &root, self.span(&root))
                .map_err(|mut error| {
                    error.span = super::input_span(&self.source_map, &error.field_path);
                    error
                })?;
        }
        Ok(PreparedInputs { value })
    }

    /// Validate only the named step's handler output before the runtime commits it.
    pub fn validate_step_output(&self, step: &StepId, value: &Value) -> Result<(), Diagnostic> {
        let Some(schema) = self.step_outputs.get(step) else {
            return Err(Diagnostic::at(
                "step.unknown",
                "Unknown workflow step",
                "/steps",
                self.span("/steps"),
            ));
        };
        let path = format!("/steps/{step}/output");
        validate_context_value(value, self.graph.context_limits()).map_err(|_| {
            Diagnostic::at(
                "context.limit",
                "Handler output exceeds context limits",
                &path,
                self.span(&format!("/steps/{step}")),
            )
        })?;
        schema.validate(value, &path, self.span(&format!("/steps/{step}")))
    }
}

fn runtime_binding_diagnostic(error: ContextError, path: &str, span: SourceSpan) -> Diagnostic {
    let (code, message) = match error {
        ContextError::Missing(_) => (
            "binding.missing",
            "Required step input reference is missing",
        ),
        ContextError::InvalidReference(_) => (
            "binding.reference",
            "Step input reference is invalid or traverses a scalar",
        ),
        ContextError::Limit(_) => ("context.limit", "Step input resolution exceeds a limit"),
        ContextError::Operand(_) => ("binding.operand", "Step input operand has the wrong type"),
    };
    Diagnostic::at(code, message, path, span)
}

fn located(parsed: &ParsedWorkflow, path: &str) -> SourceSpan {
    parsed
        .locations
        .get(path)
        .copied()
        .unwrap_or(parsed.locations[""])
}

fn graph_error(error: GraphError, parsed: &ParsedWorkflow) -> Diagnostic {
    let path = match &error {
        GraphError::MissingEntry(_) => "/entry".to_owned(),
        GraphError::MissingTarget { step, route, .. } => {
            if route == "success" {
                format!("/steps/{step}/routes/success")
            } else if route == "final_error" {
                format!("/steps/{step}/routes/error")
            } else {
                format!("/steps/{step}/routes/choices/{route}")
            }
        }
        GraphError::DuplicateChoice { step, .. } | GraphError::EmptyChoices { step } => {
            format!("/steps/{step}/routes/choices")
        }
        GraphError::Cycle(step) | GraphError::Unreachable(step) => format!("/steps/{step}"),
        GraphError::InvalidExecutionLimit { field, .. } => format!("/limits/{field}"),
        _ => "/steps".to_owned(),
    };
    Diagnostic::at(
        "graph.invalid",
        &error.to_string(),
        &path,
        located(parsed, &path),
    )
}

fn check_descriptor(
    id: &StepId,
    step: &crate::domain::workflow::StepDefinition,
    descriptor: &StepDescriptor,
    parsed: &ParsedWorkflow,
) -> Result<(), Diagnostic> {
    let path = format!("/steps/{id}");
    let valid_routes = match (&step.routes, &descriptor.routes) {
        (Routes::Success(_), RouteContract::Success) => true,
        (Routes::Choices(actual), RouteContract::Choices(declared)) => {
            actual
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<BTreeSet<_>>()
                == *declared
        }
        _ => false,
    };
    if !valid_routes {
        return Err(Diagnostic::at(
            "step.routes",
            "Routes do not match the step descriptor",
            &format!("{path}/routes"),
            located(parsed, &format!("{path}/routes")),
        ));
    }
    if descriptor.ordered_rule && !matches!(descriptor.routes, RouteContract::Choices(_)) {
        return Err(Diagnostic::at(
            "step.descriptor",
            "Ordered-rule descriptor must declare choice routes",
            &path,
            located(parsed, &path),
        ));
    }
    let control = parsed.controls.get(id);
    let valid_control = match descriptor.control {
        ControlKind::None => control.is_none(),
        ControlKind::ChildCall => control.is_some_and(|c| c.max_iterations.is_none()),
        ControlKind::Repeat => control.is_some_and(|c| c.max_iterations.is_some()),
    };
    if !valid_control {
        return Err(Diagnostic::at(
            "step.control",
            "Child control and repeat bounds do not match the descriptor",
            &path,
            located(parsed, &path),
        ));
    }
    match (descriptor.ordered_rule, parsed.rules.get(id)) {
        (true, Some(rule)) => {
            let Routes::Choices(choices) = &step.routes else {
                return Err(Diagnostic::at(
                    "step.descriptor",
                    "Ordered rule requires choice routes",
                    &path,
                    located(parsed, &path),
                ));
            };
            let names = choices.iter().map(|(name, _)| name.clone()).collect();
            rule.check(&names).map_err(|error| {
                Diagnostic::at(
                    "rule.invalid",
                    &error.to_string(),
                    &format!("{path}/rule"),
                    located(parsed, &format!("{path}/rule")),
                )
            })?;
        }
        (false, None) => {}
        _ => {
            return Err(Diagnostic::at(
                "rule.shape",
                "Ordered rule is required only by a rule descriptor",
                &format!("{path}/rule"),
                located(parsed, &path),
            ));
        }
    }
    Ok(())
}

fn check_inputs(
    id: &StepId,
    step: &crate::domain::workflow::StepDefinition,
    schema: &Schema,
    parsed: &ParsedWorkflow,
) -> Result<(), Diagnostic> {
    let root = format!("/steps/{id}/with");
    if !schema.object_possible() {
        return Err(Diagnostic::at(
            "binding.type",
            "Step input schema cannot accept an object",
            &root,
            located(parsed, &root),
        ));
    }
    for (name, binding) in &step.inputs {
        let path = child(&root, name);
        preflight_binding(
            binding,
            crate::domain::workflow::ContextLimits {
                output_bytes: parsed.definition.limits.max_context_bytes,
                work_nodes: 65_536,
            },
        )
        .map_err(|_| {
            Diagnostic::at(
                "binding.limit",
                "Binding exceeds depth, node or byte limits",
                &path,
                located(parsed, &path),
            )
        })?;
        check_operator_operands(binding, &path, located(parsed, &path))?;
        for constraint in schema.root_constraints() {
            let expected = constraint
                .get("properties")
                .and_then(|properties| properties.get(name))
                .or_else(|| constraint.get("additionalProperties"));
            if let Some(expected) = expected {
                if !matches!(binding, Binding::Literal(_))
                    && super::schema::is_known_empty(expected, schema.raw())
                {
                    return Err(Diagnostic::at(
                        "binding.property",
                        "Destination schema forbids this input",
                        &path,
                        located(parsed, &path),
                    ));
                }
                check_binding_shape(
                    binding,
                    expected,
                    schema.raw(),
                    &path,
                    located(parsed, &path),
                )?;
            }
        }
    }
    for constraint in schema.root_constraints() {
        for name in constraint
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !step.inputs.contains_key(name) {
                return Err(Diagnostic::at(
                    "binding.required",
                    "Required input binding is missing",
                    &root,
                    located(parsed, &root),
                ));
            }
        }
    }
    Ok(())
}

fn descriptor_fact(descriptor: &StepDescriptor) -> impl serde::Serialize + '_ {
    struct ChoiceNames<'a>(&'a BTreeSet<ChoiceName>);
    impl serde::Serialize for ChoiceNames<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_seq(self.0.iter().map(ChoiceName::as_str))
        }
    }
    #[derive(serde::Serialize)]
    #[serde(untagged)]
    enum BorrowedRoutes<'a> {
        Success { success: bool },
        Choices { choices: ChoiceNames<'a> },
    }
    let routes = match &descriptor.routes {
        RouteContract::Success => BorrowedRoutes::Success { success: true },
        RouteContract::Choices(names) => BorrowedRoutes::Choices {
            choices: ChoiceNames(names),
        },
    };
    #[derive(serde::Serialize)]
    struct BorrowedContract<'a> {
        #[serde(rename = "type")]
        type_name: &'a str,
        input_schema: &'a Value,
        output_schema: &'a Value,
        routes: BorrowedRoutes<'a>,
        control: &'static str,
        ordered_rule: bool,
    }
    BorrowedContract {
        type_name: descriptor.type_name.as_str(),
        input_schema: &descriptor.input_schema,
        output_schema: &descriptor.output_schema,
        routes,
        control: match descriptor.control {
            ControlKind::None => "None",
            ControlKind::ChildCall => "ChildCall",
            ControlKind::Repeat => "Repeat",
        },
        ordered_rule: descriptor.ordered_rule,
    }
}

fn identity(source: &str, representation: &Value) -> String {
    let encoded = serde_json::to_vec(representation).expect("JSON representation");
    let mut hash = Sha256::new();
    hash.update(b"workflow-compiled-v1\0");
    hash.update((source.len() as u64).to_be_bytes());
    hash.update(source.as_bytes());
    hash.update((encoded.len() as u64).to_be_bytes());
    hash.update(&encoded);
    format!("{:x}", hash.finalize())
}

struct StepSchemas {
    inputs: BTreeMap<StepId, Schema>,
    outputs: BTreeMap<StepId, Schema>,
    facts: Map<String, Value>,
}

fn compile_steps(
    parsed: &ParsedWorkflow,
    descriptors: &BTreeMap<StepId, SpecializedDescriptor>,
) -> Result<StepSchemas, Diagnostic> {
    let mut result = StepSchemas {
        inputs: BTreeMap::new(),
        outputs: BTreeMap::new(),
        facts: Map::new(),
    };
    for (id, step) in &parsed.definition.steps {
        let specialized = descriptors.get(id).ok_or_else(|| {
            Diagnostic::at(
                "step.type",
                "Unknown step type",
                &format!("/steps/{id}/type"),
                located(parsed, &format!("/steps/{id}/type")),
            )
        })?;
        let descriptor = &specialized.descriptor;
        if descriptor.type_name != step.step_type {
            return Err(Diagnostic::at(
                "step.descriptor",
                "Descriptor key and type identity disagree",
                &format!("/steps/{id}/type"),
                located(parsed, &format!("/steps/{id}/type")),
            ));
        }
        check_descriptor(id, step, descriptor, parsed)?;
        let input_schema = Schema::compile(
            descriptor.input_schema.clone(),
            &format!("/steps/{id}/with"),
            located(parsed, &format!("/steps/{id}/with")),
        )?;
        let output_schema = Schema::compile(
            descriptor.output_schema.clone(),
            &format!("/steps/{id}/output"),
            located(parsed, &format!("/steps/{id}")),
        )?;
        check_inputs(id, step, &input_schema, parsed)?;
        check_rule_predicates(id, parsed)?;
        result.inputs.insert(id.clone(), input_schema);
        result.outputs.insert(id.clone(), output_schema);
        result.facts.insert(
            id.to_string(),
            json!({"contract": descriptor_fact(descriptor), "registration": specialized.facts}),
        );
    }
    Ok(result)
}

fn check_rule_predicates(id: &StepId, parsed: &ParsedWorkflow) -> Result<(), Diagnostic> {
    let Some(rule) = parsed.rules.get(id) else {
        return Ok(());
    };
    let boolean = json!({"type":"boolean"});
    for (index, case) in rule.cases.iter().enumerate() {
        let path = format!("/steps/{id}/rule/cases/{index}/when");
        preflight_binding(
            &case.when,
            crate::domain::workflow::ContextLimits {
                output_bytes: parsed.definition.limits.max_context_bytes,
                work_nodes: 65_536,
            },
        )
        .map_err(|_| {
            Diagnostic::at(
                "binding.limit",
                "Rule predicate exceeds binding limits",
                &path,
                located(parsed, &path),
            )
        })?;
        check_binding_shape(
            &case.when,
            &boolean,
            &boolean,
            &path,
            located(parsed, &path),
        )?;
    }
    Ok(())
}

pub fn compile(
    decoded: DecodedSource,
    version_id: VersionId,
    facts: CompileFacts<'_>,
) -> Result<CompiledWorkflow, Diagnostic> {
    let parsed = parse_workflow(&decoded, version_id)?;
    let mut budget = FactBudget::new(located(&parsed, "/steps"));
    let mut exact = FactBudget::new(located(&parsed, "/steps"));
    exact.punctuation(5)?;
    exact.charge(&dependency_facts(&parsed, facts.dependencies)?)?;
    for (index, (id, step)) in parsed.definition.steps.iter().enumerate() {
        if let Some(descriptor) = facts.descriptors.get(&step.step_type) {
            budget.schema(&descriptor.input_schema)?;
            budget.schema(&descriptor.output_schema)?;
            exact.punctuation(usize::from(index > 0))?;
            charge_descriptor(&mut exact, id, descriptor, &Value::Null)?;
        }
    }
    let descriptors = parsed
        .definition
        .steps
        .iter()
        .filter_map(|(id, step)| {
            facts.descriptors.get(&step.step_type).map(|descriptor| {
                (
                    id.clone(),
                    SpecializedDescriptor {
                        descriptor: descriptor.clone(),
                        facts: Value::Null,
                        checks: Vec::new(),
                    },
                )
            })
        })
        .collect();
    compile_resolved(decoded, parsed, &descriptors, facts.dependencies)
}

pub(crate) struct SpecializedDescriptor {
    pub descriptor: StepDescriptor,
    pub facts: Value,
    pub checks: Vec<crate::application::workflow::registry::InputCheck>,
}

pub(crate) fn compile_resolved(
    decoded: DecodedSource,
    parsed: ParsedWorkflow,
    descriptors: &BTreeMap<StepId, SpecializedDescriptor>,
    dependencies: &BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
) -> Result<CompiledWorkflow, Diagnostic> {
    let dependencies = dependency_facts(&parsed, dependencies)?;
    check_expanded_facts(descriptors, &dependencies, located(&parsed, "/steps"))?;
    let graph = validate(parsed.definition.clone()).map_err(|error| graph_error(error, &parsed))?;
    let input = Schema::compile(
        parsed.definition.input_schema.clone().expect("required"),
        "/input_schema",
        located(&parsed, "/input_schema"),
    )?;
    let params = Schema::compile(
        parsed
            .definition
            .parameter_schema
            .clone()
            .expect("required"),
        "/parameter_schema",
        located(&parsed, "/parameter_schema"),
    )?;
    let output = Schema::compile(
        parsed.definition.output_schema.clone().expect("required"),
        "/output_schema",
        located(&parsed, "/output_schema"),
    )?;
    let steps = compile_steps(&parsed, descriptors)?;
    check_availability(&decoded, &parsed, &input, &params, &steps.outputs)?;
    let representation = json!({
        "format": "workflow-compiled-v1",
        "definition": super::json_value(&decoded.root, "")?,
        "descriptors": steps.facts,
        "dependencies": dependencies,
    });
    let content_hash = identity(&decoded.source, &representation);
    Ok(CompiledWorkflow {
        source: decoded.source,
        representation,
        source_map: parsed.locations,
        content_hash,
        graph,
        input,
        params,
        output,
        step_inputs: steps.inputs,
        step_outputs: steps.outputs,
        rules: parsed.rules,
        controls: parsed.controls,
        checks: descriptors
            .iter()
            .map(|(id, d)| (id.clone(), d.checks.clone()))
            .collect(),
    })
}

#[cfg(test)]
#[path = "compile_test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "compile_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "compile_validation_tests.rs"]
mod validation_tests;

#[cfg(test)]
#[path = "review1_tests.rs"]
mod review1_tests;

fn dependency_facts(
    parsed: &ParsedWorkflow,
    dependencies: &BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
) -> Result<BTreeMap<String, Vec<String>>, Diagnostic> {
    let children = parsed
        .controls
        .values()
        .map(|c| c.child_workflow_id)
        .collect();
    let dependency_closure = check_dependencies(
        parsed.definition.workflow_id,
        &children,
        dependencies,
        located(parsed, "/steps"),
    )?;
    let dependencies = dependency_closure
        .iter()
        .map(|(id, children)| {
            (
                id.to_string(),
                children.iter().map(ToString::to_string).collect(),
            )
        })
        .collect();
    Ok(dependencies)
}

pub(crate) fn charge_descriptor(
    budget: &mut FactBudget,
    id: &StepId,
    descriptor: &StepDescriptor,
    registration: &Value,
) -> Result<(), Diagnostic> {
    // Serialize the exact representation through borrowed fields; no JSON tree copy.
    #[derive(serde::Serialize)]
    struct Fact<'a, C: serde::Serialize> {
        contract: C,
        registration: &'a Value,
    }
    budget.charge(&id.as_str())?;
    budget.punctuation(1)?; // colon (commas charged by caller)
    budget.charge(&Fact {
        contract: descriptor_fact(descriptor),
        registration,
    })
}
fn check_expanded_facts(
    descriptors: &BTreeMap<StepId, SpecializedDescriptor>,
    dependencies: &BTreeMap<String, Vec<String>>,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    let mut budget = FactBudget::new(span);
    budget.punctuation(5)?; // [{},dependencies] excluding dependencies themselves
    for (index, (id, descriptor)) in descriptors.iter().enumerate() {
        budget.punctuation(usize::from(index > 0))?;
        charge_descriptor(&mut budget, id, &descriptor.descriptor, &descriptor.facts)?;
    }
    budget.charge(dependencies)
}

#[cfg(test)]
use super::fact_budget::MAX_COMPILED_FACT_BYTES;
#[cfg(test)]
fn check_fact_budget(
    descriptors: &Map<String, Value>,
    dependencies: &BTreeMap<String, Vec<String>>,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    FactBudget::new(span).charge(&(descriptors, dependencies))
}

#[cfg(test)]
#[path = "aggregate_tests.rs"]
mod aggregate_tests;

pub(crate) fn check_resolved_budget(
    parsed: &ParsedWorkflow,
    descriptors: &BTreeMap<StepId, SpecializedDescriptor>,
    dependencies: &BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
) -> Result<(), Diagnostic> {
    check_expanded_facts(
        descriptors,
        &dependency_facts(parsed, dependencies)?,
        located(parsed, "/steps"),
    )
}
