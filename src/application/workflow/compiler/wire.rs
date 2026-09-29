use super::syntax::child;
use super::{
    DecodedSource, Diagnostic, LocatedNode, NodeValue, SourceSpan, fields, json_value,
    parse_binding, required, scalar_string, sequence,
};
use crate::domain::workflow::{
    BudgetResource, ChoiceName, ExecutionLimits, OrderedRule, ResourceName, ResourceRequirement,
    RootBudgetLimits, Routes, RuleCase, StepDefinition, StepId, TransitionTarget, TypeName,
    VersionId, WorkflowDefinition, WorkflowId,
};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

pub(crate) struct ParsedWorkflow {
    pub definition: WorkflowDefinition,
    pub controls: BTreeMap<StepId, StepControl>,
    pub rules: BTreeMap<StepId, OrderedRule>,
    pub locations: BTreeMap<String, SourceSpan>,
}

#[derive(Debug, Clone)]
pub struct StepControl {
    pub child_workflow_id: WorkflowId,
    pub child_version_id: VersionId,
    pub max_iterations: Option<u32>,
}

fn name<T>(
    node: &LocatedNode,
    path: &str,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Result<T, Diagnostic> {
    let raw = scalar_string(node, path)?;
    parse(raw).ok_or_else(|| Diagnostic::at("syntax.name", "Invalid checked name", path, node.span))
}

fn uuid(node: &LocatedNode, path: &str) -> Result<Uuid, Diagnostic> {
    name(node, path, |s| Uuid::parse_str(s).ok())
}

fn positive_u32(node: &LocatedNode, path: &str, max: u32) -> Result<u32, Diagnostic> {
    let NodeValue::Scalar(Value::Number(value)) = &node.value else {
        return Err(Diagnostic::at(
            "syntax.integer",
            "Expected a positive integer",
            path,
            node.span,
        ));
    };
    match value.as_u64().and_then(|n| u32::try_from(n).ok()) {
        Some(value) if value > 0 && value <= max => Ok(value),
        _ => Err(Diagnostic::at(
            "syntax.integer",
            "Positive integer is outside the permitted range",
            path,
            node.span,
        )),
    }
}

fn target(node: &LocatedNode, path: &str) -> Result<TransitionTarget, Diagnostic> {
    let raw = scalar_string(node, path)?;
    if raw == "$end" {
        return Ok(TransitionTarget::End);
    }
    StepId::parse(raw)
        .map(TransitionTarget::Step)
        .map_err(|_| Diagnostic::at("route.target", "Invalid route target", path, node.span))
}

fn routes(
    node: &LocatedNode,
    path: &str,
) -> Result<(Routes, Option<TransitionTarget>), Diagnostic> {
    let map = fields(node, path, &["success", "choices", "error"])?;
    let success = map.get("success");
    let choices = map.get("choices");
    let route = match (success, choices) {
        (Some(to), None) => Routes::Success(target(to, &child(path, "success"))?),
        (None, Some(choices)) => {
            let choice_path = child(path, "choices");
            let entries = fields(choices, &choice_path, &[])?;
            if entries.is_empty() {
                return Err(Diagnostic::at(
                    "route.choices",
                    "At least one choice is required",
                    &choice_path,
                    choices.span,
                ));
            }
            Routes::Choices(
                entries
                    .into_iter()
                    .map(|(name, to)| {
                        let at = child(&choice_path, &name);
                        Ok((
                            ChoiceName::parse(&name).map_err(|_| {
                                Diagnostic::at("route.choice", "Invalid choice name", &at, to.span)
                            })?,
                            target(to, &at)?,
                        ))
                    })
                    .collect::<Result<_, Diagnostic>>()?,
            )
        }
        _ => {
            return Err(Diagnostic::at(
                "route.shape",
                "Declare exactly one of success or choices",
                path,
                node.span,
            ));
        }
    };
    let error = map
        .get("error")
        .map(|node| target(node, &child(path, "error")))
        .transpose()?;
    Ok((route, error))
}

fn inputs(
    node: &LocatedNode,
    path: &str,
) -> Result<BTreeMap<String, crate::domain::workflow::Binding>, Diagnostic> {
    fields(node, path, &[])?
        .into_iter()
        .map(|(key, value)| Ok((key.clone(), parse_binding(value, &child(path, &key))?)))
        .collect()
}

fn rule(node: &LocatedNode, path: &str) -> Result<OrderedRule, Diagnostic> {
    let map = fields(node, path, &["cases", "default"])?;
    let cases_node = required(&map, "cases", path, node)?;
    let cases_path = child(path, "cases");
    let cases = sequence(cases_node, &cases_path)?
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let at = child(&cases_path, &i.to_string());
            let fields = fields(item, &at, &["when", "choice"])?;
            let when = parse_binding(required(&fields, "when", &at, item)?, &child(&at, "when"))?;
            let choice = name(
                required(&fields, "choice", &at, item)?,
                &child(&at, "choice"),
                |s| ChoiceName::parse(s).ok(),
            )?;
            Ok(RuleCase { when, choice })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let default = name(
        required(&map, "default", path, node)?,
        &child(path, "default"),
        |s| ChoiceName::parse(s).ok(),
    )?;
    Ok(OrderedRule { cases, default })
}

fn control(
    map: &BTreeMap<String, &LocatedNode>,
    step: &LocatedNode,
    path: &str,
) -> Result<Option<StepControl>, Diagnostic> {
    let child_id = map.get("child_workflow_id");
    let version_id = map.get("child_version_id");
    let iterations = map.get("max_iterations");
    match (child_id, version_id) {
        (None, None) if iterations.is_none() => Ok(None),
        (Some(workflow), Some(version)) => Ok(Some(StepControl {
            child_workflow_id: WorkflowId::new(uuid(workflow, &child(path, "child_workflow_id"))?),
            child_version_id: VersionId::new(uuid(version, &child(path, "child_version_id"))?),
            max_iterations: iterations
                .map(|node| positive_u32(node, &child(path, "max_iterations"), 10_000))
                .transpose()?,
        })),
        _ => Err(Diagnostic::at(
            "step.child",
            "Child workflow and pinned version must both be declared",
            path,
            step.span,
        )),
    }
}

struct ParsedSteps {
    steps: BTreeMap<StepId, StepDefinition>,
    controls: BTreeMap<StepId, StepControl>,
    rules: BTreeMap<StepId, OrderedRule>,
}

fn steps(node: &LocatedNode, path: &str) -> Result<ParsedSteps, Diagnostic> {
    let mut steps = BTreeMap::new();
    let mut controls = BTreeMap::new();
    let mut rules = BTreeMap::new();
    for (raw, item) in fields(node, path, &[])? {
        let at = child(path, &raw);
        let id = StepId::parse(&raw)
            .map_err(|_| Diagnostic::at("step.id", "Invalid step ID", &at, item.span))?;
        let map = fields(
            item,
            &at,
            &[
                "type",
                "with",
                "routes",
                "rule",
                "child_workflow_id",
                "child_version_id",
                "max_iterations",
            ],
        )?;
        let kind = name(
            required(&map, "type", &at, item)?,
            &child(&at, "type"),
            |s| TypeName::parse(s).ok(),
        )?;
        let (routes, final_error) =
            routes(required(&map, "routes", &at, item)?, &child(&at, "routes"))?;
        let values = inputs(required(&map, "with", &at, item)?, &child(&at, "with"))?;
        if let Some(control) = control(&map, item, &at)? {
            controls.insert(id.clone(), control);
        }
        if let Some(rule_node) = map.get("rule") {
            rules.insert(id.clone(), rule(rule_node, &child(&at, "rule"))?);
        }
        steps.insert(
            id,
            StepDefinition {
                step_type: kind,
                inputs: values,
                routes,
                final_error,
            },
        );
    }
    Ok(ParsedSteps {
        steps,
        controls,
        rules,
    })
}

fn resources(node: &LocatedNode, path: &str) -> Result<Vec<ResourceRequirement>, Diagnostic> {
    sequence(node, path)?
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let at = child(path, &i.to_string());
            let map = fields(item, &at, &["slot", "kind", "contract"])?;
            let slot = name(
                required(&map, "slot", &at, item)?,
                &child(&at, "slot"),
                |s| ResourceName::parse(s).ok(),
            )?;
            let kind = name(
                required(&map, "kind", &at, item)?,
                &child(&at, "kind"),
                |s| TypeName::parse(s).ok(),
            )?;
            let contract = map
                .get("contract")
                .map(|node| name(node, &child(&at, "contract"), |s| TypeName::parse(s).ok()))
                .transpose()?;
            Ok(ResourceRequirement {
                slot,
                kind,
                contract,
            })
        })
        .collect()
}

