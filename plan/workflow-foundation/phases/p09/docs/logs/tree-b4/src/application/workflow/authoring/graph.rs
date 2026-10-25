use crate::domain::workflow::{Routes, StepId, TransitionTarget, WorkflowDefinition};
use std::collections::{BTreeMap, VecDeque};

/// The node every `$end` route points at.
pub const END_NODE: &str = "$end";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    /// The step id, or [`END_NODE`].
    pub id: String,
    /// The step type (`agent.run`, …); empty for the end node.
    pub kind: String,
    pub entry: bool,
    /// Distance from the entry along the longest route path, so every edge points down.
    pub layer: usize,
    /// Position within the layer, in first-reached order.
    pub slot: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphEdgeKind {
    Success,
    Choice,
    /// `final_error`: taken only after retries are exhausted.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    /// `success`, the choice name, or `error`.
    pub label: String,
    pub kind: GraphEdgeKind,
}

/// The layered drawing of one validated definition: the same steps and routes the compiler
/// froze, never a second parse of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Layers including the end node's.
    pub layers: usize,
    /// The widest layer's node count.
    pub width: usize,
}

impl WorkflowGraph {
    pub fn node(&self, id: &str) -> Option<&GraphNode> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

fn target_id(target: &TransitionTarget) -> String {
    match target {
        TransitionTarget::Step(step) => step.as_str().to_owned(),
        TransitionTarget::End => END_NODE.to_owned(),
    }
}

fn edges_of(id: &StepId, definition: &WorkflowDefinition) -> Vec<GraphEdge> {
    let step = &definition.steps[id];
    let mut edges = match &step.routes {
        Routes::Success(target) => vec![GraphEdge {
            from: id.as_str().to_owned(),
            to: target_id(target),
            label: "success".to_owned(),
            kind: GraphEdgeKind::Success,
        }],
        Routes::Choices(choices) => choices
            .iter()
            .map(|(choice, target)| GraphEdge {
                from: id.as_str().to_owned(),
                to: target_id(target),
                label: choice.as_str().to_owned(),
                kind: GraphEdgeKind::Choice,
            })
            .collect(),
    };
    if let Some(target) = &step.final_error {
        edges.push(GraphEdge {
            from: id.as_str().to_owned(),
            to: target_id(target),
            label: "error".to_owned(),
            kind: GraphEdgeKind::Error,
        });
    }
    edges
}

/// Lay a validated (acyclic, fully reachable) definition out in layers: a node's layer is its
/// longest route distance from the entry, and the end node sits below every step.
pub fn workflow_graph(definition: &WorkflowDefinition) -> WorkflowGraph {
    let edges: Vec<GraphEdge> = definition
        .steps
        .keys()
        .flat_map(|id| edges_of(id, definition))
        .collect();
    // First-reached order from the entry, breadth first: the order nodes take within a layer.
    let mut order: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut queue = VecDeque::from([definition.entry.as_str().to_owned()]);
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for edge in edges.iter().filter(|edge| edge.from == id) {
            queue.push_back(edge.to.clone());
        }
        order.push(id);
    }
    // Longest path by relaxation in topological order (Kahn), which a DAG always has.
    let mut indegree: BTreeMap<&str, usize> = order.iter().map(|id| (id.as_str(), 0)).collect();
    for edge in &edges {
        *indegree.entry(edge.to.as_str()).or_default() += 1;
    }
    let mut layer: BTreeMap<&str, usize> = BTreeMap::new();
    let mut ready: VecDeque<&str> = order
        .iter()
        .map(String::as_str)
        .filter(|id| indegree.get(id) == Some(&0))
        .collect();
    while let Some(id) = ready.pop_front() {
        let depth = *layer.entry(id).or_insert(0);
        for edge in edges.iter().filter(|edge| edge.from == id) {
            let next = layer.entry(edge.to.as_str()).or_insert(0);
            *next = (*next).max(depth + 1);
            let remaining = indegree.get_mut(edge.to.as_str()).expect("edge target");
            *remaining -= 1;
            if *remaining == 0 {
                ready.push_back(edge.to.as_str());
            }
        }
    }
    let bottom = order
        .iter()
        .filter(|id| id.as_str() != END_NODE)
        .map(|id| layer.get(id.as_str()).copied().unwrap_or(0))
        .max()
        .unwrap_or(0);
    let mut slots: BTreeMap<usize, usize> = BTreeMap::new();
    let nodes: Vec<GraphNode> = order
        .iter()
        .map(|id| {
            let depth = if id == END_NODE {
                bottom + 1
            } else {
                layer.get(id.as_str()).copied().unwrap_or(0)
            };
            let slot = slots.entry(depth).or_default();
            let node = GraphNode {
                id: id.clone(),
                kind: StepId::parse(id)
                    .ok()
                    .and_then(|step| definition.steps.get(&step))
                    .map(|step| step.step_type.as_str().to_owned())
                    .unwrap_or_default(),
                entry: id.as_str() == definition.entry.as_str(),
                layer: depth,
                slot: *slot,
            };
            *slot += 1;
            node
        })
        .collect();
    WorkflowGraph {
        layers: nodes.iter().map(|node| node.layer + 1).max().unwrap_or(0),
        width: slots.values().copied().max().unwrap_or(0),
        nodes,
        edges,
    }
}
