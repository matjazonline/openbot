//! A sample owns its complete ephemeral definitions before any scripted step executes.
use super::*;
use crate::application::agent::dependency::{
    DependencyNode, DependencyWalker, agent_edges, workflow_edges,
};
use crate::application::agent::{
    MAX_AGENT_DEPENDENCIES, MAX_AGENT_MANIFEST_BYTES, ResolvedAgentDefinition,
};
use crate::domain::workflow::{CompanyId, VersionId};
use crate::entities::agent_capability::AgentId;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SampleConfigurationHash(String);
impl SampleConfigurationHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct SampleAgentSelection {
    pub agent_id: AgentId,
    pub sample_hash: SampleConfigurationHash,
}
#[derive(Debug, Clone, Serialize)]
pub struct SampleWorkflowSelection {
    pub workflow_id: WorkflowId,
    pub version_id: VersionId,
    pub content_hash: super::super::publication::ContentHash,
}

#[derive(Serialize)]
struct ConfigurationIdentity<'a> {
    kind: &'static str,
    company: CompanyId,
    agents: &'a [SampleAgentSelection],
    workflows: &'a [SampleWorkflowSelection],
    response_contract: &'a Option<super::super::channel_reply::ChannelReplyContract>,
    response_approval_policy: &'a Option<super::super::channel_reply::ReplyApprovalPolicy>,
}

