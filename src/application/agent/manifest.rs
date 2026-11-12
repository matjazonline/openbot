use super::definition::invalid;
use crate::app_error::AppResult;
use crate::domain::entities::agent_capability::{AgentId, AgentRevisionId, DependencyManifestId};
use crate::domain::workflow::StepId;
use crate::domain::workflow::{CompanyId, RunId, VersionId, WorkflowId};
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::entities::transport::PrincipalId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_AGENT_DEPENDENCIES: usize = 256;
pub const MAX_AGENT_DEPENDENCY_DEPTH: usize = 32;
pub const MAX_AGENT_MANIFEST_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRevisionSelection {
    pub agent_id: AgentId,
    pub revision_id: AgentRevisionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVersionSelection {
    pub workflow_id: WorkflowId,
    pub version_id: VersionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RolePrincipalSelection {
    pub principal_id: PrincipalId,
    pub agent_id: Option<AgentId>,
    pub revision_id: Option<AgentRevisionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleResolution {
    pub version_id: VersionId,
    pub step: StepId,
    pub role: DepartmentRole,
    pub department_id: DepartmentId,
    pub principals: Vec<RolePrincipalSelection>,
}

/// Durable identity selections only. Closure assembly is the admission transaction's job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyManifest {
    pub company_id: CompanyId,
    pub root_run_id: RunId,
    /// The original desk context, distinct from a selected ancestor role department.
    #[serde(deserialize_with = "Option::deserialize")]
    pub role_context_department_id: Option<DepartmentId>,
    pub agents: Vec<AgentRevisionSelection>,
    pub workflows: Vec<WorkflowVersionSelection>,
    pub roles: Vec<RoleResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunDependencyManifest {
    pub company_id: CompanyId,
    pub run_id: RunId,
    pub root_run_id: RunId,
    pub manifest_id: DependencyManifestId,
}

impl DependencyManifest {
    pub fn decode(bytes: &[u8]) -> AppResult<Self> {
        if bytes.len() > MAX_AGENT_MANIFEST_BYTES {
            return Err(invalid("Agent dependency manifest exceeds 4 MiB"));
        }
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|_| invalid("Invalid agent dependency manifest"))?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.agents.len().saturating_add(self.workflows.len()) > MAX_AGENT_DEPENDENCIES {
            return Err(invalid("Agent dependency manifest exceeds 256 nodes"));
        }
        let agents: BTreeSet<_> = self.agents.iter().map(|entry| entry.agent_id).collect();
        let workflows: BTreeSet<_> = self
            .workflows
            .iter()
            .map(|entry| entry.version_id)
            .collect();
        if agents.len() != self.agents.len() || workflows.len() != self.workflows.len() {
            return Err(invalid("Duplicate agent or workflow dependency selection"));
        }
        self.validate_roles(&workflows)?;
        Ok(())
    }

    fn validate_roles(&self, workflows: &BTreeSet<VersionId>) -> AppResult<()> {
        if !self.roles.is_empty() && self.role_context_department_id.is_none() {
            return Err(invalid(
                "Captured roles require their original department context",
            ));
        }
        let mut identities = BTreeSet::new();
        for role in &self.roles {
            if !workflows.contains(&role.version_id)
                || !identities.insert((role.version_id, &role.step, role.role.as_str()))
                || role.principals.is_empty()
                || role.principals.len() > crate::domain::workflow::TokenAllowance::MAX_REVIEWERS
            {
                return Err(invalid("Invalid or duplicate captured role requirement"));
            }
            let mut principals = BTreeSet::new();
            for holder in &role.principals {
                if !principals.insert(holder.principal_id) {
                    return Err(invalid("Duplicate captured role principal"));
                }
                match (holder.agent_id, holder.revision_id) {
                    (None, None) => {}
                    (Some(agent), Some(revision))
                        if role.principals.len() == 1
                            && self.agents.iter().any(|selected| {
                                selected.agent_id == agent && selected.revision_id == revision
                            }) => {}
                    _ => {
                        return Err(invalid(
                            "Role holder does not match the captured agent revision",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn canonical_content(&self) -> AppResult<Vec<u8>> {
        self.validate()?;
        let mut manifest = self.clone();
        manifest.agents.sort_by_key(|entry| entry.agent_id);
        manifest
            .workflows
            .sort_by_key(|entry| (entry.workflow_id, entry.version_id));
        manifest
            .roles
            .sort_by_key(|entry| (entry.version_id, entry.step.clone(), entry.role.as_str()));
        for role in &mut manifest.roles {
            role.principals.sort_by_key(|entry| entry.principal_id);
        }
        let value =
            serde_json::to_value(manifest).map_err(|_| invalid("Invalid dependency manifest"))?;
        let bytes =
            serde_json::to_vec(&value).map_err(|_| invalid("Invalid dependency manifest"))?;
        if bytes.len() > MAX_AGENT_MANIFEST_BYTES {
            return Err(invalid("Agent dependency manifest exceeds 4 MiB"));
        }
        Ok(bytes)
    }

    pub fn content_hash(&self) -> AppResult<[u8; 32]> {
        Ok(Sha256::digest(self.canonical_content()?).into())
    }
}
