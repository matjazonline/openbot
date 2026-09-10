//! Run preparation and dispatch policy, independent of rmcp and Rig. Step 6 supplies the durable
//! journal; requiring it here prevents simulation or a new caller from gaining unjournaled effects.
use super::{
    harness::{
        ToolInvocation,
        mcp::{HarnessMcpToolHost, McpToolDeclaration},
    },
    mcp_client::McpClient,
};
use crate::{
    app_error::{AppError, AppResult},
    entities::{harness::HarnessKind, mcp::*},
    use_cases::mcp::{McpCredentialPersistence, McpPersistence},
};
use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

pub fn fingerprint(schema: &Value) -> String {
    format!("{:x}", Sha256::digest(schema.to_string().as_bytes()))
}

#[derive(Clone)]
pub struct McpRunScope {
    pub company_id: Uuid,
    pub agent_id: Uuid,
    pub task_id: Uuid,
    pub run_id: Uuid,
}
/// The journal must atomically check lease, selection/definition/credential revisions and current
/// grants while claiming a stable invocation ID. Unknown/in-flight effects never return Execute.
#[async_trait]
pub trait McpInvocationJournal: Send + Sync {
    /// Return only a committed receipt matching the same tenant/run/invocation and arguments.
    async fn completed(
        &self,
        scope: &McpRunScope,
        declaration: &McpToolDeclaration,
        invocation_id: &str,
        arguments: &Value,
    ) -> AppResult<Option<Value>>;
    async fn claim(
        &self,
        scope: &McpRunScope,
        declaration: &McpToolDeclaration,
        invocation_id: &str,
        arguments: &Value,
    ) -> AppResult<McpClaim>;
    async fn complete(
        &self,
        scope: &McpRunScope,
        invocation_id: &str,
        result: &Value,
    ) -> AppResult<()>;
    async fn indeterminate(&self, scope: &McpRunScope, invocation_id: &str) -> AppResult<()>;
}
pub enum McpClaim {
    Execute,
    Completed(Value),
}

pub struct McpRuntime {
    catalog: Arc<dyn McpPersistence>,
    credentials: Arc<dyn McpCredentialPersistence>,
    client: Arc<dyn McpClient>,
}
impl McpRuntime {
    pub fn new(
        catalog: Arc<dyn McpPersistence>,
        credentials: Arc<dyn McpCredentialPersistence>,
        client: Arc<dyn McpClient>,
    ) -> Self {
        Self {
            catalog,
            credentials,
            client,
        }
    }
    pub async fn prepare(
        self: &Arc<Self>,
        scope: McpRunScope,
        harness: HarnessKind,
        journal: Arc<dyn McpInvocationJournal>,
    ) -> AppResult<Arc<dyn HarnessMcpToolHost>> {
        let selection = self
            .catalog
            .agent_mcp_selection(scope.company_id, scope.agent_id)
            .await?;
        let connections = self
            .catalog
            .mcp_connections_by_ids(scope.company_id, &selection.connection_ids)
            .await?;
        let mut declarations = Vec::new();
        for connection in connections.iter().filter(|c| {
            selection.connection_ids.contains(&c.id) && c.enabled && !c.tool_grants.is_empty()
        }) {
            if harness != HarnessKind::Rig {
                return Err(invalid("Enabled MCP grants require Rig"));
            }
            let session = self.connect(connection).await?;
            let discovery = session.discover().await;
            let closed = session.close().await;
            let discovered = discovery?;
            closed?;
            for name in &connection.tool_grants {
                let saved = connection
                    .discovered_tools
                    .iter()
                    .find(|t| &t.name == name)
                    .ok_or_else(|| invalid("MCP grant is not discovered"))?;
                validate_discovered(saved, &discovered)?;
                declarations.push(McpToolDeclaration {
                    identity: McpToolRef {
                        connection_id: connection.id,
                        name: name.clone(),
                    },
                    description: saved.description.clone(),
                    input_schema: saved.input_schema.clone(),
                    definition_revision: connection.revision,
                    credential_revision: connection.credential_revision,
                    selection_revision: selection.revision,
                });
            }
        }
        if declarations.len() > MAX_EFFECTIVE_MCP_TOOLS {
            return Err(invalid("MCP effective tool limit exceeded"));
        }
        Ok(Arc::new(RunHost {
            runtime: self.clone(),
            scope,
            declarations,
            journal,
        }))
    }
    async fn connect(
        &self,
        connection: &CompanyMcpConnection,
    ) -> AppResult<Box<dyn super::mcp_client::McpSession>> {
        let token = self
            .credentials
            .mcp_token(connection.company_id, connection.id, connection.revision)
            .await?;
        if connection.auth == McpAuth::Bearer && token.is_none() {
            return Err(invalid("MCP credential is missing"));
        }
        self.client.connect(&connection.endpoint, token).await
    }
}
fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
fn validate_discovered(saved: &McpDiscoveredTool, tools: &[McpDiscoveredTool]) -> AppResult<()> {
    if !tools.iter().any(|t| {
        t.name == saved.name && fingerprint(&t.input_schema) == fingerprint(&saved.input_schema)
    }) {
        return Err(invalid(
            "MCP tool removed or schema changed; refresh and review grants",
        ));
    }
    Ok(())
}
struct RunHost {
    runtime: Arc<McpRuntime>,
    scope: McpRunScope,
    declarations: Vec<McpToolDeclaration>,
    journal: Arc<dyn McpInvocationJournal>,
}
#[async_trait]
impl HarnessMcpToolHost for RunHost {
    fn available(&self) -> &[McpToolDeclaration] {
        &self.declarations
    }
    async fn invoke(
        &self,
        declaration: &McpToolDeclaration,
        invocation_id: &str,
        args: Value,
    ) -> AppResult<ToolInvocation> {
        let saved = self.validate(declaration, &args)?;
        if let Some(value) = self
            .journal
            .completed(&self.scope, saved, invocation_id, &args)
            .await?
        {
            return Ok(invocation(value));
        }
        let selection = self
            .runtime
            .catalog
            .agent_mcp_selection(self.scope.company_id, self.scope.agent_id)
            .await?;
        if selection.revision != saved.selection_revision
            || !selection
                .connection_ids
                .contains(&saved.identity.connection_id)
        {
            return Err(invalid("MCP selection changed"));
        }
        let connection = self
            .runtime
            .catalog
            .mcp_connections_by_ids(self.scope.company_id, &[saved.identity.connection_id])
            .await?
            .into_iter()
            .find(|c| {
                c.id == saved.identity.connection_id
                    && c.enabled
                    && c.revision == saved.definition_revision
                    && c.credential_revision == saved.credential_revision
                    && c.tool_grants.contains(&saved.identity.name)
            })
            .ok_or_else(|| invalid("MCP definition or grant revoked"))?;
        let session = self.runtime.connect(&connection).await?;
        // Keep the durable dispatch phase boxed at the external runtime seam.
        let result = Box::pin(self.dispatch(session.as_ref(), saved, invocation_id, args)).await;
        let closed = session.close().await;
        let result = result?;
        closed?;
        Ok(invocation(result))
    }
}

