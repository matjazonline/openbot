use super::{registry::CodeOwnedRegistry, *};
use crate::domain::integrations::*;
use async_trait::async_trait;
use secrecy::SecretString;
use std::sync::Arc;

/// Created only after registered adapter validation; never a browser/wire DTO.
#[derive(Debug)]
pub(crate) struct AccountWrite {
    pub key: AccountKey,
    pub provider: ProviderId,
    pub config: AuthConfigId,
    pub method: AuthMethod,
    pub schema: CredentialSchema,
    pub identity: Option<ExternalIdentity>,
    pub metadata: AccountMetadata,
    pub payload: SecretString,
}
#[async_trait]
pub(crate) trait AccountRepository: Send + Sync {
    async fn grants(
        &self,
        owner: &AccountOwner,
        account: AccountId,
        after: Option<UseGrantId>,
    ) -> IntegrationResult<GrantInspection>;
    async fn list(&self, owner: &AccountOwner, page: PageRequest)
    -> IntegrationResult<AccountPage>;
    async fn get(&self, owner: &AccountOwner, id: AccountId)
    -> IntegrationResult<ConnectedAccount>;
    async fn create(
        &self,
        owner: &AccountOwner,
        write: AccountWrite,
    ) -> IntegrationResult<ConnectedAccount>;
    async fn replace(
        &self,
        owner: &AccountOwner,
        id: AccountId,
        expected: AccountRevision,
        write: AccountWrite,
    ) -> IntegrationResult<ConnectedAccount>;
    async fn disconnect(
        &self,
        owner: &AccountOwner,
        id: AccountId,
        expected: AuthorizationRevision,
    ) -> IntegrationResult<ConnectedAccount>;
    async fn delete(
        &self,
        owner: &AccountOwner,
        id: AccountId,
        expected: AuthorizationRevision,
    ) -> IntegrationResult<()>;
}
pub struct VaultManagement {
    repository: Arc<dyn AccountRepository>,
    registry: Arc<CodeOwnedRegistry>,
}
impl VaultManagement {
    pub(crate) fn new(
        repository: Arc<dyn AccountRepository>,
        registry: Arc<CodeOwnedRegistry>,
    ) -> Self {
        Self {
            repository,
            registry,
        }
    }
    fn static_write(&self, input: StaticAccountInput) -> IntegrationResult<AccountWrite> {
        let entry = self.registry.get(&input.app)?;
        if entry.provider.method != AuthMethod::StaticSecret {
            return Err(IntegrationError::Denied);
        }
        let identity = self.registry.validate(
            &input.app,
            AuthMethod::StaticSecret,
            &entry.provider.schema,
            &input.value,
        )?;
        Ok(AccountWrite {
            key: input.key,
            provider: entry.provider.id.clone(),
            config: entry.config.id.clone(),
            method: AuthMethod::StaticSecret,
            schema: entry.provider.schema.clone(),
            identity,
            metadata: input.metadata,
            payload: input.value,
        })
    }
}
#[async_trait]
impl ManagementGateway for VaultManagement {
    async fn list_accounts(
        &self,
        owner: &VerifiedOwner,
        page: PageRequest,
    ) -> IntegrationResult<AccountPage> {
        self.repository.list(owner.owner(), page).await
    }
    async fn get_account(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
    ) -> IntegrationResult<ConnectedAccount> {
        self.repository.get(owner.owner(), id).await
    }
    async fn create_static_account(
        &self,
        owner: &VerifiedOwner,
        input: StaticAccountInput,
    ) -> IntegrationResult<ConnectedAccount> {
        self.repository
            .create(owner.owner(), self.static_write(input)?)
            .await
    }
    async fn replace_static_account(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
        expected: AccountRevision,
        input: StaticAccountInput,
    ) -> IntegrationResult<ConnectedAccount> {
        let current = self.repository.get(owner.owner(), id).await?;
        if current.auth_method != AuthMethod::StaticSecret {
            return Err(IntegrationError::Denied);
        }
        self.repository
            .replace(owner.owner(), id, expected, self.static_write(input)?)
            .await
    }
    async fn disconnect_account(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
        expected: AuthorizationRevision,
    ) -> IntegrationResult<ConnectedAccount> {
        self.repository
            .disconnect(owner.owner(), id, expected)
            .await
    }
    async fn delete_account(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
        expected: AuthorizationRevision,
    ) -> IntegrationResult<()> {
        self.repository.delete(owner.owner(), id, expected).await
    }
    async fn inspect_grants(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
        after: Option<UseGrantId>,
    ) -> IntegrationResult<GrantInspection> {
        self.repository.grants(owner.owner(), id, after).await
    }
}
