//! The one company-agent creation transaction (01DEP-P03-D1): the agent, its principal, its
//! first published version, its default desk at its slug and its department placement commit
//! together or not at all.
//!
//! Lock order, after the caller's account `FOR SHARE` and company write lock: the new agent and
//! principal rows, publication (re-entrant account/company locks, the new agent `FOR UPDATE`),
//! the desk (new rows only; no channel gate), then placement — the department row `FOR UPDATE`,
//! principals `FOR KEY SHARE`, and last, in one sorted call, the gates of the members-only desks
//! its grants reach.
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{
    PostgresPersistence,
    agent::{AGENT_COLUMNS, AgentDb, insert_agent_on},
    agent_publication::{begin_bounded, lock_company_on, publish_initial_on},
    channel::prepare_library_agent_on,
    department::place_in_department_on,
    participant::create_agent_principal_on,
    workflow::provisioning::{ProvisioningDeployment, assistant_channel_on, ready_models_on},
};
use crate::{
    app_error::{AppError, AppResult},
    application::{
        agent::{AgentCompanyScope, AgentPublicationScope},
        workflow::{
            WorkflowActor,
            provisioning::{AssistantChannelRequest, ProvisioningRefusal, ReplyMode},
        },
    },
    domain::workflow::CompanyId,
    entities::{agent::Agent, agent_capability::AgentId},
    use_cases::agent::{AgentPlacement, AgentWrite, CompanyAgents, CreatedAgent, model_allowed},
};

/// The command key that records an agent's default desk.
pub(crate) fn default_desk_key(agent: Uuid) -> String {
    format!("agent:{agent}")
}

/// What [`create_company_agent_on`] creates: the agent under `id` from `write`, placed in
/// `placement` when one is named. A caller with writes of its own after the creation passes no
/// placement and places the agent last itself (`place_in_department_on`), so the department's
/// desk gates stay the last locks it takes.
pub(crate) struct CompanyAgentCreation {
    pub id: Uuid,
    pub write: AgentWrite,
    pub placement: Option<AgentPlacement>,
}

pub(crate) struct CreatedCompanyAgent {
    /// The agent's principal, for a caller that places it after more work of its own.
    pub principal: crate::entities::transport::PrincipalId,
    /// The default desk's provisioning result, as its `agent:{id}` command row stores it.
    pub desk: Value,
}
impl CreatedCompanyAgent {
    pub fn desk_id(&self) -> AppResult<Uuid> {
        self.desk["channel"]["id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| AppError::Internal("A default desk result has no channel id".into()))
    }
}

/// Create a company agent on the caller's transaction. The caller holds `actor`'s account row
/// `FOR SHARE` and the company write lock, and has authorized `actor`. Any error leaves writes
/// behind on `tx` that the caller must roll back (or roll back to its savepoint).
pub(crate) async fn create_company_agent_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    creation: CompanyAgentCreation,
) -> AppResult<CreatedCompanyAgent> {
    let CompanyAgentCreation {
        id,
        mut write,
        placement,
    } = creation;
    resolve_model_on(tx, company, &mut write).await?;
    insert_agent_on(tx, id, Some(company.as_uuid()), &write).await?;
    let principal = create_agent_principal_on(tx, company.as_uuid(), id, &write.name).await?;
    publish_initial_on(
        tx,
        AgentPublicationScope {
            company,
            actor,
            agent: AgentId::new(id),
        },
    )
    .await?;
    // Boxed: the desk descends into template compilation and workflow publication.
    let desk = Box::pin(assistant_channel_on(
        tx,
        templates,
        company,
        actor,
        AssistantChannelRequest {
            agent_slug: write.slug.clone(),
            channel_slug: write.slug.clone(),
            channel_name: write.name.clone(),
            reply_mode: ReplyMode::Autonomous,
            command_key: default_desk_key(id),
        },
    ))
    .await?
    .map_err(refusal_error)?;
    if let Some(placement) = placement {
        place_in_department_on(tx, company, placement.department, principal, placement.role)
            .await?;
    }
    Ok(CreatedCompanyAgent { principal, desk })
}

/// A desk refusal as the error the creating route answers with, as "Create assistant channel"
/// does: forbidden is not found, a taken address a conflict, anything invalid a bad request.
fn refusal_error(refusal: ProvisioningRefusal) -> AppError {
    match refusal {
        ProvisioningRefusal::Forbidden => AppError::NotFound("Company not found".into()),
        ProvisioningRefusal::Conflict(message) => AppError::Conflict(message),
        ProvisioningRefusal::Invalid(message) => AppError::BadRequest(message),
    }
}