/// Freeze the exact bounded model-facing result before its durable receipt is written.
fn bounded_result(value: Value) -> Value {
    let encoded = value.to_string();
    if encoded.chars().count() <= 16_384 && encoded.len() <= super::harness::runs::MAX_RESULT_BYTES
    {
        return value;
    }
    serde_json::json!({"isError":value.get("isError").and_then(Value::as_bool).unwrap_or(false),
        "truncated":true,"content":[{"type":"text","text":encoded.chars().take(4096).collect::<String>()}]})
}

fn invocation(value: Value) -> ToolInvocation {
    let success = value.get("isError").and_then(Value::as_bool) != Some(true);
    let mut invocation = ToolInvocation::success(value);
    invocation.success = success;
    invocation
}

impl RunHost {
    fn validate(
        &self,
        declaration: &McpToolDeclaration,
        args: &Value,
    ) -> AppResult<&McpToolDeclaration> {
        let saved = self
            .declarations
            .iter()
            .find(|d| d.identity == declaration.identity)
            .ok_or_else(|| invalid("MCP tool is not granted"))?;
        if saved.definition_revision != declaration.definition_revision
            || saved.selection_revision != declaration.selection_revision
            || saved.credential_revision != declaration.credential_revision
            || saved.input_schema != declaration.input_schema
        {
            return Err(invalid("MCP declaration changed"));
        }
        if args.to_string().len() > MAX_MCP_SCHEMA_BYTES
            || !super::tool_schema::compile(&saved.input_schema)?.is_valid(args)
        {
            return Err(invalid("Invalid MCP arguments"));
        }
        Ok(saved)
    }
}

impl RunHost {
    async fn dispatch(
        &self,
        session: &dyn super::mcp_client::McpSession,
        saved: &McpToolDeclaration,
        invocation_id: &str,
        args: Value,
    ) -> AppResult<Value> {
        let discovered = session.discover().await?;
        validate_discovered(
            &McpDiscoveredTool {
                name: saved.identity.name.clone(),
                description: saved.description.clone(),
                input_schema: saved.input_schema.clone(),
            },
            &discovered,
        )?;
        match self
            .journal
            .claim(&self.scope, saved, invocation_id, &args)
            .await?
        {
            McpClaim::Completed(value) => Ok(value),
            McpClaim::Execute => match session.call(&saved.identity.name, args).await {
                Ok(value) => {
                    let value = bounded_result(value);
                    self.journal
                        .complete(&self.scope, invocation_id, &value)
                        .await?;
                    Ok(value)
                }
                Err(error) => {
                    self.journal
                        .indeterminate(&self.scope, invocation_id)
                        .await?;
                    Err(error)
                }
            },
        }
    }
}
