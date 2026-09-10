//! Persistence contract for company MCP catalog operations. HTTP transport/discovery is separate.
use crate::{
    app_error::{AppError, AppResult},
    entities::mcp::*,
};
use async_trait::async_trait;
use secrecy::SecretString;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpConnectionWrite {
    pub slug: crate::entities::value_objects::McpConnectionSlug,
    pub endpoint: McpEndpoint,
    pub enabled: bool,
    pub auth: McpAuth,
    pub discovered_tools: Vec<McpDiscoveredTool>,
    pub tool_grants: Vec<McpToolName>,
}
impl McpConnectionWrite {
    pub fn validate(&self) -> AppResult<()> {
        super::channel::validate_slug(&self.slug, super::channel::SlugKind::AgentSlug)?;
        if self.discovered_tools.len() > MAX_MCP_DISCOVERED_TOOLS
            || self.tool_grants.len() > MAX_EFFECTIVE_MCP_TOOLS
        {
            return Err(invalid("MCP tool count exceeds the server limit"));
        }
        let encoded = serde_json::to_vec(&self.discovered_tools)
            .map_err(|_| invalid("Invalid MCP discovery"))?;
        if encoded.len() > MAX_MCP_DISCOVERY_BYTES {
            return Err(invalid("MCP discovery exceeds 1 MiB"));
        }
        let mut names = std::collections::HashSet::new();
        for tool in &self.discovered_tools {
            if !names.insert(&tool.name) {
                return Err(invalid("Duplicate discovered MCP tool name"));
            }
            crate::services::tool_schema::compile(&tool.input_schema)?;
            if !tool.input_schema.is_object()
                || serde_json::to_vec(&tool.input_schema)
                    .map_err(|_| invalid("Invalid MCP schema"))?
                    .len()
                    > MAX_MCP_SCHEMA_BYTES
            {
                return Err(invalid(
                    "MCP tool schema must be an object of at most 64 KiB",
                ));
            }
        }
        let mut grants = std::collections::HashSet::new();
        for grant in &self.tool_grants {
            if !names.contains(grant) || !grants.insert(grant) {
                return Err(invalid(
                    "MCP grants must uniquely name reviewed discovered tools",
                ));
            }
        }
        Ok(())
    }
}
fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}

/// Tokens are write-only and use a distinct port from catalog projections.
#[async_trait]
pub trait McpCredentialPersistence: Send + Sync {
    async fn replace_mcp_token(
        &self,
        company_id: Uuid,
        connection_id: Uuid,
        expected_revision: i64,
        token: Option<SecretString>,
    ) -> AppResult<i64>;
    async fn mcp_token(
        &self,
        company_id: Uuid,
        connection_id: Uuid,
        expected_revision: i64,
    ) -> AppResult<Option<SecretString>>;
}

