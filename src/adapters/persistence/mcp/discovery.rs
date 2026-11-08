//! Genuine owner/configuration authority. There is no workflow action, fence or business port.
use super::*;
use crate::{
    adapters::{
        integrations::owner::owner_for_user,
        persistence::{
            integration_bindings::users_on,
            integration_connect_authority::browser_deadline_on,
            integrations::PostgresVault,
            workflow::{authority::authorize_company, integration_authority::mcp_snapshot},
        },
    },
    application::{integrations::*, workflow::WorkflowActor},
    domain::{
        integrations::{authority::AuthorizationSnapshot, *},
        workflow::CompanyId,
    },
};
use chrono::Utc;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) struct PostgresMcpDiscovery {
    pub(crate) vault: PostgresVault,
    pub(crate) gateway: Arc<dyn ExecutionGateway>,
}
#[async_trait]
impl McpIntegrationDiscovery for PostgresMcpDiscovery {
    async fn refresh(
        &self,
        user: Uuid,
        owner: VerifiedOwner,
        expected: &CompanyMcpConnection,
    ) -> AppResult<CompanyMcpConnection> {
        if owner.owner() != &owner_for_user(user) {
            return Err(denied());
        }
        let authority = Arc::new(HostConfigurationAuthority {
            persistence: self.vault.persistence.clone(),
            user,
            owner: owner.clone(),
            expected: expected.clone(),
        });
        let mut tx = begin(&authority.persistence).await?;
        let (_, authorization) = authority.current(&mut tx).await?;
        tx.rollback().await?;
        let context = OwnerDiscoveryContext {
            owner,
            authorization,
            authority: authority.clone(),
            deadline: Utc::now() + chrono::Duration::seconds(30),
            cancellation: CancellationToken::new(),
        };
        let catalog = self.gateway.discover_mcp(context.clone()).await?;
        authority
            .save(&self.vault, &context, catalog.into_tools())
            .await
    }
}
struct HostConfigurationAuthority {
    persistence: PostgresPersistence,
    user: Uuid,
    owner: VerifiedOwner,
    expected: CompanyMcpConnection,
}
impl HostConfigurationAuthority {
    async fn identity(&self, tx: &mut Transaction<'_, Postgres>) -> AppResult<()> {
        users_on(tx, vec![self.user]).await?;
        authorize_company(
            tx,
            CompanyId::new(self.expected.company_id),
            WorkflowActor::authenticated(self.user)?,
        )
        .await?;
        if self.owner.browser.is_some() {
            browser_deadline_on(tx, &self.owner).await?;
        }
        Ok(())
    }
    async fn current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> AppResult<(CompanyMcpConnection, AuthorizationSnapshot)> {
        self.identity(tx).await?;
        lock_company(tx, self.expected.company_id).await?;
        let current = get_connection(tx, self.expected.company_id, self.expected.id).await?;
        if !current.enabled
            || current.revision != self.expected.revision
            || current.auth != self.expected.auth
            || current.endpoint != self.expected.endpoint
        {
            return Err(denied());
        }
        let snapshot = mcp_snapshot(tx, &current).await?.ok_or_else(denied)?;
        Ok((current, snapshot))
    }
    fn same_context(&self, c: &OwnerDiscoveryContext) -> bool {
        let browser_matches = match (&self.owner.browser, &c.owner.browser) {
            (None, None) => true,
            (Some(a), Some(b)) => a.session == b.session && a.binding == b.binding,
            _ => false,
        };
        self.owner.owner() == c.owner.owner()
            && browser_matches
            && !c.cancellation.is_cancelled()
            && c.deadline > Utc::now()
    }
    async fn live(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        c: &OwnerDiscoveryContext,
    ) -> AppResult<()> {
        if !self.same_context(c) {
            return Err(denied());
        }
        self.identity(tx).await?;
        let live: bool = sqlx::query_scalar("SELECT clock_timestamp()<$1")
            .bind(c.deadline)
            .fetch_one(&mut **tx)
            .await?;
        if !live {
            return Err(denied());
        }
        Ok(())
    }
    async fn save(
        &self,
        vault: &PostgresVault,
        c: &OwnerDiscoveryContext,
        tools: Vec<McpDiscoveredTool>,
    ) -> AppResult<CompanyMcpConnection> {
        let mut tx = begin(&self.persistence).await?;
        let (current, snapshot) = self.current(&mut tx).await?;
        if snapshot != c.authorization {
            return Err(denied());
        }
        let facts = facts(&snapshot);
        vault.validate_configuration_under_host(c, facts).await?;
        self.live(&mut tx, c).await?;
        let grants = current.tool_grants.iter().filter(|name| {
            let old = current.discovered_tools.iter().find(|t| &t.name == *name);
            let new = tools.iter().find(|t| &t.name == *name);
            matches!((old, new), (Some(a), Some(b)) if a.input_schema == b.input_schema && a.output_schema == b.output_schema)
        }).cloned().collect();
        let result = update_on(
            &mut tx,
            current.company_id,
            current.id,
            current.revision,
            McpConnectionWrite {
                slug: current.slug,
                endpoint: current.endpoint,
                enabled: current.enabled,
                auth: current.auth,
                discovered_tools: tools,
                tool_grants: grants,
            },
        )
        .await?;
        // Our catalog revision write is known and still uncommitted. Revalidate owner/session
        // and module consent before COMMIT; a concurrent disconnect cannot turn into a grant.
        vault.validate_configuration_under_host(c, facts).await?;
        self.live(&mut tx, c).await?;
        tx.commit().await?;
        Ok(result)
    }
}
#[async_trait]
impl ConfigurationAuthority for HostConfigurationAuthority {
    async fn check(
        &self,
        c: &OwnerDiscoveryContext,
        gate: &dyn ConfigurationCredentialGate,
    ) -> IntegrationResult<()> {
        if !self.same_context(c) {
            return Err(IntegrationError::Denied);
        }
        let mut tx = begin(&self.persistence).await.map_err(map_error)?;
        let (_, snapshot) = self.current(&mut tx).await.map_err(map_error)?;
        if snapshot != c.authorization {
            return Err(IntegrationError::Denied);
        }
        gate.check(c, facts(&snapshot)).await?;
        self.live(&mut tx, c).await.map_err(map_error)?;
        tx.rollback().await.map_err(|_| IntegrationError::Storage)?;
        Ok(())
    }
}
fn facts(snapshot: &AuthorizationSnapshot) -> ConfigurationAuthorityFacts {
    ConfigurationAuthorityFacts {
        owner_is_member: true,
        administrator_can_edit: true,
        connection_enabled: true,
        connection_revision: snapshot.connection_revision,
        catalog_revision: snapshot.catalog_revision,
    }
}
async fn begin(p: &PostgresPersistence) -> AppResult<Transaction<'static, Postgres>> {
    let mut tx = p.pool().begin().await?;
    sqlx::query("SELECT set_config('lock_timeout','5s',true),set_config('statement_timeout','5s',true),set_config('idle_in_transaction_session_timeout','12s',true)").execute(&mut *tx).await?;
    Ok(tx)
}
fn denied() -> AppError {
    AppError::Forbidden(
        "MCP discovery requires the current account owner and connection authority".into(),
    )
}
fn map_error(error: AppError) -> IntegrationError {
    match error {
        AppError::NotFound(_) | AppError::Forbidden(_) | AppError::InvalidCredentials => {
            IntegrationError::Denied
        }
        AppError::Conflict(_) => IntegrationError::Conflict,
        _ => IntegrationError::Storage,
    }
}
