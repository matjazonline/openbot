//! Agent creation/selection and initial publication on the caller’s provisioning transaction.
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

pub(super) async fn provision_agent_on(
    tx: &mut Transaction<'_, Postgres>,
    provisioning: &Provisioning,
    created_by: &CreationProvenance,
) -> AppResult<Provisioned<PreparedAssistantAgent>> {
    let company = provisioning.company;
    let requester = provisioning.requester;
    let key = &provisioning.key;
    let (agent_id, agent_slug) = match &provisioning.agent {
        AssistantAgent::New(spec) => {
            let mut write = match agent_write(tx, company, spec, created_by).await? {
                Ok(write) => write,
                Err(refusal) => return Ok(Err(refusal)),
            };
            if let Err(error) = write.normalize() {
                return Ok(Err(refused(error)));
            }
            if let Some(refusal) = malformed("agent slug", &write.slug) {
                return Ok(Err(refusal));
            }
            let id = key.id("agent");
            insert_agent_on(tx, id, Some(company.as_uuid()), &write).await?;
            create_agent_principal_on(tx, company.as_uuid(), id, &write.name).await?;
            (id, write.slug)
        }
        AssistantAgent::Existing(slug) => {
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
            (agent.id, agent.slug)
        }
    };

    crate::adapters::persistence::agent_publication::publish_initial_on(
        tx,
        crate::application::agent::AgentPublicationScope {
            company,
            actor: requester,
            agent: crate::entities::agent_capability::AgentId::new(agent_id),
        },
    )
    .await?;

    Ok(Ok(PreparedAssistantAgent {
        id: crate::entities::agent_capability::AgentId::new(agent_id),
        slug: ProvisioningAgentSlug(agent_slug),
    }))
}