#[async_trait]
pub trait McpPersistence: Send + Sync {
    async fn mcp_selecting_agents(
        &self,
        company_id: Uuid,
        connection_id: Uuid,
    ) -> AppResult<Vec<McpSelectingAgent>>;
    async fn list_mcp_connections(&self, company_id: Uuid) -> AppResult<Vec<CompanyMcpConnection>>;
    /// Runtime projection: at most MAX_MCP_SELECTIONS IDs; missing/deleted rows are omitted.
    async fn mcp_connections_by_ids(
        &self,
        company_id: Uuid,
        connection_ids: &[Uuid],
    ) -> AppResult<Vec<CompanyMcpConnection>>;
    async fn create_mcp_connection(
        &self,
        company_id: Uuid,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection>;
    async fn update_mcp_connection(
        &self,
        company_id: Uuid,
        connection_id: Uuid,
        expected_revision: i64,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection>;
    async fn delete_mcp_connection(
        &self,
        company_id: Uuid,
        connection_id: Uuid,
        expected_revision: i64,
    ) -> AppResult<()>;
    async fn agent_mcp_selection(
        &self,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<AgentMcpSelection>;
    /// None preserves the set; Some([]) clears it. Compare-and-swap never silently loses an edit.
    async fn replace_agent_mcp_selection(
        &self,
        selection: AgentMcpSelection,
        connection_ids: Option<Vec<Uuid>>,
    ) -> AppResult<AgentMcpSelection>;
}

/// Authorization belongs to the application boundary; storage adapters never infer an actor.
#[derive(Clone)]
pub struct McpUseCases {
    companies: std::sync::Arc<dyn super::company::CompanyPersistence>,
    catalog: std::sync::Arc<dyn McpPersistence>,
    credentials: std::sync::Arc<dyn McpCredentialPersistence>,
    client: std::sync::Arc<dyn crate::services::mcp_client::McpClient>,
}
impl McpUseCases {
    pub fn new(
        companies: std::sync::Arc<dyn super::company::CompanyPersistence>,
        catalog: std::sync::Arc<dyn McpPersistence>,
        credentials: std::sync::Arc<dyn McpCredentialPersistence>,
        client: std::sync::Arc<dyn crate::services::mcp_client::McpClient>,
    ) -> Self {
        Self {
            companies,
            catalog,
            credentials,
            client,
        }
    }
    pub async fn list(&self, user: Uuid, company: Uuid) -> AppResult<Vec<CompanyMcpConnection>> {
        super::company::managed_company(self.companies.as_ref(), user, company).await?;
        self.catalog.list_mcp_connections(company).await
    }
    pub async fn create(
        &self,
        user: Uuid,
        company: Uuid,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection> {
        super::company::managed_company(self.companies.as_ref(), user, company).await?;
        write.validate()?;
        self.client.validate_endpoint(&write.endpoint)?;
        self.catalog.create_mcp_connection(company, write).await
    }
    pub async fn update(
        &self,
        user: Uuid,
        current: &CompanyMcpConnection,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection> {
        super::company::managed_company(self.companies.as_ref(), user, current.company_id).await?;
        write.validate()?;
        self.client.validate_endpoint(&write.endpoint)?;
        self.catalog
            .update_mcp_connection(current.company_id, current.id, current.revision, write)
            .await
    }
    pub async fn delete(&self, user: Uuid, current: &CompanyMcpConnection) -> AppResult<()> {
        super::company::managed_company(self.companies.as_ref(), user, current.company_id).await?;
        self.catalog
            .delete_mcp_connection(current.company_id, current.id, current.revision)
            .await
    }
    pub async fn replace_token(
        &self,
        user: Uuid,
        current: &CompanyMcpConnection,
        token: Option<SecretString>,
    ) -> AppResult<i64> {
        super::company::managed_company(self.companies.as_ref(), user, current.company_id).await?;
        self.credentials
            .replace_mcp_token(current.company_id, current.id, current.revision, token)
            .await
    }
    pub async fn select(
        &self,
        user: Uuid,
        current: AgentMcpSelection,
        ids: Option<Vec<Uuid>>,
    ) -> AppResult<AgentMcpSelection> {
        super::company::managed_company(self.companies.as_ref(), user, current.company_id).await?;
        self.catalog.replace_agent_mcp_selection(current, ids).await
    }
}

impl McpUseCases {
    pub async fn shutdown(&self) -> AppResult<()> {
        self.client.shutdown().await
    }
    pub async fn get(
        &self,
        user: Uuid,
        company: Uuid,
        id: Uuid,
    ) -> AppResult<CompanyMcpConnection> {
        self.list(user, company)
            .await?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| AppError::NotFound("MCP connection not found".into()))
    }
    pub async fn selection(
        &self,
        user: Uuid,
        company: Uuid,
        agent: Uuid,
    ) -> AppResult<AgentMcpSelection> {
        super::company::managed_company(self.companies.as_ref(), user, company).await?;
        self.catalog.agent_mcp_selection(company, agent).await
    }
    pub async fn selecting_agents(
        &self,
        user: Uuid,
        company: Uuid,
        id: Uuid,
    ) -> AppResult<Vec<McpSelectingAgent>> {
        self.get(user, company, id).await?;
        self.catalog.mcp_selecting_agents(company, id).await
    }
    pub async fn refresh(
        &self,
        user: Uuid,
        current: &CompanyMcpConnection,
    ) -> AppResult<CompanyMcpConnection> {
        super::company::managed_company(self.companies.as_ref(), user, current.company_id).await?;
        let token = self
            .credentials
            .mcp_token(current.company_id, current.id, current.revision)
            .await?;
        if current.auth == McpAuth::Bearer && token.is_none() {
            return Err(invalid("Set a bearer token before testing this connection"));
        }
        let session = self.client.connect(&current.endpoint, token).await?;
        let discovery = session.discover().await;
        let closed = session.close().await;
        let tools = discovery?;
        closed?;
        // Retain only grants whose reviewed schema is unchanged. Refresh never expands authority.
        let grants = current
            .tool_grants
            .iter()
            .filter(|name| {
                let old = current.discovered_tools.iter().find(|t| &t.name == *name);
                let new = tools.iter().find(|t| &t.name == *name);
                matches!((old, new), (Some(a), Some(b)) if a.input_schema == b.input_schema)
            })
            .cloned()
            .collect();
        self.update(
            user,
            current,
            McpConnectionWrite {
                slug: current.slug.clone(),
                endpoint: current.endpoint.clone(),
                enabled: current.enabled,
                auth: current.auth.clone(),
                discovered_tools: tools,
                tool_grants: grants,
            },
        )
        .await
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct McpSelectingAgent {
    pub id: Uuid,
    pub name: String,
}
