//! Safe metadata for one configured binding, never the rest of its owner's vault.
use super::*;
use crate::application::integrations::choices::BindingStatusRequest;
impl PostgresVault {
    pub(crate) async fn binding_status(
        &self,
        request: BindingStatusRequest<'_>,
    ) -> IntegrationResult<Option<AccountUseStatus>> {
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        let grant = match super::grants::grant_on(&mut tx, request.reference.grant).await {
            Ok(grant) => grant,
            Err(IntegrationError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        if grant.account != request.reference.account
            || &grant.client != request.client
            || &grant.host_scope != request.scope
            || &grant.binding != request.binding
        {
            return Err(IntegrationError::Denied);
        }
        let account =
            match super::grants::read_account_on(&mut tx, &grant.owner, grant.account).await {
                Ok(account) => account,
                Err(IntegrationError::NotFound) => return Ok(None),
                Err(error) => return Err(error),
            };
        let grant = super::grants::grant_on(&mut tx, grant.id).await?;
        if grant.account != request.reference.account
            || &grant.client != request.client
            || &grant.host_scope != request.scope
            || &grant.binding != request.binding
        {
            return Err(IntegrationError::Denied);
        }
        let policy_current = match self.registry.policy(&grant.app) {
            Ok(policy) => policy == grant.app_policy,
            Err(IntegrationError::NotFound) => false,
            Err(error) => return Err(error),
        };
        let result = AccountUseStatus {
            account: account.id,
            app: grant.app,
            label: account.metadata.label().to_owned(),
            status: account.status,
            authorization_current: policy_current
                && request.reference.authorization == account.authorization_revision
                && grant.account_authorization == account.authorization_revision
                && request.reference.grant_revision == grant.revision,
            grant_enabled: grant.enabled,
        };
        tx.rollback().await.map_err(storage_error)?;
        Ok(Some(result))
    }
}
