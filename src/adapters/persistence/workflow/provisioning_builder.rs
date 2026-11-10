//! The Agent Builder's `create_agent_channel` (01DEP-P03-D5): a company agent through the shared
//! creation, then its default desk as the answer or another desk beside it, in one savepoint.
use std::str::FromStr;

use super::*;
use crate::adapters::persistence::company_agent::{CompanyAgentCreation, create_company_agent_on};
use crate::adapters::persistence::department::place_in_department_on;
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::use_cases::agent::AgentPlacement;

/// The Builder's command in a savepoint. A refusal rolls it back and is returned; so is an error
/// the creation answers for the request (a conflict, an invalid or missing reference), or one
/// left behind by a slug another creator took meanwhile. Anything else propagates, so the action
/// transaction rolls back (F046).
pub(super) async fn create_agent_channel_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    requester: WorkflowActor,
    spec: &CreateAgentChannel,
    invocation: Uuid,
) -> AppResult<Provisioned<Value>> {
    let mut savepoint = sqlx::Acquire::begin(&mut **tx).await?;
    match builder_on(
        &mut savepoint,
        templates,
        company,
        requester,
        spec,
        invocation,
    )
    .await
    {
        Ok(Ok(created)) => {
            savepoint.commit().await?;
            Ok(Ok(created))
        }
        Ok(Err(refusal)) => {
            savepoint.rollback().await?;
            Ok(Err(refusal))
        }
        Err(error) => {
            savepoint.rollback().await?;
            match error {
                AppError::Conflict(message) => Ok(Err(ProvisioningRefusal::Conflict(message))),
                AppError::BadRequest(message) | AppError::NotFound(message) => {
                    Ok(Err(ProvisioningRefusal::Invalid(message)))
                }
                error => match taken_by_another(tx, company, spec).await? {
                    Some(refusal) => Ok(Err(refusal)),
                    None => Err(error),
                },
            }
        }
    }
}

