use super::*;
use crate::domain::integrations::{authority::*, *};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub(crate) trait GrantWriter: Send + Sync {
    async fn create_confirmed(
        &self,
        confirmation: &OwnerConfirmation,
        facts: GrantAuthorityFacts,
    ) -> IntegrationResult<UseGrant>;
    async fn revoke(
        &self,
        owner: &AccountOwner,
        grant: UseGrantId,
        expected: GrantRevision,
    ) -> IntegrationResult<()>;
}
/// The concrete host locks its owners first, calls the writer, then installs the
/// nonsecret reference before releasing its host transaction.
#[async_trait]
pub(crate) trait BindingAuthority: Send + Sync {
    async fn confirm(
        &self,
        confirmation: OwnerConfirmation,
        writer: &dyn GrantWriter,
    ) -> IntegrationResult<UseGrant>;
}
pub(crate) struct GrantService {
    pub writer: Arc<dyn GrantWriter>,
    pub authority: Arc<dyn BindingAuthority>,
}
#[async_trait]
impl GrantGateway for GrantService {
    async fn create_use_grant(
        &self,
        confirmation: OwnerConfirmation,
    ) -> IntegrationResult<UseGrant> {
        self.authority
            .confirm(confirmation, self.writer.as_ref())
            .await
    }
    async fn revoke_use_grant(
        &self,
        owner: &VerifiedOwner,
        grant: UseGrantId,
        expected: GrantRevision,
    ) -> IntegrationResult<()> {
        self.writer.revoke(owner.owner(), grant, expected).await
    }
}