pub struct PreparedSample {
    pub(super) root: Arc<PublishedBundle>,
    pub(super) request: SampleRequest,
    pub(super) agents: BTreeMap<AgentId, ResolvedAgentDefinition>,
    pub(super) workflows: BTreeMap<VersionId, Arc<PublishedBundle>>,
    pub(super) configuration_hash: SampleConfigurationHash,
    pub(super) agent_selections: Vec<SampleAgentSelection>,
    pub(super) workflow_selections: Vec<SampleWorkflowSelection>,
}
impl PreparedSample {
    pub fn new(
        root: Arc<PublishedBundle>,
        mut request: SampleRequest,
        extra: Vec<Arc<PublishedBundle>>,
    ) -> AppResult<Self> {
        let bytes = serde_json::to_vec(&request).map_err(|_| refused("Invalid sample request"))?;
        if bytes.len() > MAX_SAMPLE_BYTES {
            return Err(refused(
                "Sample input, scripts and agent definitions exceed 256 KiB",
            ));
        }
        if request.scripts.len() > MAX_SAMPLE_STEPS as usize {
            return Err(refused("Sample exceeds 128 script keys"));
        }
        if extra.len() > MAX_AGENT_DEPENDENCIES {
            return Err(refused("Too many sample workflows"));
        }
        validate_response(&root, &request)?;
        let mut agents = BTreeMap::new();
        for agent in std::mem::take(&mut request.agents) {
            agent.validate()?;
            if agent.company_id != root.company_id()
                || agents.len() >= MAX_AGENT_DEPENDENCIES
                || agents.insert(agent.agent_id, agent).is_some()
            {
                return Err(refused("Invalid sample agent definitions"));
            }
        }
        let workflows = collect_workflows(root.clone(), extra, agents.len())?;
        validate_closure(&root, &agents, &workflows)?;
        let agent_selections = agents
            .values()
            .map(|agent| {
                Ok(SampleAgentSelection {
                    agent_id: agent.agent_id,
                    sample_hash: SampleConfigurationHash(format!(
                        "{:x}",
                        Sha256::digest(agent.canonical_content()?)
                    )),
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let workflow_selections: Vec<_> = workflows
            .values()
            .map(|bundle| {
                let d = bundle.compiled().graph().definition();
                SampleWorkflowSelection {
                    workflow_id: d.workflow_id,
                    version_id: d.version_id,
                    content_hash: bundle.content_hash().clone(),
                }
            })
            .collect();
        let identity = serde_json::to_value(ConfigurationIdentity {
            kind: "sample-only-v2",
            company: root.company_id(),
            agents: &agent_selections,
            workflows: &workflow_selections,
            response_contract: &request.response_contract,
            response_approval_policy: &request.response_approval_policy,
        })
        .map_err(|_| refused("Invalid sample identity"))?;
        let configuration_hash = SampleConfigurationHash(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&identity).map_err(|_| refused("Invalid sample identity"))?
            )
        ));
        Ok(Self {
            root,
            request,
            agents,
            workflows,
            configuration_hash,
            agent_selections,
            workflow_selections,
        })
    }
}

fn validate_response(root: &PublishedBundle, request: &SampleRequest) -> AppResult<()> {
    match &request.response_contract {
        Some(contract) => contract.validate(
            root.compiled()
                .graph()
                .definition()
                .output_schema
                .as_ref()
                .unwrap_or(&Value::Bool(true)),
        ),
        None if request.response_approval_policy.is_some() => Err(refused(
            "A sample final approval policy needs an explicit response contract",
        )),
        None => Ok(()),
    }
}
fn collect_workflows(
    root: Arc<PublishedBundle>,
    extra: Vec<Arc<PublishedBundle>>,
    agent_count: usize,
) -> AppResult<BTreeMap<VersionId, Arc<PublishedBundle>>> {
    let company = root.company_id();
    let mut pending = extra;
    pending.push(root);
    let mut workflows: BTreeMap<VersionId, Arc<PublishedBundle>> = BTreeMap::new();
    let mut bytes = 0usize;
    while let Some(bundle) = pending.pop() {
        let d = bundle.compiled().graph().definition();
        if bundle.company_id() != company {
            return Err(refused("Sample workflow belongs to another company"));
        }
        if let Some(prior) = workflows.get(&d.version_id) {
            if prior.content_hash() != bundle.content_hash()
                || prior.compiled().graph().definition().workflow_id != d.workflow_id
            {
                return Err(refused("Conflicting sample workflow identity"));
            }
            continue;
        }
        if workflows.len() + agent_count >= MAX_AGENT_DEPENDENCIES {
            return Err(refused("Too many sample dependency selections"));
        }
        bytes = bytes.saturating_add(super::super::publication::store_bundle(&bundle)?.len());
        if bytes > MAX_AGENT_MANIFEST_BYTES {
            return Err(refused("Sample workflow definitions exceed 4 MiB"));
        }
        pending.extend(bundle.children().values().cloned());
        workflows.insert(d.version_id, bundle);
    }
    Ok(workflows)
}
fn validate_closure(
    root: &PublishedBundle,
    agents: &BTreeMap<AgentId, ResolvedAgentDefinition>,
    workflows: &BTreeMap<VersionId, Arc<PublishedBundle>>,
) -> AppResult<()> {
    let d = root.compiled().graph().definition();
    let mut walk = DependencyWalker::new(DependencyNode::Workflow(d.workflow_id, d.version_id));
    let mut used_agents = BTreeSet::new();
    let mut used_workflows = BTreeSet::new();
    while let Some(node) = walk.next()? {
        let children = match node {
            DependencyNode::Workflow(workflow, version) => {
                let bundle = workflow_from(workflows, root.company_id(), workflow, version)?;
                used_workflows.insert(version);
                workflow_edges(&bundle)
            }
            DependencyNode::Agent(agent, mode) => {
                let definition = agents
                    .get(&agent)
                    .ok_or_else(|| refused("Sample agent definition is missing"))?;
                used_agents.insert(agent);
                agent_edges(definition, mode)
            }
        };
        walk.supply(node, children)?;
    }
    if used_agents.len() != agents.len() || used_workflows.len() != workflows.len() {
        return Err(refused("Sample contains undeclared dependency definitions"));
    }
    Ok(())
}
pub(super) fn workflow_from(
    workflows: &BTreeMap<VersionId, Arc<PublishedBundle>>,
    company: CompanyId,
    workflow: WorkflowId,
    version: VersionId,
) -> AppResult<Arc<PublishedBundle>> {
    let bundle = workflows
        .get(&version)
        .ok_or_else(|| refused("Sample workflow is absent from the sealed closure"))?;
    if bundle.company_id() != company
        || bundle.compiled().graph().definition().workflow_id != workflow
    {
        return Err(refused("Sample workflow identity does not match its pin"));
    }
    Ok(bundle.clone())
}
