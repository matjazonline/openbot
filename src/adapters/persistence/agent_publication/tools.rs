use super::*;
use crate::application::workflow::{
    http::HttpOperation, mcp::McpOperation, publication::ToolSnapshot,
};
use crate::entities::mcp::McpAuth;
use std::collections::BTreeMap;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct SourceOrder<'a> {
    kind: u8,
    connection: Uuid,
    operation: &'a str,
}
fn source_order(binding: &AgentToolBinding) -> SourceOrder<'_> {
    match &binding.source {
        AgentBindingSource::Builtin { tool } => SourceOrder {
            kind: 0,
            connection: Uuid::nil(),
            operation: tool.name(),
        },
        AgentBindingSource::Http {
            connection,
            operation,
        } => SourceOrder {
            kind: 1,
            connection: *connection,
            operation: operation.as_str(),
        },
        AgentBindingSource::Mcp {
            connection,
            operation,
            ..
        } => SourceOrder {
            kind: 2,
            connection: *connection,
            operation: operation.as_str(),
        },
        AgentBindingSource::Workflow {
            registration,
            workflow,
            ..
        } => SourceOrder {
            kind: 3,
            connection: workflow.as_uuid(),
            operation: registration.as_str(),
        },
    }
}

struct RegisteredTool {
    tool: ToolSnapshot,
    approval_required: bool,
}

pub(super) async fn resolve_tools(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    bindings: &[AgentToolBinding],
) -> AppResult<Vec<ResolvedAgentTool>> {
    validate_agent_bindings(bindings).map_err(AppError::BadRequest)?;
    let mut ordered: Vec<_> = bindings.iter().collect();
    ordered.sort_by(|left, right| source_order(left).cmp(&source_order(right)));
    let mut resolved = BTreeMap::new();
    for binding in ordered {
        let captured = match &binding.source {
            AgentBindingSource::Builtin { tool } => RegisteredTool {
                tool: builtin(company, *tool)?,
                approval_required: false,
            },
            AgentBindingSource::Http {
                connection,
                operation,
            } => http_tool(tx, company, *connection, operation).await?,
            AgentBindingSource::Mcp {
                connection,
                operation,
                reviewed_version,
                remote_name,
            } => {
                mcp_tool(
                    tx,
                    company,
                    *connection,
                    operation,
                    *reviewed_version,
                    remote_name.as_str(),
                )
                .await?
            }
            AgentBindingSource::Workflow {
                registration,
                workflow,
                version,
                params,
            } => {
                let snapshot = super::super::workflow::workflow_tools::workflow_tool_snapshot(
                    tx,
                    company,
                    registration,
                )
                .await?
                .ok_or_else(unavailable)?;
                if snapshot.workflow.as_ref().is_none_or(|pin| {
                    pin.workflow_id != *workflow
                        || pin.version_id != *version
                        || pin.params != *params
                }) {
                    return Err(unavailable());
                }
                RegisteredTool {
                    tool: snapshot,
                    approval_required: false,
                }
            }
        };
        resolved.insert(
            binding.id,
            resolved_registration(company, binding, captured)?,
        );
    }
    bindings
        .iter()
        .map(|binding| resolved.remove(&binding.id).ok_or_else(invalid))
        .collect()
}

fn resolved_registration(
    company: CompanyId,
    binding: &AgentToolBinding,
    captured: RegisteredTool,
) -> AppResult<ResolvedAgentTool> {
    let snapshot = captured.tool;
    if snapshot.company_id != company {
        return Err(invalid());
    }
    Ok(ResolvedAgentTool {
        binding: binding.clone(),
        input_schema: snapshot.contract.input_schema,
        output_schema: snapshot.contract.output_schema,
        policy: snapshot.policy,
        approval_required: captured.approval_required,
    })
}

fn builtin(company: CompanyId, tool: AgentBuiltinTool) -> AppResult<ToolSnapshot> {
    use crate::application::workflow::{knowledge, message, provisioning};
    match tool {
        AgentBuiltinTool::MessageSend => {
            message::builtin_message_snapshot(company, message::MessageKind::Send)
        }
        AgentBuiltinTool::MessageReply => {
            message::builtin_message_snapshot(company, message::MessageKind::Reply)
        }
        AgentBuiltinTool::ListCompanyAgents => provisioning::builtin_provisioning_snapshot(
            company,
            provisioning::ProvisioningKind::ListAgents,
        ),
        AgentBuiltinTool::CreateAgentChannel => provisioning::builtin_provisioning_snapshot(
            company,
            provisioning::ProvisioningKind::CreateAgentChannel,
        ),
        AgentBuiltinTool::SearchKnowledge => knowledge::builtin_knowledge_snapshot(company),
    }
}

