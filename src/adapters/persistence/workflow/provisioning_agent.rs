//! Agent selection and initial publication on the caller’s provisioning transaction.
use super::*;

pub(super) struct ProvisioningAgentSlug(pub(super) String);

pub(super) struct PreparedAssistantAgent {
    pub(super) id: crate::entities::agent_capability::AgentId,
    pub(super) slug: ProvisioningAgentSlug,
}

#[derive(sqlx::FromRow)]
struct ProvisioningAgentRow {
    id: Uuid,
    slug: String,
    provider: Option<String>,
    model: Option<String>,
}

/// The existing company agent the desk answers with, published if it is not yet.
pub(super) async fn provision_agent_on(
    tx: &mut Transaction<'_, Postgres>,
    provisioning: &Provisioning,
) -> AppResult<Provisioned<PreparedAssistantAgent>> {
    let company = provisioning.company;
    let slug = &provisioning.agent_slug;
    let agent: Option<ProvisioningAgentRow> = sqlx::query_as(
        "SELECT id, slug::text, provider, model FROM agents \
         WHERE company_id = $1 AND slug = $2 FOR KEY SHARE",
    )
    .bind(company.as_uuid())
    .bind(slug.trim().to_lowercase())
    .fetch_optional(&mut **tx)
    .await?;
    let Some(agent) = agent else {
        return Ok(Err(ProvisioningRefusal::Invalid(format!(
            "No agent named '{}' exists in this company",
            slug.trim()
        ))));
    };
    if agent.provider.is_none() || agent.model.is_none() {
        return Ok(Err(ProvisioningRefusal::Invalid(
            "Choose the agent's provider and model first: a workflow runs an agent only on an explicit model".into(),
        )));
    }

    crate::adapters::persistence::agent_publication::publish_initial_on(
        tx,
        crate::application::agent::AgentPublicationScope {
            company,
            actor: provisioning.requester,
            agent: crate::entities::agent_capability::AgentId::new(agent.id),
        },
    )
    .await?;

    Ok(Ok(PreparedAssistantAgent {
        id: crate::entities::agent_capability::AgentId::new(agent.id),
        slug: ProvisioningAgentSlug(agent.slug),
    }))
}
