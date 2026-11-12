//! Admission captures one closed identity graph; reconstruction never selects latest.
use super::*;
use crate::application::agent::dependency::{DependencyNode as Node, DependencyWalker};
use crate::application::agent::{
    AgentPublicationScope, AgentRevisionSelection, DependencyManifest, MAX_AGENT_DEPENDENCIES,
    WorkflowVersionSelection,
};
use crate::entities::agent_capability::{AgentId, AgentRevisionId, DependencyManifestId};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

mod capture;
mod storage;
#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
pub(super) use storage::{
    attach_child_on, execution_selection_on, execution_selection_with_locks_on, persist_on,
    selected_on, workflow_bundle_on,
};

struct Closure {
    company: CompanyId,
    actor: WorkflowActor,
    resources: BTreeMap<crate::domain::workflow::ResourceName, RuntimeResourceId>,
    agents: BTreeMap<AgentId, AgentRevisionId>,
    workflows: BTreeMap<VersionId, WorkflowId>,
    deadline: Instant,
}

fn refused() -> AppError {
    AppError::BadRequest(
        "Agent dependency graph is unavailable, cyclic or exceeds its limits".into(),
    )
}

fn check_selection_bound(count: usize) -> AppResult<()> {
    if count > MAX_AGENT_DEPENDENCIES {
        return Err(AppError::BadRequest(format!(
            "A run may select at most {MAX_AGENT_DEPENDENCIES} agent and workflow definitions"
        )));
    }
    Ok(())
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
        check_selection_bound(self.agents.len() + self.workflows.len())?;
        if Instant::now() >= self.deadline {
            return Err(AppError::Timeout(
                "Agent dependency capture exceeded five seconds".into(),
            ));
        }
        Ok(())
    }

    fn check_node_capacity(&self, node: Node) -> AppResult<()> {
        let selected = match node {
            Node::Workflow(_, version) => self.workflows.contains_key(&version),
            Node::Agent(agent, _) => self.agents.contains_key(&agent),
        };
        check_selection_bound(self.agents.len() + self.workflows.len() + usize::from(!selected))
    }

    async fn walk(&mut self, tx: &mut Transaction<'_, Postgres>, root: Node) -> AppResult<()> {
        let mut walker = DependencyWalker::new(root);
        while let Some(node) = walker.next()? {
            self.within_bound()?;
            let children = self.edges_on(tx, node).await?;
            walker.supply(node, children)?;
        }
        self.within_bound()
    }

    async fn edges_on(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        node: Node,
    ) -> AppResult<Vec<Node>> {
        // Refuse a new selection before map insertion or an active-pointer lookup.
        // Different use views of a selected agent do not consume another selection.
        self.check_node_capacity(node)?;
        let children = match node {
            Node::Workflow(workflow, version) => self.workflow_edges(tx, workflow, version).await?,
            Node::Agent(agent, mode) => self.agent_edges(tx, agent, mode).await?,
        };
        self.within_bound()?;
        Ok(children)
    }
}
