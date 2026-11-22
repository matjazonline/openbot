//! Bounded Agent Builder directory, scoped to the actual requester.
use super::*;

/// One listed agent: id, slug, name, description, provider and model.
#[derive(sqlx::FromRow)]
struct ListedAgent {
    id: Uuid,
    slug: String,
    name: String,
    description: Option<String>,
    provider: Option<String>,
    model: Option<String>,
}

/// The company's agents by slug -- with their departments and desks, only one department's
/// members and role holders when `department` names it (01DEP-P03-D10) -- and its keyed models.
pub(super) async fn list_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    requester: WorkflowActor,
    arguments: &Value,
) -> AppResult<Value> {
    use crate::adapters::persistence::department_roster::{
        CompanyAddresses, agent_addresses_on, agent_departments_on,
    };
    let department = match department_on(tx, company, requester, arguments).await? {
        Ok(department) => department,
        Err(refusal) => return Ok(refusal.tool_error()),
    };
    let mut budget = MAX_LISTING_BYTES;
    let mut truncated = false;
    let mut fits = |entry: &Value| {
        let size = entry.to_string().len() + 1;
        let fit = size <= budget;
        if fit {
            budget -= size;
        }
        fit
    };
    let mut models = Vec::new();
    // Each model names the department that owns its connection: a created agent may use only its
    // department's or one above it (Plan07 02A).
    let ready: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT connection.provider, model.name, unit.name FROM company_model_connections AS connection \
         JOIN departments AS unit ON unit.company_id = connection.company_id \
          AND unit.id = connection.unit_id \
         CROSS JOIN LATERAL unnest(connection.models) WITH ORDINALITY AS model(name, position) \
         WHERE connection.company_id = $1 AND connection.api_key IS NOT NULL \
         ORDER BY (unit.is_root AND connection.default_for_unit) DESC, connection.provider, \
                  model.position",
    )
    .bind(company.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    for (provider, model, unit) in ready {
        let entry = json!({"provider": provider, "model": model, "department": unit});
        if models.len() == MAX_LISTED_MODELS || !fits(&entry) {
            truncated = true;
            break;
        }
        models.push(entry);
    }
    let rows = rows_on(tx, company, requester, department).await?;
    truncated |= rows.len() > MAX_LISTED_AGENTS;
    let rows: Vec<ListedAgent> = rows.into_iter().take(MAX_LISTED_AGENTS).collect();
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut departments = agent_departments_on(
        tx,
        company,
        &ids,
        crate::entities::department::ArchivedUnits::Exclude,
    )
    .await?;
    let mut addresses = CompanyAddresses::new(company, Some(&templates.app_domain));
    let mut desks = agent_addresses_on(tx, &mut addresses, &ids).await?;
    let name = |name: &str| name.chars().take(MAX_LISTED_NAME_CHARS).collect::<String>();
    let mut agents = Vec::new();
    for ListedAgent {
        id,
        slug,
        name: agent_name,
        description,
        provider,
        model,
    } in rows
    {
        let (placed, more) = departments.remove(&id).unwrap_or_default();
        truncated |= more;
        let entry = json!({
            "slug": slug,
            "name": name(&agent_name),
            "description": description
                .map(|text| text.chars().take(MAX_LISTED_DESCRIPTION_CHARS).collect::<String>()),
            "provider": provider, "model": model,
            "departments": placed.iter().map(|department| json!({
                "slug": department.slug,
                "name": name(&department.name),
                "roles": department.roles.iter().map(|role| role.as_str()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "address": desks.remove(&id).map(|address| json!({"principal_id": address.principal_id, "address": address.address})),
        });
        if !fits(&entry) {
            truncated = true;
            break;
        }
        agents.push(entry);
    }
    Ok(json!({"agents": agents, "models": models, "truncated": truncated}))
}

async fn department_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    requester: WorkflowActor,
    arguments: &Value,
) -> AppResult<Provisioned<Option<Uuid>>> {
    let Some(slug) = arguments.get("department").and_then(Value::as_str) else {
        return Ok(Ok(None));
    };
    let id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM departments WHERE company_id=$1 AND slug=$2 \
         AND unit_management_role($3,$1,id) IS NOT NULL",
    )
    .bind(company.as_uuid())
    .bind(slug)
    .bind(requester.user_id())
    .fetch_optional(&mut **tx)
    .await?;
    Ok(match id {
        Some(id) => Ok(Some(id)),
        None => Err(ProvisioningRefusal::Invalid(format!(
            "No department has the slug '{slug}'"
        ))),
    })
}

async fn rows_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    requester: WorkflowActor,
    department: Option<Uuid>,
) -> AppResult<Vec<ListedAgent>> {
    let rows: Vec<ListedAgent> = sqlx::query_as(
        "SELECT agent.id, agent.slug::text AS slug, agent.name, agent.description, agent.provider, \
           agent.model FROM agents AS agent \
         WHERE agent.company_id = $1 AND agent_management_role($4,$1,agent.id) IS NOT NULL \
           AND ($2::uuid IS NULL OR EXISTS ( \
           SELECT 1 FROM principals AS principal WHERE principal.company_id = agent.company_id \
             AND principal.agent_id = agent.id AND ( \
               EXISTS (SELECT 1 FROM department_members AS member \
                 WHERE member.company_id = principal.company_id AND member.department_id = $2 \
                   AND member.principal_id = principal.id) \
               OR EXISTS (SELECT 1 FROM department_roles AS role \
                 WHERE role.company_id = principal.company_id AND role.department_id = $2 \
                   AND role.principal_id = principal.id)))) \
         ORDER BY agent.slug LIMIT $3",
    )
    .bind(company.as_uuid())
    .bind(department)
    .bind(MAX_LISTED_AGENTS as i64 + 1)
    .bind(requester.user_id())
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}
