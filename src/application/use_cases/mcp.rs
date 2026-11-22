//! Persistence contract for company MCP catalog operations. HTTP transport/discovery is separate.
use crate::{
    app_error::{AppError, AppResult},
    entities::mcp::*,
};
use async_trait::async_trait;
use secrecy::SecretString;
use serde::Deserialize;
use uuid::Uuid;

/// Display identity for the schema an operator is currently reviewing.
pub(crate) fn schema_fingerprint(schema: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(schema.to_string().as_bytes()))
}

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
        validate_catalog(&self.discovered_tools, &self.tool_grants)
    }
}
/// Validate the reviewed discovery and grants together; callers cannot widen a bad catalog.
pub(crate) fn validate_catalog(
    discovered_tools: &[McpDiscoveredTool],
    tool_grants: &[McpToolName],
) -> AppResult<()> {
    if discovered_tools.len() > MAX_MCP_DISCOVERED_TOOLS
        || tool_grants.len() > MAX_EFFECTIVE_MCP_TOOLS
    {
        return Err(invalid("MCP tool count exceeds the server limit"));
    }
    let encoded =
        serde_json::to_vec(discovered_tools).map_err(|_| invalid("Invalid MCP discovery"))?;
    if encoded.len() > MAX_MCP_DISCOVERY_BYTES {
        return Err(invalid("MCP discovery exceeds 1 MiB"));
    }
    let mut names = std::collections::HashSet::new();
    for tool in discovered_tools {
        if !names.insert(&tool.name) {
            return Err(invalid("Duplicate discovered MCP tool name"));
        }
        for schema in std::iter::once(&tool.input_schema).chain(&tool.output_schema) {
            crate::services::tool_schema::compile(schema)?;
            if !schema.is_object()
                || serde_json::to_vec(schema)
                    .map_err(|_| invalid("Invalid MCP schema"))?
                    .len()
                    > MAX_MCP_SCHEMA_BYTES
            {
                return Err(invalid(
                    "MCP tool schema must be an object of at most 64 KiB",
                ));
            }
        }
    }
    let mut grants = std::collections::HashSet::new();
    for grant in tool_grants {
        if !names.contains(grant) || !grants.insert(grant) {
            return Err(invalid(
                "MCP grants must uniquely name reviewed discovered tools",
            ));
        }
    }
    Ok(())
}

fn not_found() -> AppError {
    AppError::NotFound("MCP connection not found".into())
}
fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
/// A connection id names one endpoint for its lifetime; frozen workflow actions rely on it.
pub fn endpoint_fixed() -> AppError {
    invalid("An MCP connection's endpoint cannot change; create a new connection instead")
}

