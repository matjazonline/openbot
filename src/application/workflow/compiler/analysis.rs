use super::syntax::{child, fields, sequence};
use super::{
    DecodedSource, Diagnostic, LocatedNode, NodeValue, ParsedWorkflow, PathGuarantee, Schema,
};
use crate::domain::workflow::{ContextReference, Routes, StepId, TransitionTarget, WorkflowId};
use std::collections::{BTreeMap, BTreeSet};

struct ReferenceUse {
    reference: ContextReference,
    optional: bool,
    path: String,
    span: super::SourceSpan,
}

fn node_at<'a>(root: &'a LocatedNode, path: &str) -> Option<&'a LocatedNode> {
    let mut node = root;
    for segment in path.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        node = match &node.value {
            NodeValue::Mapping(entries) => entries.iter().find(|(key, _)|
                matches!(&key.value, NodeValue::Scalar(serde_json::Value::String(value)) if value == &segment)
            ).map(|(_, value)| value)?,
            NodeValue::Sequence(items) => items.get(segment.parse::<usize>().ok()?)?,
            NodeValue::Scalar(_) => return None,
        };
    }
    Some(node)
}

fn add_reference(node: &LocatedNode, path: &str, optional: bool, output: &mut Vec<ReferenceUse>) {
    if let NodeValue::Scalar(serde_json::Value::String(raw)) = &node.value
        && let Ok(reference) = ContextReference::parse(raw)
    {
        output.push(ReferenceUse {
            reference,
            optional,
            path: path.into(),
            span: node.span,
        });
    }
}

fn walk_binding(node: &LocatedNode, path: &str, output: &mut Vec<ReferenceUse>) {
    let Ok(map) = fields(node, path, &[]) else {
        return;
    };
    let Some((operator, operand)) = map.into_iter().next() else {
        return;
    };
    let at = child(path, &operator);
    match operator.as_str() {
        "ref" => add_reference(operand, &at, false, output),
        "exists" => add_reference(operand, &at, true, output),
        "default" => {
            if let Ok(parts) = fields(operand, &at, &[]) {
                if let Some(reference) = parts.get("ref") {
                    add_reference(reference, &child(&at, "ref"), true, output);
                }
                if let Some(fallback) = parts.get("value") {
                    walk_binding(fallback, &child(&at, "value"), output);
                }
            }
        }
        "object" => {
            if let Ok(fields) = fields(operand, &at, &[]) {
                for (key, node) in fields {
                    walk_binding(node, &child(&at, &key), output);
                }
            }
        }
        "array" | "concat" | "and" | "or" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "in" => {
            if let Ok(items) = sequence(operand, &at) {
                for (i, node) in items.iter().enumerate() {
                    walk_binding(node, &child(&at, &i.to_string()), output);
                }
            }
        }
        "not" => walk_binding(operand, &at, output),
        _ => {}
    }
}

fn incoming(
    definition: &crate::domain::workflow::WorkflowDefinition,
) -> BTreeMap<StepId, Vec<(StepId, bool)>> {
    let mut result: BTreeMap<StepId, Vec<(StepId, bool)>> = definition
        .steps
        .keys()
        .cloned()
        .map(|id| (id, Vec::new()))
        .collect();
    for (from, step) in &definition.steps {
        let mut add = |target: &TransitionTarget, success: bool| {
            if let TransitionTarget::Step(to) = target {
                result
                    .get_mut(to)
                    .expect("validated graph")
                    .push((from.clone(), success));
            }
        };
        match &step.routes {
            Routes::Success(target) => add(target, true),
            Routes::Choices(choices) => {
                for (_, target) in choices {
                    add(target, true);
                }
            }
        }
        if let Some(error) = &step.final_error {
            add(error, false);
        }
    }
    result
}

fn availability(
    definition: &crate::domain::workflow::WorkflowDefinition,
) -> BTreeMap<StepId, BTreeSet<StepId>> {
    let edges = incoming(definition);
    let mut available: BTreeMap<StepId, BTreeSet<StepId>> = BTreeMap::new();
    while available.len() < definition.steps.len() {
        let Some((id, predecessors)) = edges.iter().find(|(id, predecessors)| {
            !available.contains_key(*id)
                && predecessors
                    .iter()
                    .all(|(from, _)| available.contains_key(from))
        }) else {
            break;
        };
        let common = predecessors
            .iter()
            .map(|(from, success)| {
                let mut set = available[from].clone();
                if *success {
                    set.insert(from.clone());
                }
                set
            })
            .reduce(|a, b| a.intersection(&b).cloned().collect())
            .unwrap_or_default();
        available.insert(id.clone(), common);
    }
    available
}

