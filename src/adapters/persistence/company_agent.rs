//! The one company-agent creation transaction (01DEP-P03-D1): the agent, its principal, its
//! first published version, its direct address/workflow and department placement commit
//! together or not at all.
//!
//! Lock order, after the caller's account `FOR SHARE` and company write lock: the new agent and
//! principal rows, publication (re-entrant account/company locks, the new agent `FOR UPDATE`),
//! the desk (new rows only; no channel gate), then placement — the department row `FOR UPDATE`,
//! principals `FOR KEY SHARE`, and last, in one sorted call, the gates of the members-only desks
//! its grants reach.
use async_trait::async_trait;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{
    PostgresPersistence,
    agent::{AGENT_COLUMNS, AgentDb, insert_agent_on},
    agent_publication::{begin_bounded, lock_company_on, publish_initial_on},
    channel::prepare_library_agent_on,
    department::place_in_department_on,
    participant::create_agent_principal_on,
    workflow::provisioning::{
        AgentWorkflowOwner, ProvisioningDeployment, provision_agent_workflow_on, ready_models_on,
    },
};
use crate::{
    app_error::{AppError, AppResult},
    application::{
        agent::{AgentCompanyScope, AgentPublicationScope},
        workflow::{WorkflowActor, provisioning::ProvisioningRefusal},
    },
    domain::workflow::CompanyId,
    entities::{agent::Agent, agent_capability::AgentId},
    use_cases::agent::{AgentPlacement, AgentWrite, CompanyAgents, CreatedAgent, model_allowed},
};

/// Membership is established before publication; callers doing more construction finalize
/// deferred placement last so department desk gates remain the last locks they take.
#[derive(Clone, Copy)]
pub(crate) enum AgentCreationPlacement {
    Unplaced,
    Complete(AgentPlacement),
    Deferred(AgentPlacement),
}

impl AgentCreationPlacement {
    fn requested(self) -> Option<AgentPlacement> {
        match self {
            Self::Unplaced => None,
            Self::Complete(placement) | Self::Deferred(placement) => Some(placement),
        }
    }
}

pub(crate) struct CompanyAgentCreation {
    pub id: Uuid,
    pub write: AgentWrite,
    pub placement: AgentCreationPlacement,
}

pub(crate) struct CreatedCompanyAgent {
    /// The agent's principal, for a caller that places it after more work of its own.
    pub principal: crate::entities::transport::PrincipalId,
    pub workflow: super::workflow::provisioning::AgentWorkflow,
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
    let mut creation = creation;
    // Publication capture dominates this phase; keep it off the constructor's future frame.
    let principal = Box::pin(insert_company_agent_identity_on(
        tx,
        company,
        actor,
        &mut creation,
    ))
    .await?;
    let CompanyAgentCreation {
        id,
        write,
        placement,
    } = creation;
    // Boxed: template compilation and publication otherwise dominate the creator's future.
    let workflow = Box::pin(provision_agent_workflow_on(
        tx,
        templates,
        company,
        actor,
        AgentWorkflowOwner {
            agent: id,
            principal,
            slug: &write.slug,
        },
    ))
    .await?
    .map_err(refusal_error)?;
    if let AgentCreationPlacement::Complete(placement) = placement {
        place_in_department_on(
            tx,
            templates,
            company,
            actor,
            placement.department,
            principal,
            placement.role,
        )
        .await?;
    }
    Ok(CreatedCompanyAgent {
        principal,
        workflow,
    })
}

async fn insert_company_agent_identity_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    creation: &mut CompanyAgentCreation,
) -> AppResult<crate::entities::transport::PrincipalId> {
    let id = creation.id;
    let placement = creation.placement;
    let write = &mut creation.write;
    super::workflow::management::authorize_target_on(
        tx,
        company,
        actor,
        crate::use_cases::agent::placement_target(placement.requested()),
    )
    .await?;
    resolve_model_on(
        tx,
        company,
        placement.requested().map(|placement| placement.department),
        write,
    )
    .await?;
    insert_agent_on(tx, id, Some(company.as_uuid()), write).await?;
    let principal = create_agent_principal_on(tx, company.as_uuid(), id, &write.name).await?;
    if let Some(placement) = placement.requested() {
        super::department::prepare_new_agent_placement_on(
            tx,
            crate::application::ports::department::DepartmentScope { company, actor },
            principal,
            placement,
        )
        .await?;
    }
    super::agent::write_direct_settings_on(tx, Some(company.as_uuid()), id, write).await?;
    register_agent_address_on(tx, company, principal, &write.slug).await?;
    publish_initial_on(
        tx,
        AgentPublicationScope {
            company,
            actor,
            agent: AgentId::new(id),
        },
    )
    .await?;
    Ok(principal)
}