fn locations(node: &LocatedNode, path: &str, output: &mut BTreeMap<String, SourceSpan>) {
    output.insert(path.to_owned(), node.span);
    match &node.value {
        NodeValue::Sequence(items) => {
            for (i, child_node) in items.iter().enumerate() {
                locations(child_node, &child(path, &i.to_string()), output);
            }
        }
        NodeValue::Mapping(entries) => {
            for (key, child_node) in entries {
                if let NodeValue::Scalar(Value::String(name)) = &key.value {
                    locations(child_node, &child(path, name), output);
                }
            }
        }
        NodeValue::Scalar(_) => {}
    }
}

pub(crate) fn parse_workflow(
    decoded: &DecodedSource,
    version_id: VersionId,
) -> Result<ParsedWorkflow, Diagnostic> {
    let root = &decoded.root;
    let map = fields(
        root,
        "",
        &[
            "format_version",
            "workflow_id",
            "input_schema",
            "parameter_schema",
            "output_schema",
            "resources",
            "entry",
            "steps",
            "limits",
        ],
    )?;
    let format = required(&map, "format_version", "", root)?;
    if format.value_as_u64() != Some(1) {
        return Err(Diagnostic::at(
            "format.version",
            "Only format_version 1 is supported",
            "/format_version",
            format.span,
        ));
    }
    let workflow_id = WorkflowId::new(uuid(
        required(&map, "workflow_id", "", root)?,
        "/workflow_id",
    )?);
    let input_schema = json_value(required(&map, "input_schema", "", root)?, "/input_schema")?;
    let parameter_schema = json_value(
        required(&map, "parameter_schema", "", root)?,
        "/parameter_schema",
    )?;
    let output_schema = json_value(required(&map, "output_schema", "", root)?, "/output_schema")?;
    let resources = resources(required(&map, "resources", "", root)?, "/resources")?;
    let entry = name(required(&map, "entry", "", root)?, "/entry", |s| {
        StepId::parse(s).ok()
    })?;
    let parsed_steps = steps(required(&map, "steps", "", root)?, "/steps")?;
    let limits_node = required(&map, "limits", "", root)?;
    let limits = execution_limits(limits_node)?;
    let mut spans = BTreeMap::new();
    locations(root, "", &mut spans);
    Ok(ParsedWorkflow {
        definition: WorkflowDefinition {
            format_version: 1,
            workflow_id,
            version_id,
            input_schema: Some(input_schema),
            parameter_schema: Some(parameter_schema),
            output_schema: Some(output_schema),
            resources,
            entry,
            steps: parsed_steps.steps,
            limits,
        },
        controls: parsed_steps.controls,
        rules: parsed_steps.rules,
        locations: spans,
    })
}

