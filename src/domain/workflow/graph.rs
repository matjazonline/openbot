use super::context::{MAX_OUTPUT_BYTES, MAX_WORK_NODES};
use super::definition::MAX_EXECUTION_STEPS;
use super::{ContextLimits, Routes, StepId, TransitionTarget, WorkflowDefinition};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

pub const MAX_GRAPH_NODES: usize = 1024;
pub const MAX_GRAPH_EDGES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GraphError {
    #[error("workflow has no steps")]
    Empty,
    #[error("workflow has {actual} steps, limit {limit}")]
    TooManySteps { actual: usize, limit: usize },
    #[error("workflow has more than {limit} routes")]
    TooManyRoutes { limit: usize },
    #[error("execution limit {field} must be 1..={maximum}, got {actual}")]
    InvalidExecutionLimit {
        field: &'static str,
        actual: u64,
        maximum: u64,
    },
    #[error("entry step {0} is missing")]
    MissingEntry(StepId),
    #[error("step {step} route {route} targets missing step {target}")]
    MissingTarget {
        step: StepId,
        route: String,
        target: StepId,
    },
    #[error("step {step} duplicates choice route {choice}")]
    DuplicateChoice {
        step: StepId,
        choice: super::ChoiceName,
    },
    #[error("step {step} has no choices")]
    EmptyChoices { step: StepId },
    #[error("workflow contains a cycle involving {0}")]
    Cycle(StepId),
    #[error("step {0} is unreachable from entry")]
    Unreachable(StepId),
}

/// Only constructed after every target, cycle and reachability check succeeds.
#[derive(Debug, Clone)]
pub struct ValidatedWorkflow {
    definition: WorkflowDefinition,
}

impl ValidatedWorkflow {
    pub fn definition(&self) -> &WorkflowDefinition {
        &self.definition
    }

    /// The runtime must use these limits for each binding and debit max_steps
    /// from the validated definition across the whole run, including repeats.
    pub fn context_limits(&self) -> ContextLimits {
        ContextLimits {
            output_bytes: self.definition.limits.max_context_bytes,
            work_nodes: MAX_WORK_NODES,
        }
    }
}

fn validate_limits(definition: &WorkflowDefinition) -> Result<(), GraphError> {
    let steps = definition.limits.max_steps;
    if steps == 0 || steps > MAX_EXECUTION_STEPS {
        return Err(GraphError::InvalidExecutionLimit {
            field: "max_steps",
            actual: u64::from(steps),
            maximum: u64::from(MAX_EXECUTION_STEPS),
        });
    }
    let bytes = definition.limits.max_context_bytes;
    if bytes == 0 || bytes > MAX_OUTPUT_BYTES {
        return Err(GraphError::InvalidExecutionLimit {
            field: "max_context_bytes",
            actual: bytes as u64,
            maximum: MAX_OUTPUT_BYTES as u64,
        });
    }
    Ok(())
}

fn preflight_routes(definition: &WorkflowDefinition) -> Result<(), GraphError> {
    let mut total = 0usize;
    for (id, step) in &definition.steps {
        let count = match &step.routes {
            Routes::Success(_) => 1,
            Routes::Choices(choices) if choices.is_empty() => {
                return Err(GraphError::EmptyChoices { step: id.clone() });
            }
            Routes::Choices(choices) => choices.len(),
        };
        total = total
            .saturating_add(count)
            .saturating_add(usize::from(step.final_error.is_some()));
        if total > MAX_GRAPH_EDGES {
            return Err(GraphError::TooManyRoutes {
                limit: MAX_GRAPH_EDGES,
            });
        }
    }
    Ok(())
}

fn routes(step: &super::StepDefinition) -> Vec<(String, &TransitionTarget)> {
    let mut result = match &step.routes {
        Routes::Success(target) => vec![("success".to_owned(), target)],
        Routes::Choices(choices) => choices
            .iter()
            .map(|(name, target)| (name.to_string(), target))
            .collect(),
    };
    if let Some(target) = &step.final_error {
        result.push(("final_error".to_owned(), target));
    }
    result.sort_by(|left, right| left.0.cmp(&right.0));
    result
}