async fn register_agent_address_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    principal: crate::entities::transport::PrincipalId,
    slug: &str,
) -> AppResult<()> {
    crate::use_cases::channel::validate_slug(slug, crate::use_cases::channel::SlugKind::AgentSlug)?;
    let inserted: Option<String> = sqlx::query_scalar(
        "INSERT INTO addresses (company_id,slug,target_kind,principal_id,is_primary) \
         VALUES ($1,$2,'principal',$3,true) ON CONFLICT (company_id,slug) DO NOTHING RETURNING slug::text",
    ).bind(company.as_uuid()).bind(slug).bind(principal.as_uuid())
        .fetch_optional(&mut **tx).await?;
    if inserted.is_none() {
        return Err(AppError::Conflict(format!(
            "Address '{slug}' is already taken in this company."
        )));
    }
    super::integration::email_binding::sync_principal_email_binding_on(
        tx,
        company.as_uuid(),
        principal,
    )
    .await
}

/// A desk refusal as the error the creating route answers with, as "Create assistant channel"
/// does: forbidden is not found, a taken address a conflict, anything invalid a bad request.
pub(crate) fn refusal_error(refusal: ProvisioningRefusal) -> AppError {
    match refusal {
        ProvisioningRefusal::Forbidden => AppError::NotFound("Company not found".into()),
        ProvisioningRefusal::Conflict(message) => AppError::Conflict(message),
        ProvisioningRefusal::Invalid(message) => AppError::BadRequest(message),
    }
}

/// An omitted model pinned to the nearest default from the agent's department up to the root
/// (Plan07 02A.2; the root's for an unplaced agent), and an explicit one checked against the
/// keyed connections (01DEP-P03-D3) and the department's reach: the desk's workflow runs an agent
/// only on an explicit model it can call.
async fn resolve_model_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: Option<crate::entities::department::DepartmentId>,
    write: &mut AgentWrite,
) -> AppResult<()> {
    let start = department.map(|department| department.as_uuid());
    match (write.provider.as_deref(), write.model.as_deref()) {
        (None, None) => {
            let default: Option<(String, String)> =
                sqlx::query_as("SELECT provider, model FROM unit_default_model($1, $2)")
                    .bind(company.as_uuid())
                    .bind(start)
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
                        id: Uuid::nil(),
                        unit_id: Uuid::nil(),
                        unit_name: String::new(),
                        in_root: false,
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
            if !model_in_reach_on(tx, company, start, provider).await? {
                return Err(AppError::BadRequest(format!(
                    "Model '{model}' of provider '{provider}' belongs to a department outside this agent's department."
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

/// Whether `provider`'s connection belongs to `start` (the root when `None`) or an ancestor
/// (Plan07 02A, README decision 1).
pub(crate) async fn model_in_reach_on(
    db: &mut sqlx::PgConnection,
    company: CompanyId,
    start: Option<Uuid>,
    provider: &str,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT unit_reaches($1, COALESCE($2, (SELECT id FROM departments \
             WHERE company_id = $1 AND is_root)), unit_id) \
         FROM company_model_connections WHERE company_id = $1 AND provider = lower(btrim($3))",
    )
    .bind(company.as_uuid())
    .bind(start)
    .bind(provider)
    .fetch_optional(db)
    .await?
    .unwrap_or(false))
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
        super::workflow::management::authorize_target_on(
            &mut tx,
            company,
            actor,
            crate::use_cases::agent::placement_target(placement),
        )
        .await?;
        if let Some(library_agent_id) = library_agent_id {
            let unit = match placement {
                Some(placement) => placement.department,
                None => super::company::root_on(&mut tx, company).await?,
            };
            prepare_library_agent_on(
                &mut tx,
                company.as_uuid(),
                unit.as_uuid(),
                library_agent_id,
                &mut write,
            )
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
                placement: placement.map_or(
                    AgentCreationPlacement::Unplaced,
                    AgentCreationPlacement::Complete,
                ),
            },
        ))
        .await?;
        let principal_id = created.principal;
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
        Ok(CreatedAgent {
            agent,
            principal_id,
        })
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
