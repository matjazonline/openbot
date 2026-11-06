//! Admission captures one closed identity graph; reconstruction never selects latest.
use super::*;
use crate::application::agent::{
    AgentPublicationScope, AgentRevisionSelection, DependencyManifest, MAX_AGENT_DEPENDENCIES,
    MAX_AGENT_DEPENDENCY_DEPTH, WorkflowVersionSelection,
};
use crate::entities::agent_capability::{AgentId, AgentRevisionId, DependencyManifestId};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

mod capture;
mod storage;
pub(super) use storage::{
    attach_child_on, execution_selection_on, persist_on, selected_on, workflow_bundle_on,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    Workflow(WorkflowId, VersionId),
    Agent(AgentId, AgentUseMode),
}

struct Frame {
    node: Node,
    children: Vec<Node>,
    next: usize,
    height: usize,
}

struct Closure {
    company: CompanyId,
    actor: WorkflowActor,
    resources: BTreeMap<crate::domain::workflow::ResourceName, RuntimeResourceId>,
    agents: BTreeMap<AgentId, AgentRevisionId>,
    workflows: BTreeMap<VersionId, WorkflowId>,
    deadline: Instant,
    edges: usize,
}

fn refused() -> AppError {
    AppError::BadRequest(
        "Agent dependency graph is unavailable, cyclic or exceeds its limits".into(),
    )
}

/// The caller already owns the company serialization lock. No network work runs here.
pub(super) async fn capture_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
) -> AppResult<DependencyManifest> {
    let definition = command.binding().bundle().compiled().graph().definition();
    let mut closure = Closure {
        company: command.company_id(),
        actor: command.actor(),
        resources: command.binding().resources().clone(),
        agents: BTreeMap::new(),
        workflows: BTreeMap::new(),
        deadline: Instant::now() + Duration::from_secs(5),
        edges: 0,
    };
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','5000ms',true)")
        .execute(&mut **tx).await?;
    tokio::time::timeout(
        Duration::from_secs(5),
        closure.walk(
            tx,
            Node::Workflow(definition.workflow_id, definition.version_id),
        ),
    )
    .await
    .map_err(|_| AppError::Timeout("Agent dependency capture exceeded five seconds".into()))??;
    let manifest = DependencyManifest {
        company_id: closure.company,
        root_run_id: command.proposed_run_id(),
        agents: closure
            .agents
            .into_iter()
            .map(|(agent_id, revision_id)| AgentRevisionSelection {
                agent_id,
                revision_id,
            })
            .collect(),
        workflows: closure
            .workflows
            .into_iter()
            .map(|(version_id, workflow_id)| WorkflowVersionSelection {
                workflow_id,
                version_id,
            })
            .collect(),
    };
    manifest.canonical_content()?;
    Ok(manifest)
}

impl Closure {
    fn within_bound(&self) -> AppResult<()> {
        if self.agents.len() + self.workflows.len() > MAX_AGENT_DEPENDENCIES {
            return Err(AppError::BadRequest(format!(
                "A run may select at most {MAX_AGENT_DEPENDENCIES} agent and workflow definitions"
            )));
        }
        if Instant::now() >= self.deadline {
            return Err(AppError::Timeout(
                "Agent dependency capture exceeded five seconds".into(),
            ));
        }
        Ok(())
    }

    async fn walk(&mut self, tx: &mut Transaction<'_, Postgres>, root: Node) -> AppResult<()> {
        let mut done = BTreeMap::<Node, usize>::new();
        let mut active = BTreeSet::new();
        let mut stack = vec![self.frame(tx, root).await?];
        active.insert(root);
        while let Some(frame) = stack.last_mut() {
            self.within_bound()?;
            if frame.next == frame.children.len() {
                let finished = stack.pop().ok_or_else(invalid)?;
                active.remove(&finished.node);
                done.insert(finished.node, finished.height);
                if let Some(parent) = stack.last_mut() {
                    parent.height = parent.height.max(finished.height + 1);
                }
                continue;
            }
            let child = frame.children[frame.next];
            frame.next += 1;
            if active.contains(&child) {
                return Err(AppError::BadRequest(
                    "Run dependencies contain an agent/workflow cycle".into(),
                ));
            }
            if let Some(height) = done.get(&child) {
                if stack.len() + height > MAX_AGENT_DEPENDENCY_DEPTH {
                    return Err(AppError::BadRequest(format!(
                        "Run dependency depth exceeds {MAX_AGENT_DEPENDENCY_DEPTH}"
                    )));
                }
                let frame = stack.last_mut().ok_or_else(invalid)?;
                frame.height = frame.height.max(height + 1);
                continue;
            }
            if stack.len() >= MAX_AGENT_DEPENDENCY_DEPTH {
                return Err(AppError::BadRequest(format!(
                    "Run dependency depth exceeds {MAX_AGENT_DEPENDENCY_DEPTH}"
                )));
            }
            if done.len() + active.len() >= MAX_AGENT_DEPENDENCIES * 2 {
                return Err(refused());
            }
            active.insert(child);
            stack.push(self.frame(tx, child).await?);
        }
        Ok(())
    }

    async fn frame(&mut self, tx: &mut Transaction<'_, Postgres>, node: Node) -> AppResult<Frame> {
        let children = match node {
            Node::Workflow(workflow, version) => self.workflow_edges(tx, workflow, version).await?,
            Node::Agent(agent, mode) => self.agent_edges(tx, agent, mode).await?,
        };
        self.edges = self.edges.saturating_add(children.len());
        // Every workflow has <=256 children and <=256 references with two use modes;
        // every agent has <=32 tools. Also bounds the work before deduplication.
        const MAX_EDGES: usize = MAX_AGENT_DEPENDENCIES * (MAX_AGENT_DEPENDENCIES * 3 + 32);
        if self.edges > MAX_EDGES {
            return Err(refused());
        }
        self.within_bound()?;
        Ok(Frame {
            node,
            children,
            next: 0,
            height: 1,
        })
    }
}