fn check_reference(
    use_: ReferenceUse,
    current: &StepId,
    available: &BTreeSet<StepId>,
    parsed: &ParsedWorkflow,
    input: &Schema,
    params: &Schema,
    outputs: &BTreeMap<StepId, Schema>,
) -> Result<(), Diagnostic> {
    let reference = &use_.reference;
    let schema = match reference.root_name() {
        "input" => input,
        "params" => params,
        "steps" => {
            let id = reference.step_id().expect("step root");
            let Some(schema) = outputs.get(id) else {
                return Err(Diagnostic::at(
                    "reference.step",
                    "Reference names an unknown step",
                    &use_.path,
                    use_.span,
                ));
            };
            if (id == current || !available.contains(id)) && !use_.optional {
                return Err(Diagnostic::at(
                    "reference.path",
                    "Step output is not available on every incoming route; use an explicit default",
                    &use_.path,
                    use_.span,
                ));
            }
            schema
        }
        "run.id" | "run.parent_id" => return Ok(()),
        _ => unreachable!(),
    };
    let guarantee = schema.guarantees(reference.tail());
    match guarantee {
        PathGuarantee::Impossible => Err(Diagnostic::at(
            "reference.impossible",
            "Reference cannot exist under its schema",
            &use_.path,
            use_.span,
        )),
        PathGuarantee::UnsafeTraversal => Err(Diagnostic::at(
            "reference.traversal",
            "Schema permits scalar traversal; a default only catches missing values",
            &use_.path,
            use_.span,
        )),
        PathGuarantee::Optional if !use_.optional => Err(Diagnostic::at(
            "reference.optional",
            "Schema does not guarantee this path; use an explicit default",
            &use_.path,
            use_.span,
        )),
        _ => {
            let _ = parsed;
            Ok(())
        }
    }
}

pub(crate) fn check_availability(
    decoded: &DecodedSource,
    parsed: &ParsedWorkflow,
    input: &Schema,
    params: &Schema,
    outputs: &BTreeMap<StepId, Schema>,
) -> Result<(), Diagnostic> {
    let sets = availability(&parsed.definition);
    for (id, step) in &parsed.definition.steps {
        let base = format!("/steps/{id}");
        let mut uses = Vec::new();
        for name in step.inputs.keys() {
            let path = child(&child(&base, "with"), name);
            if let Some(node) = node_at(&decoded.root, &path) {
                walk_binding(node, &path, &mut uses);
            }
        }
        if let Some(rule) = parsed.rules.get(id) {
            for i in 0..rule.cases.len() {
                let path = format!("{base}/rule/cases/{i}/when");
                if let Some(node) = node_at(&decoded.root, &path) {
                    walk_binding(node, &path, &mut uses);
                }
            }
        }
        for reference in uses {
            check_reference(reference, id, &sets[id], parsed, input, params, outputs)?;
        }
    }
    Ok(())
}

pub(crate) fn check_dependencies(
    current: WorkflowId,
    children: &BTreeSet<WorkflowId>,
    graph: &BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
    span: super::SourceSpan,
) -> Result<BTreeMap<WorkflowId, BTreeSet<WorkflowId>>, Diagnostic> {
    const MAX_DEPENDENCY_NODES: usize = 4_096;
    const MAX_DEPENDENCY_EDGES: usize = 8_192;
    let mut edges = 0usize;
    for children in graph.values() {
        edges = edges.saturating_add(children.len());
        if graph.len() > MAX_DEPENDENCY_NODES || edges > MAX_DEPENDENCY_EDGES {
            return Err(Diagnostic::at(
                "dependency.limit",
                "Dependency facts exceed the node or edge limit",
                "/steps",
                span,
            ));
        }
    }
    if !graph.contains_key(&current) {
        return Err(Diagnostic::at(
            "dependency.missing",
            "Dependency facts omit this workflow",
            "/workflow_id",
            span,
        ));
    }
    if graph[&current] != *children {
        return Err(Diagnostic::at(
            "dependency.mismatch",
            "Dependency facts do not match declared child workflows",
            "/steps",
            span,
        ));
    }
    let mut visited = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut closure = BTreeMap::new();
    let mut stack = vec![(current, false)];
    while let Some((id, exiting)) = stack.pop() {
        if exiting {
            active.remove(&id);
            continue;
        }
        if active.contains(&id) {
            return Err(Diagnostic::at(
                "dependency.cycle",
                "Recursive child workflow dependency",
                "/steps",
                span,
            ));
        }
        if !visited.insert(id) {
            continue;
        }
        let Some(children) = graph.get(&id) else {
            return Err(Diagnostic::at(
                "dependency.missing",
                "Child workflow dependency facts are missing",
                "/steps",
                span,
            ));
        };
        active.insert(id);
        closure.insert(id, children.clone());
        stack.push((id, true));
        stack.extend(children.iter().rev().map(|child| (*child, false)));
    }
    Ok(closure)
}