fn validate_acyclic(
    adjacency: &BTreeMap<StepId, BTreeSet<StepId>>,
    mut indegree: BTreeMap<StepId, usize>,
) -> Result<(), GraphError> {
    let mut zero: BTreeSet<_> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut visited = 0;
    while let Some(id) = zero.pop_first() {
        visited += 1;
        for target in &adjacency[&id] {
            let degree = indegree.get_mut(target).expect("known target");
            *degree -= 1;
            if *degree == 0 {
                zero.insert(target.clone());
            }
        }
    }
    if visited != adjacency.len() {
        return Err(GraphError::Cycle(
            indegree
                .into_iter()
                .find(|(_, degree)| *degree > 0)
                .expect("cycle node")
                .0,
        ));
    }
    Ok(())
}

fn validate_reachable(
    definition: &WorkflowDefinition,
    adjacency: &BTreeMap<StepId, BTreeSet<StepId>>,
) -> Result<(), GraphError> {
    let mut reachable = BTreeSet::new();
    let mut queue = VecDeque::from([definition.entry.clone()]);
    while let Some(id) = queue.pop_front() {
        if reachable.insert(id.clone()) {
            queue.extend(adjacency[&id].iter().cloned());
        }
    }
    if let Some(id) = definition.steps.keys().find(|id| !reachable.contains(*id)) {
        return Err(GraphError::Unreachable(id.clone()));
    }
    Ok(())
}

