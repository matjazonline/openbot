//! Native storage for one agent's ordered draft bindings. Retained identities never retarget.
use crate::app_error::{AppError, AppResult};
use crate::domain::entities::agent_capability::{
    AgentBindingSource, AgentToolBinding, validate_agent_bindings,
};
use serde_json::Value;
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

pub(super) async fn bindings_on(
    db: &mut PgConnection,
    company: Option<Uuid>,
    agent: Uuid,
) -> AppResult<Vec<AgentToolBinding>> {
    let values: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',binding.id,'alias',binding.alias,'source',public.agent_binding_source(binding))
         FROM agent_draft_bindings AS assigned JOIN agent_capability_bindings AS binding ON binding.id=assigned.binding_id
         WHERE assigned.company_id IS NOT DISTINCT FROM $1 AND assigned.agent_id=$2 ORDER BY assigned.position LIMIT 33",
    )
    .bind(company).bind(agent).fetch_all(db).await?;
    let bindings: Vec<AgentToolBinding> = values
        .into_iter()
        .map(|value| serde_json::from_value(value).map_err(|_| malformed()))
        .collect::<AppResult<_>>()?;
    validate_agent_bindings(&bindings).map_err(|_| malformed())?;
    Ok(bindings)
}

fn malformed() -> AppError {
    AppError::Internal("Invalid stored agent capability binding".into())
}

pub(in crate::adapters::persistence) fn relationship_error(error: sqlx::Error) -> AppError {
    match error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
    {
        Some("23503" | "23514") => AppError::BadRequest(
            "Select a valid tool target belonging to this agent's company".into(),
        ),
        _ => AppError::from(error),
    }
}

/// The caller holds the owning agent FOR UPDATE for the complete draft write.
pub(super) async fn replace_bindings_on(
    tx: &mut Transaction<'_, Postgres>,
    company: Option<Uuid>,
    agent: Uuid,
    bindings: &[AgentToolBinding],
) -> AppResult<()> {
    validate_agent_bindings(bindings).map_err(AppError::BadRequest)?;
    for binding in bindings {
        insert_identity_on(tx, company, agent, binding).await?;
    }
    sqlx::query("DELETE FROM agent_draft_bindings WHERE agent_id=$1")
        .bind(agent)
        .execute(&mut **tx)
        .await?;
    for (position, binding) in bindings.iter().enumerate() {
        sqlx::query("INSERT INTO agent_draft_bindings(company_id,agent_id,binding_id,alias,position) VALUES($1,$2,$3,$4,$5)")
            .bind(company).bind(agent).bind(binding.id.as_uuid()).bind(binding.alias.as_str())
            .bind(position as i16).execute(&mut **tx).await.map_err(relationship_error)?;
    }
    Ok(())
}

async fn insert_identity_on(
    tx: &mut Transaction<'_, Postgres>,
    company: Option<Uuid>,
    agent: Uuid,
    binding: &AgentToolBinding,
) -> AppResult<()> {
    let source = serde_json::to_value(&binding.source).map_err(|_| malformed())?;
    sqlx::query(
        "INSERT INTO agent_capability_bindings(company_id,agent_id,id,alias,kind,builtin,http_connection_id,mcp_connection_id,
            operation_name,reviewed_version,remote_name,workflow_registration,workflow_id,version_id,params)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) ON CONFLICT(id) DO NOTHING",
    )
    .bind(company).bind(agent).bind(binding.id.as_uuid()).bind(binding.alias.as_str())
    .bind(source["kind"].as_str()).bind(source["tool"].as_str())
    .bind(match &binding.source { AgentBindingSource::Http { connection, .. } => Some(*connection), _ => None })
    .bind(match &binding.source { AgentBindingSource::Mcp { connection, .. } => Some(*connection), _ => None })
    .bind(source["operation"].as_str())
    .bind(match &binding.source { AgentBindingSource::Mcp { reviewed_version, .. } => Some(*reviewed_version), _ => None })
    .bind(source["remote_name"].as_str()).bind(source["registration"].as_str())
    .bind(match &binding.source { AgentBindingSource::Workflow { workflow, .. } => Some(workflow.as_uuid()), _ => None })
    .bind(match &binding.source { AgentBindingSource::Workflow { version, .. } => Some(version.as_uuid()), _ => None })
    .bind(match &binding.source { AgentBindingSource::Workflow { params, .. } => Some(params), _ => None })
    .execute(&mut **tx).await.map_err(relationship_error)?;
    let same: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_capability_bindings AS binding WHERE id=$1 AND agent_id=$2
         AND company_id IS NOT DISTINCT FROM $3 AND alias=$4 AND public.agent_binding_source(binding)=$5)",
    )
    .bind(binding.id.as_uuid()).bind(agent).bind(company).bind(binding.alias.as_str()).bind(source)
    .fetch_one(&mut **tx).await?;
    if !same {
        return Err(AppError::Conflict(
            "A binding identity cannot change its owner, alias or target".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) async fn assign_test_tool_names(
    persistence: &super::PostgresPersistence,
    agent: Uuid,
    names: &[&str],
) -> AppResult<()> {
    let mut tx = persistence.pool().begin().await?;
    let company: Option<Uuid> =
        sqlx::query_scalar("SELECT company_id FROM agents WHERE id=$1 FOR UPDATE")
            .bind(agent)
            .fetch_one(&mut *tx)
            .await
            .map_err(relationship_error)?;
    let names = names
        .iter()
        .map(crate::domain::workflow::TypeName::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| malformed())?;
    let bindings = match company {
        Some(company) => {
            super::agent_binding_selection::resolve_bindings_on(
                &mut tx,
                company,
                Some(agent),
                &names,
            )
            .await?
        }
        None => names
            .iter()
            .map(|name| {
                crate::entities::agent_capability::AgentBuiltinTool::parse(name.as_str())
                    .map(AgentToolBinding::builtin)
                    .ok_or_else(malformed)
            })
            .collect::<AppResult<Vec<_>>>()?,
    };
    replace_bindings_on(&mut tx, company, agent, &bindings).await?;
    sqlx::query("UPDATE agents SET draft_revision=draft_revision+1 WHERE id=$1")
        .bind(agent)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