fn execution_limits(limits_node: &LocatedNode) -> Result<ExecutionLimits, Diagnostic> {
    let limit_fields = fields(
        limits_node,
        "/limits",
        &["max_steps", "max_context_bytes", "root_budget"],
    )?;
    let max_steps = positive_u32(
        required(&limit_fields, "max_steps", "/limits", limits_node)?,
        "/limits/max_steps",
        100_000,
    )?;
    let max_context_bytes = positive_u32(
        required(&limit_fields, "max_context_bytes", "/limits", limits_node)?,
        "/limits/max_context_bytes",
        1_048_576,
    )? as usize;
    let root_budget = limit_fields
        .get("root_budget")
        .map(|node| root_budget_limits(node))
        .transpose()?
        .unwrap_or_default();
    Ok(ExecutionLimits {
        max_steps,
        max_context_bytes,
        root_budget,
    })
}

fn root_budget_limits(node: &LocatedNode) -> Result<RootBudgetLimits, Diagnostic> {
    let path = "/limits/root_budget";
    let values = fields(node, path, &["activations", "model_calls", "repetitions"])?;
    let read = |field: &str, resource: BudgetResource| {
        positive_u32(
            required(&values, field, path, node)?,
            &format!("{path}/{field}"),
            resource.maximum(),
        )
    };
    RootBudgetLimits::new(
        read("activations", BudgetResource::Activation)?,
        read("model_calls", BudgetResource::ModelCall)?,
        read("repetitions", BudgetResource::Repetition)?,
    )
    .map_err(|_| Diagnostic::at("syntax.integer", "Invalid root budget", path, node.span))
}

trait NumericNode {
    fn value_as_u64(&self) -> Option<u64>;
}
impl NumericNode for LocatedNode {
    fn value_as_u64(&self) -> Option<u64> {
        match &self.value {
            NodeValue::Scalar(Value::Number(n)) => n.as_u64(),
            _ => None,
        }
    }
}