/// Tokens are write-only and use a distinct port from catalog projections.
#[async_trait]
pub trait McpCredentialPersistence: Send + Sync {
    /// Applies only while `unit_id`, the authorized unit, still owns the connection.
    async fn replace_mcp_token(
        &self,
        company_id: Uuid,
        unit_id: Uuid,
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
    async fn set_mcp_level(
        &self,
        command: crate::application::workflow::connection_level::ConnectionLevelCommand,
    ) -> AppResult<CompanyMcpConnection>;
    /// Live connections owned by one of `units`.
    async fn list_mcp_connections(
        &self,
        company_id: Uuid,
        units: &[Uuid],
    ) -> AppResult<Vec<CompanyMcpConnection>>;
    async fn create_mcp_connection(
        &self,
        company_id: Uuid,
        unit_id: Uuid,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection>;
    /// Writes apply only while `unit_id`, the authorized unit, still owns the connection.
    async fn update_mcp_connection(
        &self,
        company_id: Uuid,
        unit_id: Uuid,
        connection_id: Uuid,
        expected_revision: i64,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection>;
    async fn delete_mcp_connection(
        &self,
        company_id: Uuid,
        unit_id: Uuid,
        connection_id: Uuid,
        expected_revision: i64,
    ) -> AppResult<()>;
    /// Move a connection to another unit; the actor must manage both (Plan07 01B.4).
    async fn move_mcp_connection(
        &self,
        actor: Uuid,
        company_id: Uuid,
        connection_id: Uuid,
        target: Uuid,
    ) -> AppResult<crate::application::ports::unit_resources::MovedResource>;
}

/// Authorization belongs to the application boundary; storage adapters never infer an actor.
#[derive(Clone)]
pub struct McpUseCases {
    discovery: std::sync::Arc<dyn McpIntegrationDiscovery>,
    companies: std::sync::Arc<dyn super::company::CompanyPersistence>,
    catalog: std::sync::Arc<dyn McpPersistence>,
    credentials: std::sync::Arc<dyn McpCredentialPersistence>,
    client: std::sync::Arc<dyn crate::services::mcp_client::McpClient>,
}

/// Host-owned configuration discovery and catalog commit, separate from business actions.
#[async_trait]
pub trait McpIntegrationDiscovery: Send + Sync {
    async fn refresh(
        &self,
        user: Uuid,
        owner: crate::application::integrations::VerifiedOwner,
        current: &CompanyMcpConnection,
    ) -> AppResult<CompanyMcpConnection>;
}
#[cfg(test)]
pub(crate) struct UnavailableMcpDiscovery;
#[cfg(test)]
#[async_trait]
impl McpIntegrationDiscovery for UnavailableMcpDiscovery {
    async fn refresh(
        &self,
        _: Uuid,
        _: crate::application::integrations::VerifiedOwner,
        _: &CompanyMcpConnection,
    ) -> AppResult<CompanyMcpConnection> {
        Err(AppError::ServiceUnavailable(
            "Integration discovery is unavailable".into(),
        ))
    }
}
impl McpUseCases {
    pub fn new(
        companies: std::sync::Arc<dyn super::company::CompanyPersistence>,
        catalog: std::sync::Arc<dyn McpPersistence>,
        credentials: std::sync::Arc<dyn McpCredentialPersistence>,
        client: std::sync::Arc<dyn crate::services::mcp_client::McpClient>,
        discovery: std::sync::Arc<dyn McpIntegrationDiscovery>,
    ) -> Self {
        Self {
            discovery,
            companies,
            catalog,
            credentials,
            client,
        }
    }
    pub async fn level_actor(
        &self,
        user: Uuid,
        company: Uuid,
    ) -> AppResult<crate::entities::access_level::LevelActor> {
        let access =
            super::company::readable_company(self.companies.as_ref(), user, company).await?;
        self.managed_units(user, company).await?;
        Ok(crate::entities::access_level::LevelActor {
            membership: access.membership,
            access_level: access.access_level,
        })
    }
    /// The units whose connections this person configures (Plan07, user decision a); entry
    /// needs some unit, and each connection is then checked against its own unit.
    async fn managed_units(&self, user: Uuid, company: Uuid) -> AppResult<Vec<Uuid>> {
        let company = crate::domain::workflow::CompanyId::new(company);
        crate::application::ports::administration::require_management(
            self.companies.as_ref(),
            user,
            company,
            crate::application::ports::administration::ManagementTarget::AnyUnit,
        )
        .await?;
        Ok(self
            .companies
            .managed_units(user, company)
            .await?
            .into_iter()
            .map(|unit| unit.as_uuid())
            .collect())
    }
    /// The connection's unit, when this person manages it; otherwise the usual not-found.
    async fn managed_unit(&self, user: Uuid, current: &CompanyMcpConnection) -> AppResult<Uuid> {
        let unit = current.unit_id.ok_or_else(not_found)?;
        let managed = crate::application::ports::administration::manages_unit(
            self.companies.as_ref(),
            user,
            crate::domain::workflow::CompanyId::new(current.company_id),
            crate::entities::department::DepartmentId::new(unit),
        )
        .await?;
        managed.then_some(unit).ok_or_else(not_found)
    }
    /// The units this person may put a connection in, and the default for a new one.
    pub async fn unit_choices(
        &self,
        user: Uuid,
        company: Uuid,
    ) -> AppResult<(
        Vec<crate::application::ports::administration::ManagedUnit>,
        Option<Uuid>,
    )> {
        self.managed_units(user, company).await?;
        let company = crate::domain::workflow::CompanyId::new(company);
        let units = self
            .companies
            .managed_unit_choices(
                user,
                company,
                crate::entities::department::ArchivedUnits::Exclude,
            )
            .await?;
        let default = self
            .companies
            .default_resource_unit(user, company)
            .await?
            .map(|unit| unit.as_uuid());
        Ok((units, default))
    }
    pub async fn list(&self, user: Uuid, company: Uuid) -> AppResult<Vec<CompanyMcpConnection>> {
        let units = self.managed_units(user, company).await?;
        self.catalog.list_mcp_connections(company, &units).await
    }
    pub async fn create(
        &self,
        user: Uuid,
        company: Uuid,
        unit: Option<Uuid>,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection> {
        let unit = crate::application::ports::administration::resource_creation_unit(
            self.companies.as_ref(),
            user,
            crate::domain::workflow::CompanyId::new(company),
            unit.map(crate::entities::department::DepartmentId::new),
        )
        .await?;
        write.validate()?;
        self.client.validate_endpoint(&write.endpoint)?;
        self.catalog
            .create_mcp_connection(company, unit.as_uuid(), write)
            .await
    }
    pub async fn update(
        &self,
        user: Uuid,
        current: &CompanyMcpConnection,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection> {
        let unit = self.managed_unit(user, current).await?;
        write.validate()?;
        if write.endpoint != current.endpoint {
            return Err(endpoint_fixed());
        }
        self.client.validate_endpoint(&write.endpoint)?;
        self.catalog
            .update_mcp_connection(
                current.company_id,
                unit,
                current.id,
                current.revision,
                write,
            )
            .await
    }
    /// The level change authorizes the connection's unit in its own transaction.
    pub async fn set_level(
        &self,
        command: crate::application::workflow::connection_level::ConnectionLevelCommand,
    ) -> AppResult<CompanyMcpConnection> {
        self.catalog.set_mcp_level(command).await
    }
    pub async fn delete(&self, user: Uuid, current: &CompanyMcpConnection) -> AppResult<()> {
        let unit = self.managed_unit(user, current).await?;
        self.catalog
            .delete_mcp_connection(current.company_id, unit, current.id, current.revision)
            .await
    }
    pub async fn replace_token(
        &self,
        user: Uuid,
        current: &CompanyMcpConnection,
        token: Option<SecretString>,
    ) -> AppResult<i64> {
        let unit = self.managed_unit(user, current).await?;
        self.credentials
            .replace_mcp_token(
                current.company_id,
                unit,
                current.id,
                current.revision,
                token,
            )
            .await
    }
    pub async fn move_to(
        &self,
        user: Uuid,
        company: Uuid,
        id: Uuid,
        target: Uuid,
    ) -> AppResult<crate::application::ports::unit_resources::MovedResource> {
        self.managed_units(user, company).await?;
        self.catalog
            .move_mcp_connection(user, company, id, target)
            .await
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
    pub async fn refresh(
        &self,
        user: Uuid,
        current: &CompanyMcpConnection,
        owner: crate::application::integrations::VerifiedOwner,
    ) -> AppResult<CompanyMcpConnection> {
        self.managed_unit(user, current).await?;
        if matches!(current.auth, McpAuth::Integration(_)) {
            return self.discovery.refresh(user, owner, current).await;
        }
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
                matches!((old, new), (Some(a), Some(b)) if a.input_schema == b.input_schema
                    && a.output_schema == b.output_schema)
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