async fn builder_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    requester: WorkflowActor,
    spec: &CreateAgentChannel,
    invocation: Uuid,
) -> AppResult<Provisioned<Value>> {
    let created_by = CreationProvenance::user(requester.user_id());
    let mut write = match agent_write(tx, company, spec, &created_by).await? {
        Ok(write) => write,
        Err(refusal) => return Ok(Err(refusal)),
    };
    if let Err(error) = write.normalize() {
        return Ok(Err(refused(error)));
    }
    if let Some(refusal) = malformed("agent slug", &write.slug) {
        return Ok(Err(refusal));
    }
    let desk_slug = spec
        .channel_slug
        .as_deref()
        .map_or_else(|| write.slug.clone(), |slug| slug.trim().to_lowercase());
    let default_desk = desk_slug == write.slug;
    if default_desk && !answers_as_default_desk(spec, &write.name) {
        return Ok(Err(ProvisioningRefusal::Invalid(
            "The agent's own address is its autonomous default desk; choose another address for a reviewed desk, custom steps or another name".into(),
        )));
    }
    let placement = match placement_on(tx, company, requester, spec).await? {
        Ok(placement) => placement,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let key = ProvisioningKey::invocation(invocation);
    let agent_slug = write.slug.clone();
    // Placed last, after any additional desk, so the department's desk gates are the last
    // locks this command takes (01DEP-P03-B1-F5).
    let created = Box::pin(create_company_agent_on(
        tx,
        templates,
        company,
        requester,
        CompanyAgentCreation {
            id: key.id("agent"),
            write,
            placement: None,
        },
    ))
    .await?;
    let answer = if default_desk {
        created.desk
    } else {
        let provisioning = Provisioning {
            company,
            requester,
            agent_slug,
            channel_slug: desk_slug,
            channel_name: spec
                .channel_name
                .clone()
                .unwrap_or_else(|| spec.name.clone()),
            reply_mode: spec.reply_mode,
            steps: spec.steps.clone(),
            key,
        };
        match Box::pin(provision_on(tx, templates, &provisioning)).await? {
            Ok(provisioned) => provisioned.to_json(),
            Err(refusal) => return Ok(Err(refusal)),
        }
    };
    if let Some(placement) = placement {
        place_in_department_on(
            tx,
            company,
            placement.department,
            created.principal,
            placement.role,
        )
        .await?;
    }
    Ok(Ok(answer))
}

/// Whether the requested desk is exactly the agent's default one: autonomous, no added steps, and
/// the agent's own name.
fn answers_as_default_desk(spec: &CreateAgentChannel, agent_name: &str) -> bool {
    spec.reply_mode == ReplyMode::Autonomous
        && spec.steps.is_empty()
        && spec
            .channel_name
            .as_deref()
            .is_none_or(|name| name.trim() == agent_name)
}

/// The department the agent joins: the named one, or the requester's own when they belong to
/// exactly one (plan 01-departments 03C item 0). A role needs a named department.
async fn placement_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    requester: WorkflowActor,
    spec: &CreateAgentChannel,
) -> AppResult<Provisioned<Option<AgentPlacement>>> {
    let role = match spec.role.as_deref().map(DepartmentRole::from_str) {
        None => None,
        Some(Ok(role)) => Some(role),
        Some(Err(_)) => {
            return Ok(Err(ProvisioningRefusal::Invalid(format!(
                "'{}' is not a department role",
                spec.role.as_deref().unwrap_or_default()
            ))));
        }
    };
    let department = match spec.department.as_deref() {
        Some(slug) => {
            let id: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM departments WHERE company_id = $1 AND slug = $2",
            )
            .bind(company.as_uuid())
            .bind(slug.trim().to_lowercase())
            .fetch_optional(&mut **tx)
            .await?;
            match id {
                Some(id) => Some(DepartmentId::new(id)),
                None => {
                    return Ok(Err(ProvisioningRefusal::Invalid(format!(
                        "No department named '{}' exists in this company",
                        slug.trim()
                    ))));
                }
            }
        }
        None if role.is_some() => {
            return Ok(Err(ProvisioningRefusal::Invalid(
                "Name the department the agent holds its role in".into(),
            )));
        }
        None => {
            let ids: Vec<Uuid> = sqlx::query_scalar(
                "SELECT placed.department_id FROM principals AS principal \
                 JOIN (SELECT company_id, department_id, principal_id FROM department_members \
                       UNION \
                       SELECT company_id, department_id, principal_id FROM department_roles) \
                   AS placed ON placed.company_id = principal.company_id \
                  AND placed.principal_id = principal.id \
                 WHERE principal.company_id = $1 AND principal.user_id = $2 \
                   AND principal.kind = 'person' \
                 GROUP BY placed.department_id LIMIT 2",
            )
            .bind(company.as_uuid())
            .bind(requester.user_id())
            .fetch_all(&mut **tx)
            .await?;
            match ids.as_slice() {
                [id] => Some(DepartmentId::new(*id)),
                _ => None,
            }
        }
    };
    Ok(Ok(
        department.map(|department| AgentPlacement { department, role })
    ))
}

/// After an unexpected error: the conflict refusal when the agent's slug or either desk address
/// was taken by a creator that does not hold the company lock.
async fn taken_by_another(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    spec: &CreateAgentChannel,
) -> AppResult<Option<ProvisioningRefusal>> {
    let taken: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM agents WHERE company_id = $1 AND slug = $2)",
    )
    .bind(company.as_uuid())
    .bind(spec.slug.trim().to_lowercase())
    .fetch_one(&mut **tx)
    .await?;
    if taken {
        return Ok(Some(ProvisioningRefusal::Conflict(format!(
            "An agent named '{}' already exists in this company",
            spec.slug.trim()
        ))));
    }
    if let Some(refusal) = taken_slug(tx, company, &spec.slug).await? {
        return Ok(Some(refusal));
    }
    match spec.channel_slug.as_deref() {
        Some(slug) => taken_slug(tx, company, slug).await,
        None => Ok(None),
    }
}
