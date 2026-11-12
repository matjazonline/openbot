use super::super::workflow::row_locks::RowLocks;
use super::*;
use crate::application::workflow::{
    http::HttpOperation, mcp::McpOperation, publication::ToolSnapshot,
};
use crate::entities::mcp::{CompanyMcpConnection, McpAuth};
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

pub(in crate::adapters::persistence) struct RegisteredTool {
    pub(in crate::adapters::persistence) tool: ToolSnapshot,
    pub(in crate::adapters::persistence) approval_required: bool,
}

pub(in crate::adapters::persistence) fn ordered_bindings<'a>(
    bindings: impl IntoIterator<Item = &'a AgentToolBinding>,
) -> Vec<&'a AgentToolBinding> {
    let mut ordered: Vec<_> = bindings.into_iter().collect();
    ordered.sort_by(|left, right| source_order(left).cmp(&source_order(right)));
    ordered
}

pub(super) async fn resolve_tools(
    tx: &mut PgConnection,
    company: CompanyId,
    bindings: &[AgentToolBinding],
    native_only: bool,
) -> AppResult<Vec<ResolvedAgentTool>> {
    validate_agent_bindings(bindings).map_err(AppError::BadRequest)?;
    memory_publication_on(tx, company, bindings.iter().map(|binding| &binding.source)).await?;
    let ordered = ordered_bindings(bindings);
    let mut resolved = BTreeMap::new();
    for binding in ordered {
        let captured = match &binding.source {
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
            source => direct_registration_on(tx, company, source, native_only).await?,
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

/// An agent version with memory tools publishes only while the company has memory enabled
/// (phase 05 U1/E5). Admission and live use do not re-check: once memory is disabled a
/// published agent keeps running and each memory call answers `memory.not_enabled`.
async fn memory_publication_on<'a>(
    db: &mut PgConnection,
    company: CompanyId,
    mut sources: impl Iterator<Item = &'a AgentBindingSource>,
) -> AppResult<()> {
    let uses_memory = sources
        .any(|source| matches!(source, AgentBindingSource::Builtin { tool } if tool.is_memory()));
    if uses_memory && !crate::adapters::persistence::memory::memory_enabled_on(db, company).await? {
        return Err(AppError::NotFound(
            "Memory is not enabled for this company".into(),
        ));
    }
    Ok(())
}

/// Publication preserves hosted dispatch restrictions; native-only agents use the broker.
pub(in crate::adapters::persistence) async fn direct_registration_on(
    db: &mut PgConnection,
    company: CompanyId,
    source: &AgentBindingSource,
    native_only: bool,
) -> AppResult<RegisteredTool> {
    if !native_only && let AgentBindingSource::Mcp { connection, .. } = source {
        let current = super::super::mcp::publication_connection_with_locks_on(
            db,
            company.as_uuid(),
            *connection,
            RowLocks::Wait,
        )
        .await?
        .ok_or_else(unavailable)?;
        if matches!(current.auth, McpAuth::Integration(_)) {
            return Err(unavailable());
        }
    }
    direct_registration_with_locks_on(db, company, source, RowLocks::Wait).await
}

pub(in crate::adapters::persistence) async fn direct_registration_with_locks_on(
    db: &mut PgConnection,
    company: CompanyId,
    source: &AgentBindingSource,
    locks: RowLocks,
) -> AppResult<RegisteredTool> {
    match source {
        AgentBindingSource::Builtin { tool } => Ok(RegisteredTool {
            tool: builtin(company, *tool)?,
            approval_required: false,
        }),
        AgentBindingSource::Http {
            connection,
            operation,
        } => http_tool(db, company, *connection, operation, locks).await,
        AgentBindingSource::Mcp {
            connection,
            operation,
            reviewed_version,
            remote_name,
        } => {
            mcp_tool(
                db,
                company,
                *connection,
                operation,
                *reviewed_version,
                remote_name.as_str(),
                locks,
            )
            .await
        }
        AgentBindingSource::Workflow { .. } => Err(invalid()),
    }
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
        AgentBuiltinTool::SearchMemory
        | AgentBuiltinTool::MemoryTimeline
        | AgentBuiltinTool::MemoryDetails
        | AgentBuiltinTool::SaveMemory => {
            let memory = crate::application::workflow::memory_tools::MemoryTool::from_builtin(tool)
                .ok_or_else(|| AppError::Internal("Invalid built-in memory tool".into()))?;
            crate::application::workflow::memory_tools::builtin_memory_snapshot(company, memory)
        }
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
    locks: RowLocks,
) -> AppResult<()> {
    let query = match owner {
        CredentialOwner::Http => {
            "SELECT connection_id FROM workflow_http_credentials WHERE company_id=$1 AND connection_id=$2 FOR SHARE"
        }
        CredentialOwner::Mcp => {
            "SELECT connection_id FROM company_mcp_credentials WHERE company_id=$1 AND connection_id=$2 FOR SHARE"
        }
    };
    sqlx::query_scalar::<_, Uuid>(&locks.sql(query))
        .bind(company.as_uuid())
        .bind(connection)
        .fetch_optional(db)
        .await?
        .ok_or_else(unavailable)?;
    Ok(())
}

async fn http_tool(
    tx: &mut PgConnection,
    company: CompanyId,
    connection: Uuid,
    operation: &TypeName,
    locks: RowLocks,
) -> AppResult<RegisteredTool> {
    let current =
        super::super::workflow::http::connection_with_locks_on(tx, company, connection, locks)
            .await?
            .ok_or_else(|| {
                AppError::BadRequest("Selected HTTP connection is unavailable".into())
            })?;
    if !current.enabled {
        return Err(AppError::BadRequest(
            "Selected HTTP connection is unavailable".into(),
        ));
    }
    if matches!(
        current.credential_source,
        crate::domain::integrations::authority::CredentialSource::Integration(_)
    ) {
        return Err(unavailable());
    }
    if current.uses_owned_bearer() {
        credential_ready(tx, company, connection, CredentialOwner::Http, locks).await?;
    }
    let (snapshot, approval_required): (serde_json::Value, bool) = sqlx::query_as(&locks.sql(
        "SELECT snapshot,approval_required FROM workflow_http_operations \
         WHERE company_id=$1 AND connection_id=$2 AND name=$3 AND revoked_at IS NULL FOR SHARE",
    ))
    .bind(company.as_uuid())
    .bind(connection)
    .bind(operation.as_str())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| {
        AppError::BadRequest("Selected HTTP operation is unavailable or revoked".into())
    })?;
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

#[derive(sqlx::FromRow)]
struct ReviewedMcpOperation {
    snapshot: serde_json::Value,
    approval_required: bool,
    version: Uuid,
}

async fn mcp_credential_ready(
    tx: &mut PgConnection,
    company: CompanyId,
    current: &CompanyMcpConnection,
    locks: RowLocks,
) -> AppResult<()> {
    if let McpAuth::Integration(reference) = &current.auth {
        // Use the existing consent-bound MCP credential path. This metadata read
        // takes no integration-owner locks; dispatch rechecks the frozen grant
        // and obtains the actual credential through the integration gateway.
        super::super::workflow::integration_authority::mcp_snapshot(tx, current).await?;
        let ready = super::super::integrations::PostgresVault::mcp_reference_ready_on(
            tx,
            company.as_uuid(),
            current.id,
            current.endpoint.as_str(),
            reference,
        )
        .await?;
        if !ready {
            return Err(unavailable());
        }
    }
    if current.auth == McpAuth::Bearer {
        credential_ready(tx, company, current.id, CredentialOwner::Mcp, locks).await?;
    }
    Ok(())
}

async fn mcp_tool(
    tx: &mut PgConnection,
    company: CompanyId,
    connection: Uuid,
    operation: &TypeName,
    reviewed_version: Uuid,
    remote_name: &str,
    locks: RowLocks,
) -> AppResult<RegisteredTool> {
    let current = super::super::mcp::publication_connection_with_locks_on(
        tx,
        company.as_uuid(),
        connection,
        locks,
    )
    .await?
    .ok_or_else(|| AppError::BadRequest("Selected MCP connection is unavailable".into()))?;
    if !current.enabled {
        return Err(AppError::BadRequest(
            "Selected MCP connection is unavailable".into(),
        ));
    }
    mcp_credential_ready(tx, company, &current, locks).await?;
    sqlx::query_scalar::<_, String>(&locks.sql(
        "SELECT tool_name FROM company_mcp_tool_grants WHERE company_id=$1 AND connection_id=$2 \
         AND tool_name=$3 AND revoked_at IS NULL FOR SHARE",
    ))
    .bind(company.as_uuid())
    .bind(connection)
    .bind(remote_name)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| {
        AppError::BadRequest("Selected MCP tool permission is revoked or missing".into())
    })?;
    let reviewed: ReviewedMcpOperation = sqlx::query_as(
        &locks.sql("SELECT snapshot,approval_required,version FROM workflow_mcp_operations WHERE company_id=$1 \
         AND connection_id=$2 AND name=$3 AND revoked_at IS NULL FOR SHARE")
    ).bind(company.as_uuid()).bind(connection).bind(operation.as_str())
        .fetch_optional(&mut *tx).await?.ok_or_else(|| AppError::BadRequest("Selected MCP operation is unavailable or revoked".into()))?;
    if reviewed.version != reviewed_version {
        return Err(AppError::BadRequest(
            "Selected MCP schema review changed; select the reviewed operation again".into(),
        ));
    }
    let tool: ToolSnapshot = serde_json::from_value(reviewed.snapshot).map_err(|_| invalid())?;
    if tool.contract.name.as_str() != remote_name {
        return Err(AppError::BadRequest(
            "Selected MCP reviewed operation no longer matches its remote tool".into(),
        ));
    }
    McpOperation {
        tool: tool.clone(),
        approval_required: reviewed.approval_required,
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
        approval_required: reviewed.approval_required,
    })
}
