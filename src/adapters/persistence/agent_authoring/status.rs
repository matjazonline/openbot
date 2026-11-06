use super::*;
use std::collections::BTreeMap;

#[derive(sqlx::FromRow)]
struct StatusRow {
    id: Uuid,
    access: String,
    availability: String,
}

const STATUS_SQL: &str = r#"
SELECT binding.id,
    CASE WHEN live_grant.binding_id IS NULL THEN 'unpublished'
         WHEN live_grant.revoked_at IS NULL THEN 'live' ELSE 'revoked' END AS access,
    CASE
        WHEN binding.kind='http' AND (http_connection.id IS NULL OR NOT http_connection.enabled)
            THEN 'connection_unavailable'
        WHEN binding.kind='mcp' AND (mcp_connection.id IS NULL OR NOT mcp_connection.enabled OR mcp_connection.deleted_at IS NOT NULL)
            THEN 'connection_unavailable'
        WHEN binding.kind='mcp' AND (mcp_grant.tool_name IS NULL OR mcp_grant.revoked_at IS NOT NULL)
            THEN 'grant_required'
        WHEN binding.kind='mcp' AND mcp_operation.version IS DISTINCT FROM binding.reviewed_version
            THEN 'review_changed'
        WHEN binding.kind='http' AND (http_operation.name IS NULL OR http_operation.revoked_at IS NOT NULL)
            THEN 'operation_unavailable'
        WHEN binding.kind='mcp' AND (mcp_operation.name IS NULL OR mcp_operation.revoked_at IS NOT NULL)
            THEN 'operation_unavailable'
        WHEN binding.kind='workflow' AND (registration.name IS NULL OR registration.revoked_at IS NOT NULL OR registration.workflow_id IS DISTINCT FROM binding.workflow_id)
            THEN 'operation_unavailable'
        WHEN binding.kind='workflow' AND (registration.version_id IS DISTINCT FROM binding.version_id OR registration.params IS DISTINCT FROM binding.params)
            THEN 'historical_pin'
        ELSE 'configured' END AS availability
FROM agent_capability_bindings AS binding
LEFT JOIN agent_binding_grants AS live_grant
    ON live_grant.company_id=binding.company_id AND live_grant.agent_id=binding.agent_id AND live_grant.binding_id=binding.id
LEFT JOIN workflow_http_connections AS http_connection
    ON http_connection.company_id=binding.company_id AND http_connection.id=binding.http_connection_id
LEFT JOIN workflow_http_operations AS http_operation
    ON http_operation.company_id=binding.company_id AND http_operation.connection_id=binding.http_connection_id AND http_operation.name=binding.operation_name
LEFT JOIN company_mcp_connections AS mcp_connection
    ON mcp_connection.company_id=binding.company_id AND mcp_connection.id=binding.mcp_connection_id
LEFT JOIN workflow_mcp_operations AS mcp_operation
    ON mcp_operation.company_id=binding.company_id AND mcp_operation.connection_id=binding.mcp_connection_id AND mcp_operation.name=binding.operation_name
LEFT JOIN company_mcp_tool_grants AS mcp_grant
    ON mcp_grant.company_id=binding.company_id AND mcp_grant.connection_id=binding.mcp_connection_id AND mcp_grant.tool_name=binding.remote_name
LEFT JOIN workflow_tool_registrations AS registration
    ON registration.company_id=binding.company_id AND registration.name=binding.workflow_registration
WHERE binding.company_id=$1 AND binding.agent_id=$2 AND binding.id=ANY($3)
ORDER BY binding.id
LIMIT 65
"#;

pub(super) struct ToolStatus {
    pub access: AgentGrantState,
    pub availability: AgentToolAvailability,
}

pub(super) async fn tools(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    ids: &[Uuid],
) -> AppResult<BTreeMap<Uuid, ToolStatus>> {
    if ids.len() > 64 {
        return Err(invalid());
    }
    let rows: Vec<StatusRow> = sqlx::query_as(STATUS_SQL)
        .bind(scope.company.as_uuid())
        .bind(scope.agent.as_uuid())
        .bind(ids)
        .fetch_all(db)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.id,
                ToolStatus {
                    access: access(&row.access)?,
                    availability: availability(&row.availability)?,
                },
            ))
        })
        .collect()
}

fn availability(value: &str) -> AppResult<AgentToolAvailability> {
    Ok(match value {
        "configured" => AgentToolAvailability::Configured,
        "connection_unavailable" => AgentToolAvailability::ConnectionUnavailable,
        "grant_required" => AgentToolAvailability::GrantRequired,
        "review_changed" => AgentToolAvailability::ReviewChanged,
        "operation_unavailable" => AgentToolAvailability::OperationUnavailable,
        "historical_pin" => AgentToolAvailability::HistoricalPin,
        _ => return Err(invalid()),
    })
}

pub(super) fn access(value: &str) -> AppResult<AgentGrantState> {
    Ok(match value {
        "unpublished" => AgentGrantState::Unpublished,
        "live" => AgentGrantState::Live,
        "revoked" => AgentGrantState::Revoked,
        _ => return Err(invalid()),
    })
}

pub(super) fn project(
    binding: &AgentToolBinding,
    states: &BTreeMap<Uuid, ToolStatus>,
) -> AppResult<AgentToolState> {
    let state = states.get(&binding.id.as_uuid()).ok_or_else(invalid)?;
    Ok(AgentToolState {
        assignment: AgentToolAssignment {
            binding: Some(binding.id),
            alias: binding.alias.clone(),
            choice: AgentToolChoice::from_source(&binding.source)?,
        },
        access: state.access,
        availability: state.availability,
    })
}
