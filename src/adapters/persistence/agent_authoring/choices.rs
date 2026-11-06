use super::*;

pub(super) async fn resolve(
    db: &mut PgConnection,
    company: Uuid,
    previous: &[AgentToolBinding],
    assignments: &[AgentToolAssignment],
) -> AppResult<Vec<AgentToolBinding>> {
    if assignments.len() > MAX_AGENT_BINDINGS {
        return Err(AppError::BadRequest(
            "An agent may have at most 32 tools".into(),
        ));
    }
    let mut result = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        let existing = assignment
            .binding
            .map(|id| {
                previous
                    .iter()
                    .find(|binding| binding.id == id)
                    .ok_or_else(|| {
                        AppError::BadRequest(
                            "Selected binding is not part of this agent draft".into(),
                        )
                    })
            })
            .transpose()?;
        if let Some(binding) = existing
            && binding.alias == assignment.alias
            && AgentToolChoice::from_source(&binding.source)? == assignment.choice
        {
            result.push(binding.clone());
            continue;
        }
        let source = current_source(db, company, &assignment.choice).await?;
        if matches!(&source,AgentBindingSource::Builtin {tool} if tool.is_provisioning()) {
            return Err(AppError::BadRequest("Builder provisioning capabilities may only be kept or removed; they cannot be added".into()));
        }
        result.push(AgentToolBinding {
            id: AgentBindingId::new(Uuid::new_v4()),
            alias: assignment.alias.clone(),
            source,
        });
    }
    validate_agent_bindings(&result).map_err(AppError::BadRequest)?;
    Ok(result)
}

async fn current_source(
    db: &mut PgConnection,
    company: Uuid,
    choice: &AgentToolChoice,
) -> AppResult<AgentBindingSource> {
    let source = match choice {
        AgentToolChoice::Builtin { tool } => AgentBindingSource::Builtin { tool: *tool },
        AgentToolChoice::Http {
            connection,
            operation,
        } => {
            let available: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_http_connections AS connection JOIN workflow_http_operations AS operation ON operation.company_id=connection.company_id AND operation.connection_id=connection.id WHERE connection.company_id=$1 AND connection.id=$2 AND connection.enabled AND operation.name=$3 AND operation.revoked_at IS NULL)")
                .bind(company).bind(connection).bind(operation.as_str()).fetch_one(&mut *db).await?;
            if !available {
                return Err(unavailable("HTTP operation"));
            }
            AgentBindingSource::Http {
                connection: *connection,
                operation: operation.clone(),
            }
        }
        AgentToolChoice::Mcp {
            connection,
            operation,
            reviewed_version,
            remote_name,
        } => {
            let available: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM company_mcp_connections AS connection JOIN workflow_mcp_operations AS operation ON operation.company_id=connection.company_id AND operation.connection_id=connection.id JOIN company_mcp_tool_grants AS grant_row ON grant_row.company_id=operation.company_id AND grant_row.connection_id=operation.connection_id AND grant_row.tool_name=$5 AND grant_row.revoked_at IS NULL WHERE connection.company_id=$1 AND connection.id=$2 AND connection.enabled AND connection.deleted_at IS NULL AND operation.name=$3 AND operation.version=$4 AND operation.revoked_at IS NULL AND operation.snapshot->'contract'->>'name'=$5)")
                .bind(company).bind(connection).bind(operation.as_str()).bind(reviewed_version).bind(remote_name.as_str()).fetch_one(&mut *db).await?;
            if !available {
                return Err(unavailable("Reviewed MCP operation"));
            }
            AgentBindingSource::Mcp {
                connection: *connection,
                operation: operation.clone(),
                reviewed_version: *reviewed_version,
                remote_name: remote_name.clone(),
            }
        }
        AgentToolChoice::Workflow {
            registration,
            workflow,
            version,
            ..
        } => {
            let params: Option<serde_json::Value> = sqlx::query_scalar("SELECT params FROM workflow_tool_registrations WHERE company_id=$1 AND name=$2 AND workflow_id=$3 AND version_id=$4 AND revoked_at IS NULL")
                .bind(company).bind(registration.as_str()).bind(workflow.as_uuid()).bind(version.as_uuid()).fetch_optional(&mut *db).await?;
            AgentBindingSource::Workflow {
                registration: registration.clone(),
                workflow: *workflow,
                version: *version,
                params: params.ok_or_else(|| unavailable("Workflow registration"))?,
            }
        }
    };
    if AgentToolChoice::from_source(&source)? != *choice {
        return Err(unavailable("Tool selection"));
    }
    Ok(source)
}

fn unavailable(kind: &str) -> AppError {
    AppError::BadRequest(format!(
        "{kind} is unavailable or changed. Select its current company target again."
    ))
}
