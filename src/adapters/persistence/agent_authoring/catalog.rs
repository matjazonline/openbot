use super::*;
use crate::application::workflow::{
    http::HttpOperation, mcp::McpOperation, publication::ToolSnapshot,
};
use crate::domain::workflow::{TypeName, VersionId, WorkflowId};
use sqlx::{Postgres, QueryBuilder};

#[derive(sqlx::FromRow)]
struct ConnectionRow {
    id: Uuid,
    name: String,
}

pub(super) async fn connections(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    kind: AgentToolKind,
    after: Option<Uuid>,
) -> AppResult<AgentConnectionPage> {
    let mut query = QueryBuilder::<Postgres>::new(match kind {
        AgentToolKind::Http => {
            "SELECT id,origin AS name FROM workflow_http_connections WHERE enabled AND company_id="
        }
        AgentToolKind::Mcp => {
            "SELECT id,slug AS name FROM company_mcp_connections WHERE enabled AND deleted_at IS NULL AND company_id="
        }
        _ => {
            return Ok(AgentConnectionPage {
                items: vec![],
                next: None,
            });
        }
    });
    query.push_bind(scope.company.as_uuid());
    if let Some(after) = after {
        query.push(" AND id>").push_bind(after);
    }
    query
        .push(" ORDER BY id LIMIT ")
        .push_bind((AGENT_CATALOG_PAGE + 1) as i64);
    let rows: Vec<ConnectionRow> = query.build_query_as().fetch_all(db).await?;
    let more = rows.len() > AGENT_CATALOG_PAGE;
    let items = rows
        .into_iter()
        .take(AGENT_CATALOG_PAGE)
        .map(|row| AgentConnectionOption {
            id: row.id,
            name: row.name,
        })
        .collect::<Vec<_>>();
    let next = if more {
        items.last().map(|item| item.id)
    } else {
        None
    };
    Ok(AgentConnectionPage { items, next })
}

#[derive(sqlx::FromRow)]
struct OperationRow {
    name: String,
    snapshot: serde_json::Value,
    approval_required: bool,
    version: Option<Uuid>,
}

async fn operations(
    db: &mut PgConnection,
    request: &AgentToolPageRequest,
    connection: Uuid,
) -> AppResult<Vec<OperationRow>> {
    let mut query = QueryBuilder::<Postgres>::new(match request.kind {
        AgentToolKind::Http => {
            "SELECT name,snapshot,approval_required,NULL::uuid AS version FROM workflow_http_operations WHERE revoked_at IS NULL AND company_id="
        }
        AgentToolKind::Mcp => {
            "SELECT name,snapshot,approval_required,version FROM workflow_mcp_operations WHERE revoked_at IS NULL AND company_id="
        }
        _ => return Err(invalid()),
    });
    query
        .push_bind(request.scope.company.as_uuid())
        .push(" AND connection_id=")
        .push_bind(connection);
    if let Some(after) = &request.after {
        query.push(" AND name>").push_bind(after.as_str());
    }
    query
        .push(" ORDER BY name LIMIT ")
        .push_bind((AGENT_CATALOG_PAGE + 1) as i64);
    Ok(query.build_query_as().fetch_all(db).await?)
}

pub(super) async fn http(
    db: &mut PgConnection,
    request: &AgentToolPageRequest,
) -> AppResult<AgentToolPage> {
    let id = request
        .connection
        .ok_or_else(|| AppError::BadRequest("Choose a company HTTP connection".into()))?;
    let enabled: bool = sqlx::query_scalar(
        "SELECT enabled FROM workflow_http_connections WHERE company_id=$1 AND id=$2",
    )
    .bind(request.scope.company.as_uuid())
    .bind(id)
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(|| AppError::NotFound("HTTP connection".into()))?;
    let rows = operations(db, request, id).await?;
    let more = rows.len() > AGENT_CATALOG_PAGE;
    let mut items = Vec::with_capacity(rows.len().min(AGENT_CATALOG_PAGE));
    for row in rows.into_iter().take(AGENT_CATALOG_PAGE) {
        tokio::task::yield_now().await;
        let name = TypeName::parse(row.name).map_err(|_| invalid())?;
        let tool: ToolSnapshot = serde_json::from_value(row.snapshot).map_err(|_| invalid())?;
        let availability = if !enabled {
            AgentToolAvailability::ConnectionUnavailable
        } else if (HttpOperation {
            tool,
            approval_required: row.approval_required,
        })
        .validate(request.scope.company)
        .is_err()
        {
            AgentToolAvailability::OperationUnavailable
        } else {
            AgentToolAvailability::Configured
        };
        items.push(AgentToolOption {
            choice: AgentToolChoice::Http {
                connection: id,
                operation: name.clone(),
            },
            name,
            availability,
        });
    }
    Ok(page(items, more))
}