#[derive(Clone, Copy)]
enum CredentialOwner {
    Http,
    Mcp,
}

async fn credential_ready(
    db: &mut PgConnection,
    company: CompanyId,
    connection: Uuid,
    owner: CredentialOwner,
) -> AppResult<()> {
    let query = match owner {
        CredentialOwner::Http => {
            "SELECT connection_id FROM workflow_http_credentials WHERE company_id=$1 AND connection_id=$2 FOR SHARE"
        }
        CredentialOwner::Mcp => {
            "SELECT connection_id FROM company_mcp_credentials WHERE company_id=$1 AND connection_id=$2 FOR SHARE"
        }
    };
    sqlx::query_scalar::<_, Uuid>(query)
        .bind(company.as_uuid())
        .bind(connection)
        .fetch_optional(db)
        .await?
        .ok_or_else(unavailable)?;
    Ok(())
}

async fn http_tool(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    connection: Uuid,
    operation: &TypeName,
) -> AppResult<RegisteredTool> {
    let current = super::super::workflow::http::connection_on(tx, company, connection)
        .await?
        .ok_or_else(unavailable)?;
    if !current.enabled {
        return Err(unavailable());
    }
    if current.bearer {
        credential_ready(tx, company, connection, CredentialOwner::Http).await?;
    }
    let (snapshot, approval_required): (serde_json::Value, bool) = sqlx::query_as(
        "SELECT snapshot,approval_required FROM workflow_http_operations \
         WHERE company_id=$1 AND connection_id=$2 AND name=$3 AND revoked_at IS NULL FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(connection)
    .bind(operation.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(unavailable)?;
    let tool: ToolSnapshot = serde_json::from_value(snapshot).map_err(|_| invalid())?;
    HttpOperation {
        tool: tool.clone(),
        approval_required,
    }
    .validate(company)?;
    Ok(RegisteredTool {
        tool,
        approval_required,
    })
}

async fn mcp_tool(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    connection: Uuid,
    operation: &TypeName,
    reviewed_version: Uuid,
    remote_name: &str,
) -> AppResult<RegisteredTool> {
    let current = super::super::mcp::publication_connection_on(tx, company.as_uuid(), connection)
        .await?
        .ok_or_else(unavailable)?;
    if !current.enabled {
        return Err(unavailable());
    }
    if current.auth == McpAuth::Bearer {
        credential_ready(tx, company, connection, CredentialOwner::Mcp).await?;
    }
    sqlx::query_scalar::<_, String>(
        "SELECT tool_name FROM company_mcp_tool_grants WHERE company_id=$1 AND connection_id=$2 \
         AND tool_name=$3 AND revoked_at IS NULL FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(connection)
    .bind(remote_name)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(unavailable)?;
    let (snapshot, approval_required, version): (serde_json::Value, bool, Uuid) = sqlx::query_as(
        "SELECT snapshot,approval_required,version FROM workflow_mcp_operations WHERE company_id=$1 \
         AND connection_id=$2 AND name=$3 AND revoked_at IS NULL FOR SHARE"
    ).bind(company.as_uuid()).bind(connection).bind(operation.as_str())
        .fetch_optional(&mut **tx).await?.ok_or_else(unavailable)?;
    if version != reviewed_version {
        return Err(unavailable());
    }
    let tool: ToolSnapshot = serde_json::from_value(snapshot).map_err(|_| invalid())?;
    if tool.contract.name.as_str() != remote_name {
        return Err(unavailable());
    }
    McpOperation {
        tool: tool.clone(),
        approval_required,
    }
    .validate(
        company,
        current
            .discovered_tools
            .iter()
            .find(|tool| tool.name.as_str() == remote_name),
        current
            .tool_grants
            .iter()
            .any(|name| name.as_str() == remote_name),
    )?;
    Ok(RegisteredTool {
        tool,
        approval_required,
    })
}
