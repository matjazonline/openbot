//! Workflow publication captures only durable agent identities and their source uses.
use super::http::begin_http;
use super::*;
use crate::application::agent::AgentKey;
use crate::application::workflow::lifecycle::{PublicationDependencies, PublicationDirectory};
use crate::application::workflow::publication::{
    AgentReference, AgentReferenceUse, agent_reference_uses,
};
use crate::application::workflow::templates::CompanyWorkflowDraft;
use crate::entities::agent_capability::AgentId;
use std::collections::BTreeMap;

pub struct AgentPublicationDirectory<D> {
    pub persistence: PostgresPersistence,
    pub inner: D,
}

#[async_trait]
impl<D: PublicationDirectory> PublicationDirectory for AgentPublicationDirectory<D> {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        let mut captured = self.inner.capture(actor, draft).await?;
        let Some(selection) = agent_selection(draft)? else {
            return Ok(captured);
        };
        let mut tx = begin_http(&self.persistence).await?;
        authority::authorize_company(&mut tx, draft.company(), actor).await?;
        capture_agents_on(&mut tx, draft.company(), &selection, &mut captured).await?;
        tx.commit().await?;
        Ok(captured)
    }
}

pub(super) struct AgentSelection {
    references: BTreeMap<AgentKey, Vec<AgentReferenceUse>>,
}

pub(super) fn agent_selection(draft: &CompanyWorkflowDraft) -> AppResult<Option<AgentSelection>> {
    let decoded = crate::adapters::workflow_source::decode(draft.source())
        .map_err(|diagnostic| AppError::BadRequest(diagnostic.message.into()))?;
    let references = agent_reference_uses(&decoded).map_err(AppError::BadRequest)?;
    Ok((!references.is_empty()).then_some(AgentSelection { references }))
}

pub(super) async fn capture_agents_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    selection: &AgentSelection,
    captured: &mut PublicationDependencies,
) -> AppResult<()> {
    for (key, uses) in &selection.references {
        let id: Uuid = sqlx::query_scalar(
            "SELECT agent.id FROM agents AS agent WHERE agent.company_id=$1 AND agent.slug=$2 \
             AND EXISTS(SELECT 1 FROM agent_revisions AS revision \
                        WHERE revision.company_id=agent.company_id AND revision.agent_id=agent.id)",
        )
        .bind(company.as_uuid())
        .bind(key.as_str())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Agent {} must be published in this company before a workflow can reference it",
                key.as_str()
            ))
        })?;
        captured.snapshots.agents.push(AgentReference {
            company_id: company,
            agent_id: AgentId::new(id),
            key: key.clone(),
            uses: uses.clone(),
        });
    }
    Ok(())
}

pub(super) async fn verify_on(
    tx: &mut Transaction<'_, Postgres>,
    bundle: &PublishedBundle,
) -> AppResult<()> {
    for reference in &bundle.snapshots().agents {
        let current: Option<Uuid> = sqlx::query_scalar(
            "SELECT agent.id FROM agents AS agent WHERE agent.company_id=$1 AND agent.id=$2 AND agent.slug=$3 \
             AND EXISTS(SELECT 1 FROM agent_revisions AS revision \
                        WHERE revision.company_id=agent.company_id AND revision.agent_id=agent.id) FOR SHARE OF agent"
        ).bind(bundle.company_id().as_uuid()).bind(reference.agent_id.as_uuid()).bind(reference.key.as_str())
            .fetch_optional(&mut **tx).await?;
        if reference.company_id != bundle.company_id()
            || current != Some(reference.agent_id.as_uuid())
        {
            return Err(AppError::Conflict(format!(
                "Agent reference {} changed during publication",
                reference.key.as_str()
            )));
        }
    }
    Ok(())
}

/// Sample-only references never become a publication eligibility token.
pub(super) async fn capture_sample_agents_on(
    tx: &mut Transaction<'_, Postgres>,
    draft: &CompanyWorkflowDraft,
    captured: &mut PublicationDependencies,
) -> AppResult<()> {
    let Some(selection) = agent_selection(draft)? else {
        return Ok(());
    };
    for (key, uses) in selection.references {
        let id: Uuid =
            sqlx::query_scalar("SELECT id FROM agents WHERE company_id=$1 AND slug=$2 FOR SHARE")
                .bind(draft.company().as_uuid())
                .bind(key.as_str())
                .fetch_optional(&mut **tx)
                .await?
                .ok_or_else(missing)?;
        captured.snapshots.agents.push(AgentReference {
            company_id: draft.company(),
            agent_id: AgentId::new(id),
            key,
            uses,
        });
    }
    Ok(())
}
