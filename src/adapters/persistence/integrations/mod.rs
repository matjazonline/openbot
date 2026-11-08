//! All connected-account and credential SQL belongs to this adapter.
mod accounts;
pub(crate) mod connect;
pub(crate) mod grants;
pub(crate) mod inventory;
pub(crate) mod lifecycle;
pub(crate) mod resolve;
mod rows;
mod status;
#[cfg(test)]
mod tests;
use super::PostgresPersistence;
use crate::{
    application::integrations::{
        registry::{CodeOwnedRegistry, static_registry},
        *,
    },
    domain::integrations::*,
};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct PostgresVault {
    pub(super) persistence: PostgresPersistence,
    pub(super) registry: Arc<CodeOwnedRegistry>,
}
impl PostgresVault {
    pub fn new(persistence: PostgresPersistence) -> Self {
        Self {
            persistence,
            registry: Arc::new(static_registry()),
        }
    }
    pub(crate) fn with_registry(
        persistence: PostgresPersistence,
        registry: Arc<CodeOwnedRegistry>,
    ) -> Self {
        Self {
            persistence,
            registry,
        }
    }
    pub fn gateway(
        &self,
        authority: Arc<dyn crate::application::integrations::grants::BindingAuthority>,
        connect: Arc<dyn ConnectGateway>,
        execution: Arc<dyn ExecutionGateway>,
    ) -> Arc<dyn IntegrationGateway> {
        let management = Arc::new(management::VaultManagement::new(
            Arc::new(self.clone()),
            self.registry.clone(),
        ));
        Arc::new(LocalIntegrationGateway::new(
            management,
            connect,
            Arc::new(crate::application::integrations::grants::GrantService {
                writer: Arc::new(self.clone()),
                authority,
            }),
            execution,
        ))
    }
    fn registration(
        &self,
        provider: &ProviderId,
        config: &AuthConfigId,
        method: AuthMethod,
        schema: &CredentialSchema,
    ) -> IntegrationResult<&crate::application::integrations::registry::Registration> {
        self.registry.for_account(provider, config, method, schema)
    }
}
fn storage_error(error: sqlx::Error) -> IntegrationError {
    if let Some(db) = error.as_database_error() {
        return match db.constraint() {
            Some(
                "integration_account_owner_cap"
                | "integration_grant_cap"
                | "integration_connect_cap",
            ) => IntegrationError::Limit,
            Some("integration_owner_retired") => IntegrationError::Denied,
            _ => match db.code().as_deref() {
                Some("23505") => IntegrationError::Conflict,
                Some("23514" | "22P02") => IntegrationError::Invalid,
                _ => IntegrationError::Storage,
            },
        };
    }
    IntegrationError::Storage
}

#[cfg(test)]
pub(crate) mod connect_tests;

mod observations;
#[cfg(test)]
pub(crate) mod renewal_tests;
#[cfg(test)]
pub(crate) use observations::tests as observation_test_support;