pub fn validate(definition: WorkflowDefinition) -> Result<ValidatedWorkflow, GraphError> {
    let len = definition.steps.len();
    if len == 0 {
        return Err(GraphError::Empty);
    }
    if len > MAX_GRAPH_NODES {
        return Err(GraphError::TooManySteps {
            actual: len,
            limit: MAX_GRAPH_NODES,
        });
    }
    if !definition.steps.contains_key(&definition.entry) {
        return Err(GraphError::MissingEntry(definition.entry.clone()));
    }
    validate_limits(&definition)?;
    preflight_routes(&definition)?;
    let mut adjacency: BTreeMap<StepId, BTreeSet<StepId>> = definition
        .steps
        .keys()
        .cloned()
        .map(|id| (id, BTreeSet::new()))
        .collect();
    let mut indegree: BTreeMap<StepId, usize> =
        definition.steps.keys().cloned().map(|id| (id, 0)).collect();
    for (id, step) in &definition.steps {
        if let Routes::Choices(choices) = &step.routes {
            let mut seen = BTreeSet::new();
            for (choice, _) in choices {
                if !seen.insert(choice) {
                    return Err(GraphError::DuplicateChoice {
                        step: id.clone(),
                        choice: choice.clone(),
                    });
                }
            }
        }
        for (route, target) in routes(step) {
            let TransitionTarget::Step(target) = target else {
                continue;
            };
            if !definition.steps.contains_key(target) {
                return Err(GraphError::MissingTarget {
                    step: id.clone(),
                    route,
                    target: target.clone(),
                });
            }
            let newly_added = adjacency
                .get_mut(id)
                .expect("known step")
                .insert(target.clone());
            if newly_added {
                *indegree.get_mut(target).expect("known target") += 1;
            }
        }
    }
    validate_acyclic(&adjacency, indegree)?;
    validate_reachable(&definition, &adjacency)?;
    Ok(ValidatedWorkflow { definition })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::workflow::{
        ChoiceName, ExecutionLimits, ResourceName, ResourceRequirement, RouteSelection,
        StepDefinition, TypeName, VersionId, WorkflowId, select_route,
    };
    use uuid::Uuid;

    fn id(s: &str) -> StepId {
        StepId::parse(s).unwrap()
    }
    fn step(target: TransitionTarget) -> StepDefinition {
        StepDefinition {
            step_type: TypeName::parse("agent.run").unwrap(),
            inputs: BTreeMap::new(),
            routes: Routes::Success(target),
            final_error: None,
        }
    }
    fn definition(items: Vec<(&str, StepDefinition)>) -> WorkflowDefinition {
        WorkflowDefinition {
            format_version: 1,
            workflow_id: WorkflowId::new(Uuid::nil()),
            version_id: VersionId::new(Uuid::nil()),
            input_schema: None,
            parameter_schema: None,
            output_schema: None,
            resources: vec![],
            entry: id("a"),
            steps: items
                .into_iter()
                .map(|(name, step)| (id(name), step))
                .collect(),
            limits: ExecutionLimits {
                root_budget: Default::default(),
                max_steps: 10,
                max_context_bytes: 1024,
            },
        }
    }

    #[test]
    fn sequential_branch_and_routes() {
        let mut branch = step(TransitionTarget::End);
        branch.routes = Routes::Choices(vec![
            (
                ChoiceName::parse("yes").unwrap(),
                TransitionTarget::Step(id("b")),
            ),
            (ChoiceName::parse("no").unwrap(), TransitionTarget::End),
        ]);
        branch.final_error = Some(TransitionTarget::End);
        let graph = validate(definition(vec![
            ("a", branch),
            ("b", step(TransitionTarget::End)),
        ]))
        .unwrap();
        assert_eq!(
            select_route(
                &graph,
                &id("a"),
                &RouteSelection::Choice(ChoiceName::parse("yes").unwrap())
            )
            .unwrap(),
            &TransitionTarget::Step(id("b"))
        );
        assert_eq!(
            select_route(&graph, &id("a"), &RouteSelection::FinalError).unwrap(),
            &TransitionTarget::End
        );
        assert_eq!(
            select_route(
                &graph,
                &id("a"),
                &RouteSelection::Choice(ChoiceName::parse("no").unwrap())
            )
            .unwrap(),
            &TransitionTarget::End
        );
        assert_eq!(
            select_route(&graph, &id("b"), &RouteSelection::Success).unwrap(),
            &TransitionTarget::End
        );
        assert!(matches!(
            select_route(&graph, &id("unknown"), &RouteSelection::Success),
            Err(super::super::RouteError::UnknownStep(_))
        ));
        assert!(
            select_route(
                &graph,
                &id("a"),
                &RouteSelection::Choice(ChoiceName::parse("unknown").unwrap())
            )
            .is_err()
        );
        assert!(select_route(&graph, &id("a"), &RouteSelection::Success).is_err());
        assert!(select_route(&graph, &id("b"), &RouteSelection::FinalError).is_err());
    }

    #[test]
    fn bad_graphs_are_rejected() {
        assert!(matches!(
            validate(definition(vec![])),
            Err(GraphError::Empty)
        ));
        assert!(matches!(
            validate(definition(vec![("b", step(TransitionTarget::End))])),
            Err(GraphError::MissingEntry(_))
        ));
        let missing = definition(vec![("a", step(TransitionTarget::Step(id("missing"))))]);
        assert!(matches!(
            validate(missing),
            Err(GraphError::MissingTarget { .. })
        ));
        let self_cycle = definition(vec![("a", step(TransitionTarget::Step(id("a"))))]);
        assert!(matches!(validate(self_cycle), Err(GraphError::Cycle(_))));
        let disconnected_cycle = definition(vec![
            ("a", step(TransitionTarget::End)),
            ("b", step(TransitionTarget::Step(id("c")))),
            ("c", step(TransitionTarget::Step(id("b")))),
        ]);
        assert!(matches!(
            validate(disconnected_cycle),
            Err(GraphError::Cycle(_))
        ));
        assert!(matches!(
            validate(definition(vec![
                ("a", step(TransitionTarget::End)),
                ("b", step(TransitionTarget::End))
            ])),
            Err(GraphError::Unreachable(_))
        ));
        let mut empty_choices = step(TransitionTarget::End);
        empty_choices.routes = Routes::Choices(vec![]);
        assert!(matches!(
            validate(definition(vec![("a", empty_choices)])),
            Err(GraphError::EmptyChoices { .. })
        ));
    }

    #[test]
    fn large_chain_and_node_ceiling() {
        let names: Vec<_> = (0..MAX_GRAPH_NODES).map(|n| format!("s{n}")).collect();
        let items: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                (
                    name.as_str(),
                    step(names.get(i + 1).map_or(TransitionTarget::End, |next| {
                        TransitionTarget::Step(id(next))
                    })),
                )
            })
            .collect();
        let mut graph = definition(items);
        graph.entry = id("s0");
        assert!(validate(graph.clone()).is_ok());
        graph.steps.insert(id("extra"), step(TransitionTarget::End));
        assert!(matches!(
            validate(graph),
            Err(GraphError::TooManySteps { .. })
        ));
    }

    #[test]
    fn duplicate_choices_and_deterministic_diagnostics() {
        let choice = ChoiceName::parse("same").unwrap();
        let mut branch = step(TransitionTarget::End);
        branch.routes = Routes::Choices(vec![
            (choice.clone(), TransitionTarget::End),
            (choice.clone(), TransitionTarget::End),
        ]);
        assert!(matches!(
            validate(definition(vec![("a", branch)])),
            Err(GraphError::DuplicateChoice { .. })
        ));

        let first = ChoiceName::parse("first").unwrap();
        let second = ChoiceName::parse("second").unwrap();
        let routes = vec![
            (second, TransitionTarget::Step(id("missing_b"))),
            (first, TransitionTarget::Step(id("missing_a"))),
        ];
        let mut branch = step(TransitionTarget::End);
        branch.routes = Routes::Choices(routes.clone());
        let error = validate(definition(vec![("a", branch.clone())])).unwrap_err();
        branch.routes = Routes::Choices(routes.into_iter().rev().collect());
        assert_eq!(
            validate(definition(vec![("a", branch)])).unwrap_err(),
            error
        );
    }

    #[test]
    fn exact_edge_ceiling() {
        let names: Vec<_> = (0..MAX_GRAPH_NODES).map(|n| format!("s{n}")).collect();
        let mut items = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let target = names.get(index + 1).map_or(TransitionTarget::End, |next| {
                TransitionTarget::Step(id(next))
            });
            let mut node = step(TransitionTarget::End);
            node.routes = Routes::Choices(
                (0..8)
                    .map(|n| (ChoiceName::parse(format!("c{n}")).unwrap(), target.clone()))
                    .collect(),
            );
            items.push((name.as_str(), node));
        }
        let mut graph = definition(items);
        graph.entry = id("s0");
        assert!(validate(graph.clone()).is_ok());
        let extra = ChoiceName::parse("extra").unwrap();
        if let Routes::Choices(choices) = &mut graph.steps.get_mut(&id("s0")).unwrap().routes {
            choices.push((extra, TransitionTarget::End));
        }
        assert!(matches!(
            validate(graph),
            Err(GraphError::TooManyRoutes { .. })
        ));
    }

    #[test]
    fn execution_limits_are_positive_and_platform_capped() {
        let mut graph = definition(vec![("a", step(TransitionTarget::End))]);
        graph.limits.max_steps = 0;
        assert!(matches!(
            validate(graph.clone()),
            Err(GraphError::InvalidExecutionLimit {
                field: "max_steps",
                ..
            })
        ));
        graph.limits.max_steps = MAX_EXECUTION_STEPS + 1;
        assert!(matches!(
            validate(graph.clone()),
            Err(GraphError::InvalidExecutionLimit {
                field: "max_steps",
                ..
            })
        ));
        graph.limits.max_steps = MAX_EXECUTION_STEPS;
        graph.limits.max_context_bytes = 0;
        assert!(matches!(
            validate(graph.clone()),
            Err(GraphError::InvalidExecutionLimit {
                field: "max_context_bytes",
                ..
            })
        ));
        graph.limits.max_context_bytes = MAX_OUTPUT_BYTES + 1;
        assert!(matches!(
            validate(graph.clone()),
            Err(GraphError::InvalidExecutionLimit {
                field: "max_context_bytes",
                ..
            })
        ));
        graph.limits.max_context_bytes = MAX_OUTPUT_BYTES;
        let validated = validate(graph).unwrap();
        assert_eq!(validated.context_limits().output_bytes, MAX_OUTPUT_BYTES);
        assert_eq!(validated.definition().limits.max_steps, MAX_EXECUTION_STEPS);
    }

    #[test]
    fn output_schema_and_typed_resource_requirement_survive_validation() {
        let mut graph = definition(vec![("a", step(TransitionTarget::End))]);
        graph.output_schema = Some(serde_json::json!({"type":"object"}));
        graph.resources.push(ResourceRequirement {
            slot: ResourceName::parse("reviewer").unwrap(),
            kind: TypeName::parse("agent").unwrap(),
            contract: Some(TypeName::parse("review.v1").unwrap()),
        });
        let validated = validate(graph).unwrap();
        assert!(validated.definition().output_schema.is_some());
        assert_eq!(
            validated.definition().resources[0].slot.as_str(),
            "reviewer"
        );
        assert_eq!(
            validated.definition().resources[0]
                .contract
                .as_ref()
                .unwrap()
                .as_str(),
            "review.v1"
        );
    }

    #[test]
    fn oversized_route_list_fails_before_duplicate_scanning() {
        let mut node = step(TransitionTarget::End);
        node.routes = Routes::Choices(
            (0..MAX_GRAPH_EDGES + 1)
                .map(|_| {
                    (
                        ChoiceName::parse("duplicate").unwrap(),
                        TransitionTarget::End,
                    )
                })
                .collect(),
        );
        assert!(matches!(
            validate(definition(vec![("a", node)])),
            Err(GraphError::TooManyRoutes { .. })
        ));
    }
}