/// An omitted model pinned to the company default — the default connection with a key, its
/// first model — and an explicit one checked against the keyed connections (01DEP-P03-D3): the
/// desk's workflow runs an agent only on an explicit model it can call.
async fn resolve_model_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    write: &mut AgentWrite,
) -> AppResult<()> {
    match (write.provider.as_deref(), write.model.as_deref()) {
        (None, None) => {
            let default: Option<(String, String)> = sqlx::query_as(
                "SELECT provider, models[1] FROM company_model_connections \
                 WHERE company_id = $1 AND is_default AND api_key IS NOT NULL \
                   AND cardinality(models) > 0",
            )
            .bind(company.as_uuid())
            .fetch_optional(&mut **tx)
            .await?;
            let (provider, model) = default.ok_or_else(no_usable_model)?;
            write.provider = Some(provider);
            write.model = Some(model);
            Ok(())
        }
        (Some(provider), Some(model)) => {
            let ready: Vec<_> = ready_models_on(tx, company)
                .await?
                .into_iter()
                .map(
                    |(provider, model)| crate::entities::company::CompanyModelConnection {
                        provider: provider.into(),
                        models: vec![model.into()],
                        is_default: false,
                        has_api_key: true,
                    },
                )
                .collect();
            if ready.is_empty() {
                return Err(no_usable_model());
            }
            if !model_allowed(&ready, provider, model) {
                return Err(AppError::BadRequest(format!(
                    "Model '{model}' of provider '{provider}' is not enabled with an API key in this company."
                )));
            }
            Ok(())
        }
        _ => Err(AppError::BadRequest(
            "Select both a provider and one of its enabled models, or inherit the company default."
                .into(),
        )),
    }
}

fn no_usable_model() -> AppError {
    AppError::BadRequest(
        "Add a model connection with an API key before creating an agent: its desk answers on an explicit model."
            .into(),
    )
}

/// Company-agent creation for the UI, the JSON API and library copies (01DEP-P03-D2): one
/// bounded transaction under the company write lock around [`create_company_agent_on`].
pub struct CompanyAgentService {
    persistence: PostgresPersistence,
    templates: ProvisioningDeployment,
}
impl CompanyAgentService {
    pub fn new(persistence: PostgresPersistence, templates: ProvisioningDeployment) -> Self {
        Self {
            persistence,
            templates,
        }
    }

    async fn create_on(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        library_agent_id: Option<Uuid>,
        mut write: AgentWrite,
        placement: Option<AgentPlacement>,
    ) -> AppResult<CreatedAgent> {
        let mut tx = begin_bounded(&self.persistence).await?;
        lock_company_on(&mut tx, AgentCompanyScope { company, actor }).await?;
        if let Some(library_agent_id) = library_agent_id {
            prepare_library_agent_on(&mut tx, company.as_uuid(), library_agent_id, &mut write)
                .await?;
        }
        let id = Uuid::new_v4();
        let created = Box::pin(create_company_agent_on(
            &mut tx,
            &self.templates,
            company,
            actor,
            CompanyAgentCreation {
                id,
                write,
                placement,
            },
        ))
        .await?;
        let desk_id = created.desk_id()?;
        let agent: Agent = sqlx::query_as::<_, AgentDb>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents AS agent WHERE agent.id = $1"
        ))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?
        .try_into()?;
        tx.commit()
            .await
            .map_err(super::agent_capability::relationship_error)?;
        Ok(CreatedAgent { agent, desk_id })
    }
}

#[async_trait]
impl CompanyAgents for CompanyAgentService {
    async fn create(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        write: AgentWrite,
        placement: Option<AgentPlacement>,
    ) -> AppResult<CreatedAgent> {
        self.create_on(actor, company, None, write, placement).await
    }

    async fn copy_from_library(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        library_agent_id: Uuid,
        write: AgentWrite,
        placement: Option<AgentPlacement>,
    ) -> AppResult<CreatedAgent> {
        self.create_on(actor, company, Some(library_agent_id), write, placement)
            .await
    }
}

#[cfg(test)]
#[path = "company_agent_tests.rs"]
mod tests;
