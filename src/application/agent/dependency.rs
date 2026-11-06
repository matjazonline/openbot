//! Pure iterative validation shared by native admission and sealed samples.
use super::{MAX_AGENT_DEPENDENCIES, MAX_AGENT_DEPENDENCY_DEPTH};
use crate::app_error::{AppError, AppResult};
use crate::application::workflow::publication::AgentUseMode;
use crate::domain::workflow::{VersionId, WorkflowId};
use crate::entities::agent_capability::AgentId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DependencyNode {
    Workflow(WorkflowId, VersionId),
    Agent(AgentId, AgentUseMode),
}
struct Frame {
    node: DependencyNode,
    children: Vec<DependencyNode>,
    next: usize,
    height: usize,
}
pub(crate) struct DependencyWalker {
    done: BTreeMap<DependencyNode, usize>,
    active: BTreeSet<DependencyNode>,
    stack: Vec<Frame>,
    pending: Option<DependencyNode>,
    requested: bool,
    edges: usize,
}
fn refused(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
impl DependencyWalker {
    pub(crate) fn new(root: DependencyNode) -> Self {
        Self {
            done: BTreeMap::new(),
            active: BTreeSet::new(),
            stack: vec![],
            pending: Some(root),
            requested: false,
            edges: 0,
        }
    }
    pub(crate) fn next(&mut self) -> AppResult<Option<DependencyNode>> {
        if self.requested {
            return Err(refused("Unresolved dependency traversal node"));
        }
        loop {
            if let Some(node) = self.pending {
                self.requested = true;
                return Ok(Some(node));
            }
            let Some(frame) = self.stack.last_mut() else {
                return Ok(None);
            };
            if frame.next == frame.children.len() {
                let finished = self
                    .stack
                    .pop()
                    .ok_or_else(|| refused("Invalid dependency traversal"))?;
                self.active.remove(&finished.node);
                self.done.insert(finished.node, finished.height);
                if let Some(parent) = self.stack.last_mut() {
                    parent.height = parent.height.max(finished.height + 1);
                }
                continue;
            }
            let child = frame.children[frame.next];
            frame.next += 1;
            if self.active.contains(&child) {
                return Err(refused("Run dependencies contain an agent/workflow cycle"));
            }
            if let Some(height) = self.done.get(&child) {
                if self.stack.len() + height > MAX_AGENT_DEPENDENCY_DEPTH {
                    return Err(refused("Run dependency depth exceeds 32"));
                }
                let parent = self
                    .stack
                    .last_mut()
                    .ok_or_else(|| refused("Invalid dependency traversal"))?;
                parent.height = parent.height.max(height + 1);
                continue;
            }
            if self.stack.len() >= MAX_AGENT_DEPENDENCY_DEPTH {
                return Err(refused("Run dependency depth exceeds 32"));
            }
            if self.done.len() + self.active.len() >= MAX_AGENT_DEPENDENCIES * 2 {
                return Err(refused("Dependency traversal exceeds its view bound"));
            }
            self.pending = Some(child);
        }
    }
    pub(crate) fn supply(
        &mut self,
        node: DependencyNode,
        children: Vec<DependencyNode>,
    ) -> AppResult<()> {
        if !self.requested || self.pending != Some(node) {
            return Err(refused("Unexpected dependency traversal result"));
        }
        self.edges = self.edges.saturating_add(children.len());
        const MAX_EDGES: usize = MAX_AGENT_DEPENDENCIES * (MAX_AGENT_DEPENDENCIES * 3 + 32);
        if self.edges > MAX_EDGES {
            return Err(refused("Dependency traversal exceeds its edge bound"));
        }
        self.pending = None;
        self.requested = false;
        self.active.insert(node);
        self.stack.push(Frame {
            node,
            children,
            next: 0,
            height: 1,
        });
        Ok(())
    }
}

pub(crate) fn workflow_edges(
    bundle: &crate::application::workflow::publication::PublishedBundle,
) -> Vec<DependencyNode> {
    let mut edges = BTreeSet::new();
    for child in bundle.children().values() {
        let definition = child.compiled().graph().definition();
        edges.insert(DependencyNode::Workflow(
            definition.workflow_id,
            definition.version_id,
        ));
    }
    for reference in &bundle.snapshots().agents {
        for usage in &reference.uses {
            edges.insert(DependencyNode::Agent(reference.agent_id, usage.mode));
        }
    }
    edges.into_iter().collect()
}
pub(crate) fn agent_edges(
    agent: &super::ResolvedAgentDefinition,
    mode: AgentUseMode,
) -> Vec<DependencyNode> {
    if mode == AgentUseMode::ModelOnly {
        return vec![];
    }
    agent
        .tools
        .iter()
        .filter_map(|tool| match tool.binding.source {
            crate::entities::agent_capability::AgentBindingSource::Workflow {
                workflow,
                version,
                ..
            } => Some(DependencyNode::Workflow(workflow, version)),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
