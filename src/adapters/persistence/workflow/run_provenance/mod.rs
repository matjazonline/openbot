//! Bounded metadata from native run selections. Mutable drafts, names and grants are irrelevant.
use super::*;
use crate::application::agent::{
    AgentToolChoice, ResolvedAgentDefinition, WorkflowVersionSelection,
};
use crate::application::workflow::run_views::{
    MAX_STEP_ITEMS, RunDependencies, SelectedAgentSummary, SelectedSkillSummary,
    SelectedToolSummary,
};
use crate::entities::agent_capability::{AgentId, AgentRevisionId, DependencyManifestId};

use std::time::Duration;

mod calls;

pub(super) struct CallRecordScope {
    pub record: Uuid,
    pub execution: Uuid,
}

#[derive(sqlx::FromRow)]
struct RevisionRow {
    manifest_id: Uuid,
    agent_id: Uuid,
    id: Uuid,
    content: Option<String>,
    digest: Vec<u8>,
}

const REVISIONS: &str = "SELECT ownership.manifest_id, selection.agent_id, revision.id, \
    CASE WHEN octet_length(revision.content) <= $3 THEN revision.content END AS content, revision.digest \
    FROM run_dependency_manifests AS ownership \
    JOIN agent_manifest_revisions AS selection \
      ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id \
    JOIN agent_revisions AS revision \
      ON revision.company_id=selection.company_id AND revision.agent_id=selection.agent_id AND revision.id=selection.revision_id \
    WHERE ownership.company_id=$1 AND ownership.run_id=$2";

fn invalid_provenance() -> AppError {
    AppError::Internal("Invalid immutable run provenance".into())
}

impl RevisionRow {
    fn summary(self, company: CompanyId) -> AppResult<SelectedAgentSummary> {
        let content = self.content.ok_or_else(invalid_provenance)?;
        let definition = ResolvedAgentDefinition::decode(content.as_bytes())?;
        if definition.company_id != company
            || definition.agent_id.as_uuid() != self.agent_id
            || definition.content_hash()?.as_slice() != self.digest.as_slice()
        {
            return Err(invalid_provenance());
        }
        let bindings = definition
            .tools
            .iter()
            .map(|tool| {
                Ok(SelectedToolSummary {
                    binding: tool.binding.id,
                    alias: tool.binding.alias.clone(),
                    target: AgentToolChoice::from_source(&tool.binding.source)?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let assignments = definition
            .skills
            .iter()
            .map(|skill| SelectedSkillSummary {
                assignment: skill.assignment_id,
                skill: skill.skill.skill_id,
                name: skill.skill.name.as_str().to_owned(),
            })
            .collect::<Vec<_>>();
        Ok(SelectedAgentSummary {
            manifest: DependencyManifestId::new(self.manifest_id),
            agent_id: definition.agent_id,
            revision_id: AgentRevisionId::new(self.id),
            key: definition.key.as_str().to_owned(),
            model: definition
                .model
                .as_ref()
                .map(|model| format!("{} / {}", model.provider.as_str(), model.model)),
            tools: bindings
                .iter()
                .map(|tool| tool.alias.as_str().to_owned())
                .collect(),
            skills: assignments.iter().map(|skill| skill.name.clone()).collect(),
            bindings,
            assignments,
        })
    }
}

#[derive(sqlx::FromRow)]
struct SelectedWorkflowRow {
    workflow_id: Uuid,
    version_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct OwnershipRow {
    manifest_id: Uuid,
    root_run_id: Uuid,
}

impl PostgresPersistence {
    pub(super) async fn run_dependencies(
        &self,
        company: CompanyId,
        run: RunId,
    ) -> AppResult<RunDependencies> {
        tokio::time::timeout(Duration::from_secs(5), async {
            let ownership: OwnershipRow = sqlx::query_as(
                "SELECT manifest_id, root_run_id FROM run_dependency_manifests WHERE company_id=$1 AND run_id=$2"
            ).bind(company.as_uuid()).bind(run.as_uuid()).fetch_optional(self.pool()).await?
                .ok_or_else(invalid_provenance)?;
            let sql = format!("{REVISIONS} ORDER BY selection.agent_id LIMIT $4");
            let mut agents = sqlx::query_as::<_, RevisionRow>(&sql)
                .bind(company.as_uuid()).bind(run.as_uuid())
                .bind(crate::application::agent::MAX_AGENT_DEFINITION_BYTES as i32)
                .bind(MAX_STEP_ITEMS + 1).fetch_all(self.pool()).await?;
            let agents_truncated = agents.len() as i64 > MAX_STEP_ITEMS;
            agents.truncate(MAX_STEP_ITEMS as usize);
            let mut summaries = Vec::with_capacity(agents.len());
            for agent in agents {
                summaries.push(agent.summary(company)?);
                tokio::task::yield_now().await;
            }
            let mut workflows: Vec<SelectedWorkflowRow> = sqlx::query_as(
                "SELECT selection.workflow_id, selection.version_id FROM run_dependency_manifests AS ownership \
                 JOIN agent_manifest_workflows AS selection \
                   ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id \
                 WHERE ownership.company_id=$1 AND ownership.run_id=$2 \
                 ORDER BY selection.workflow_id, selection.version_id LIMIT $3"
            ).bind(company.as_uuid()).bind(run.as_uuid()).bind(MAX_STEP_ITEMS+1)
                .fetch_all(self.pool()).await?;
            let workflows_truncated = workflows.len() as i64 > MAX_STEP_ITEMS;
            workflows.truncate(MAX_STEP_ITEMS as usize);
            Ok(RunDependencies {
                manifest: DependencyManifestId::new(ownership.manifest_id),
                root: RunId::new(ownership.root_run_id),
                agents: summaries,
                agents_truncated,
                workflows: workflows.into_iter().map(|row| WorkflowVersionSelection {
                    workflow_id: WorkflowId::new(row.workflow_id),
                    version_id: VersionId::new(row.version_id),
                }).collect(),
                workflows_truncated,
            })
        }).await.map_err(|_| invalid_provenance())?
    }

    pub(super) async fn selected_agent_metadata(
        &self,
        company: CompanyId,
        run: RunId,
        agent: AgentId,
    ) -> AppResult<SelectedAgentSummary> {
        let sql = format!("{REVISIONS} AND selection.agent_id=$4");
        sqlx::query_as::<_, RevisionRow>(&sql)
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .bind(crate::application::agent::MAX_AGENT_DEFINITION_BYTES as i32)
            .bind(agent.as_uuid())
            .fetch_optional(self.pool())
            .await?
            .ok_or_else(invalid_provenance)?
            .summary(company)
    }
}
