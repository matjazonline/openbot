use super::*;
use crate::entities::agent_capability::AgentBindingSource;

impl Closure {
    pub(super) async fn workflow_edges(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        workflow: WorkflowId,
        version: VersionId,
    ) -> AppResult<Vec<Node>> {
        if let Some(prior) = self.workflows.insert(version, workflow)
            && prior != workflow
        {
            return Err(refused());
        }
        self.within_bound()?;
        // Exact immutable pins remain usable after archive or same-workflow registration repin.
        let bytes: Vec<u8> = sqlx::query_scalar(
            "SELECT bundle FROM workflow_versions WHERE company_id=$1 AND workflow_id=$2 AND id=$3 AND octet_length(bundle)<=$4 FOR SHARE"
        ).bind(self.company.as_uuid()).bind(workflow.as_uuid()).bind(version.as_uuid())
            .bind(MAX_BUNDLE_BYTES as i32).fetch_optional(&mut **tx).await?.ok_or_else(refused)?;
        let bundle = restore_bundle(
            &bytes,
            &crate::adapters::workflow_source::WorkflowSourceDecoder,
        )?;
        let definition = bundle.compiled().graph().definition();
        if bundle.company_id() != self.company
            || definition.workflow_id != workflow
            || definition.version_id != version
        {
            return Err(invalid());
        }
        // Every reachable workflow uses the root's explicitly bound STEP resources.
        // Validate its current contracts before any root/run/provider can exist.
        super::super::resources::check_readiness(
            tx,
            super::super::resources::ReadinessSelection {
                company: self.company,
                bundle: &bundle,
                resources: &self.resources,
            },
        )
        .await?;
        let mut edges = BTreeSet::new();
        for child in bundle.children().values() {
            let definition = child.compiled().graph().definition();
            edges.insert(Node::Workflow(
                definition.workflow_id,
                definition.version_id,
            ));
        }
        for reference in &bundle.snapshots().agents {
            for usage in &reference.uses {
                edges.insert(Node::Agent(reference.agent_id, usage.mode));
            }
        }
        Ok(edges.into_iter().collect())
    }

    pub(super) async fn agent_edges(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        agent: AgentId,
        mode: AgentUseMode,
    ) -> AppResult<Vec<Node>> {
        let revision = match self.agents.get(&agent) {
            Some(revision) => *revision,
            None => {
                let revision: Uuid = sqlx::query_scalar(
                    "SELECT revision_id FROM agent_active_revisions WHERE company_id=$1 AND agent_id=$2 FOR SHARE"
                ).bind(self.company.as_uuid()).bind(agent.as_uuid())
                    .fetch_optional(&mut **tx).await?.ok_or_else(refused)?;
                let revision = AgentRevisionId::new(revision);
                self.agents.insert(agent, revision);
                self.within_bound()?;
                revision
            }
        };
        let scope = AgentPublicationScope {
            company: self.company,
            actor: self.actor,
            agent,
        };
        let revision = crate::adapters::persistence::agent_publication::storage::revision_on(
            tx, scope, revision,
        )
        .await?
        .ok_or_else(refused)?;
        super::super::agent_authority::check_definition_on(tx, &revision.definition, mode).await?;
        if mode == AgentUseMode::ModelOnly {
            return Ok(Vec::new());
        }
        Ok(revision
            .definition
            .tools
            .iter()
            .filter_map(|tool| match &tool.binding.source {
                AgentBindingSource::Workflow {
                    workflow, version, ..
                } => Some(Node::Workflow(*workflow, *version)),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
}