pub(super) async fn mcp(
    db: &mut PgConnection,
    request: &AgentToolPageRequest,
) -> AppResult<AgentToolPage> {
    let id = request
        .connection
        .ok_or_else(|| AppError::BadRequest("Choose a company MCP connection".into()))?;
    let connection =
        super::super::mcp::publication_connection_on(db, request.scope.company.as_uuid(), id)
            .await?
            .ok_or_else(|| AppError::NotFound("MCP connection".into()))?;
    let rows = operations(db, request, id).await?;
    let more = rows.len() > AGENT_CATALOG_PAGE;
    let mut items = Vec::with_capacity(rows.len().min(AGENT_CATALOG_PAGE));
    for row in rows.into_iter().take(AGENT_CATALOG_PAGE) {
        tokio::task::yield_now().await;
        let name = TypeName::parse(row.name).map_err(|_| invalid())?;
        let tool: ToolSnapshot = serde_json::from_value(row.snapshot).map_err(|_| invalid())?;
        let remote_name =
            crate::entities::mcp::McpToolName::try_from(tool.contract.name.as_str().to_owned())
                .map_err(|_| invalid())?;
        let granted = connection
            .tool_grants
            .iter()
            .any(|grant| grant == &remote_name);
        let reviewed = connection
            .discovered_tools
            .iter()
            .find(|tool| tool.name == remote_name);
        let availability = if !connection.enabled {
            AgentToolAvailability::ConnectionUnavailable
        } else if !granted {
            AgentToolAvailability::GrantRequired
        } else if (McpOperation {
            tool,
            approval_required: row.approval_required,
        })
        .validate(request.scope.company, reviewed, granted)
        .is_err()
        {
            AgentToolAvailability::ReviewChanged
        } else {
            AgentToolAvailability::Configured
        };
        items.push(AgentToolOption {
            choice: AgentToolChoice::Mcp {
                connection: id,
                operation: name.clone(),
                reviewed_version: row.version.ok_or_else(invalid)?,
                remote_name,
            },
            name,
            availability,
        });
    }
    Ok(page(items, more))
}

#[derive(sqlx::FromRow)]
struct WorkflowRow {
    name: String,
    workflow_id: Uuid,
    version_id: Uuid,
    params: serde_json::Value,
}

pub(super) async fn workflows(
    db: &mut PgConnection,
    request: &AgentToolPageRequest,
) -> AppResult<AgentToolPage> {
    let mut query = QueryBuilder::<Postgres>::new(
        "SELECT name,workflow_id,version_id,params FROM workflow_tool_registrations WHERE revoked_at IS NULL AND company_id=",
    );
    query.push_bind(request.scope.company.as_uuid());
    if let Some(after) = &request.after {
        query.push(" AND name>").push_bind(after.as_str());
    }
    query
        .push(" ORDER BY name LIMIT ")
        .push_bind((AGENT_CATALOG_PAGE + 1) as i64);
    let rows: Vec<WorkflowRow> = query.build_query_as().fetch_all(db).await?;
    let more = rows.len() > AGENT_CATALOG_PAGE;
    let items = rows
        .into_iter()
        .take(AGENT_CATALOG_PAGE)
        .map(|row| {
            let name = TypeName::parse(row.name).map_err(|_| invalid())?;
            Ok(AgentToolOption {
                choice: AgentToolChoice::Workflow {
                    registration: name.clone(),
                    workflow: WorkflowId::new(row.workflow_id),
                    version: VersionId::new(row.version_id),
                    params_digest: AgentParameterDigest::of(&row.params)?,
                },
                name,
                availability: AgentToolAvailability::Configured,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(page(items, more))
}

/// The built-ins an author can bind; the memory tools only while the company has memory
/// enabled (phase 05 E5), since publication refuses them otherwise.
pub(super) fn builtins(memory_enabled: bool) -> AppResult<AgentToolPage> {
    let memory = [
        AgentBuiltinTool::SearchMemory,
        AgentBuiltinTool::MemoryTimeline,
        AgentBuiltinTool::MemoryDetails,
        AgentBuiltinTool::SaveMemory,
    ];
    let items = [
        AgentBuiltinTool::MessageReply,
        AgentBuiltinTool::MessageSend,
        AgentBuiltinTool::SearchKnowledge,
        AgentBuiltinTool::ReadEscalation,
        AgentBuiltinTool::RespondToEscalation,
    ]
    .into_iter()
    .chain(memory.into_iter().filter(|_| memory_enabled))
    .map(|tool| {
        Ok(AgentToolOption {
            name: TypeName::parse(tool.name()).map_err(|_| invalid())?,
            choice: AgentToolChoice::Builtin { tool },
            availability: AgentToolAvailability::Configured,
        })
    })
    .collect::<AppResult<Vec<_>>>()?;
    Ok(AgentToolPage { items, next: None })
}

fn page(items: Vec<AgentToolOption>, more: bool) -> AgentToolPage {
    let next = if more {
        items.last().map(|item| item.name.clone())
    } else {
        None
    };
    AgentToolPage { items, next }
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    /// The memory tools are offered only while the company has memory enabled (E5).
    #[test]
    fn memory_scope_catalog_offers_memory_tools_only_while_enabled() {
        let names = |enabled| -> Vec<String> {
            builtins(enabled)
                .unwrap()
                .items
                .into_iter()
                .map(|item| item.name.as_str().to_owned())
                .collect()
        };
        let memory = [
            "search_memory",
            "memory_timeline",
            "memory_details",
            "save_memory",
        ];
        assert!(
            names(false)
                .iter()
                .all(|name| !memory.contains(&name.as_str()))
        );
        let enabled = names(true);
        assert!(
            memory
                .iter()
                .all(|name| enabled.iter().any(|item| item == name))
        );
        assert_eq!(enabled.len(), names(false).len() + 4);
    }
}
