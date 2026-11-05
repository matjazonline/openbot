//! Resolve the existing grant editor's selections into concrete draft targets.
use crate::app_error::{AppError, AppResult};
use crate::domain::entities::agent_capability::{
    AgentBindingId, AgentBindingSource, AgentBuiltinTool, AgentToolAlias, AgentToolBinding,
};
use crate::domain::workflow::TypeName;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

pub(super) async fn resolve_bindings_on(
    db: &mut PgConnection,
    company: Uuid,
    agent: Option<Uuid>,
    names: &[TypeName],
) -> AppResult<Vec<AgentToolBinding>> {
    if names.len() > 32 {
        return Err(AppError::BadRequest(
            "An agent may have at most 32 bindings".into(),
        ));
    }
    let previous = match agent {
        Some(agent) => super::agent_capability::bindings_on(db, Some(company), agent).await?,
        None => Vec::new(),
    };
    let mut bindings = Vec::with_capacity(names.len());
    for name in names {
        let source = match AgentBuiltinTool::parse(name.as_str()) {
            Some(tool) => AgentBindingSource::Builtin { tool },
            None => registered_source_on(db, company, name).await?,
        };
        let binding = previous
            .iter()
            .find(|binding| binding.alias.as_str() == name.as_str() && binding.source == source)
            .cloned()
            .unwrap_or(AgentToolBinding {
                id: AgentBindingId::new(Uuid::new_v4()),
                alias: AgentToolAlias::parse(name.as_str()).map_err(AppError::BadRequest)?,
                source,
            });
        bindings.push(binding);
    }
    crate::entities::agent_capability::validate_agent_bindings(&bindings)
        .map_err(AppError::BadRequest)?;
    Ok(bindings)
}

async fn registered_source_on(
    db: &mut PgConnection,
    company: Uuid,
    name: &TypeName,
) -> AppResult<AgentBindingSource> {
    let sources: Vec<Value> = sqlx::query_scalar(
        "SELECT source FROM (
            SELECT jsonb_build_object('kind','http','connection',operation.connection_id,'operation',operation.name) AS source
            FROM workflow_http_operations AS operation JOIN workflow_http_connections AS connection
                ON connection.company_id=operation.company_id AND connection.id=operation.connection_id
            WHERE operation.company_id=$1 AND operation.name=$2 AND operation.revoked_at IS NULL AND connection.enabled
            UNION ALL
            SELECT jsonb_build_object('kind','mcp','connection',operation.connection_id,'operation',operation.name,
                'reviewed_version',operation.version,'remote_name',operation.name)
            FROM workflow_mcp_operations AS operation JOIN company_mcp_connections AS connection
                ON connection.company_id=operation.company_id AND connection.id=operation.connection_id
            WHERE operation.company_id=$1 AND operation.name=$2 AND operation.revoked_at IS NULL AND connection.enabled
                AND connection.deleted_at IS NULL
            UNION ALL
            SELECT jsonb_build_object('kind','workflow','registration',registration.name,'workflow',registration.workflow_id,
                'version',registration.version_id,'params',registration.params)
            FROM workflow_tool_registrations AS registration
            WHERE registration.company_id=$1 AND registration.name=$2 AND registration.revoked_at IS NULL
        ) AS selections LIMIT 2",
    ).bind(company).bind(name.as_str()).fetch_all(db).await?;
    match sources.as_slice() {
        [source] => serde_json::from_value(source.clone())
            .map_err(|_| AppError::Internal("Invalid registered agent tool target".into())),
        _ => Err(AppError::BadRequest(
            "Select one existing, unambiguous company tool target".into(),
        )),
    }
}
