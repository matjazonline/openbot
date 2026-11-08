//! Narrow nonserializable module ports. Never re-export from the public gateway.
use crate::domain::integrations::*;
use async_trait::async_trait;
use secrecy::SecretString;

#[derive(Debug)]
pub(crate) struct CredentialRecord {
    pub account: ConnectedAccount,
    pub versions: CredentialVersions,
    pub payload: SecretString,
}
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Narrow reader is tested privately; business execution uses the locked resolver in phase05"
    )
)]
#[async_trait]
pub(crate) trait CredentialStore: Send + Sync {
    async fn read_usable(
        &self,
        owner: &AccountOwner,
        id: AccountId,
    ) -> IntegrationResult<CredentialRecord>;
}
#[derive(Debug)]
pub(crate) enum InjectionMethod {
    Bearer,
    ApiKeyHeader(
        #[cfg_attr(
            not(test),
            expect(
                dead_code,
                reason = "The fixed header is consumed by authenticated transport in phase06"
            )
        )]
        String,
    ),
}
#[derive(Debug)]
pub(crate) struct TypedInjection {
    pub method: InjectionMethod,
    pub value: SecretString,
}
pub(crate) struct CredentialCacheUpdate<'a> {
    pub previous: &'a SecretString,
    pub replacement: &'a SecretString,
}

pub(crate) trait CredentialAdapter: Send + Sync {
    fn policy_identity(&self, app: &super::registry::AppDefinition) -> IntegrationResult<String>;
    fn validate_use(
        &self,
        payload: &SecretString,
        app: &super::registry::AppDefinition,
    ) -> IntegrationResult<()>;
    fn validate(&self, payload: &SecretString) -> IntegrationResult<Option<ExternalIdentity>>;
    /// Routine renewal must preserve the provider's consent identity and permission facts.
    /// Token/cache changes are permitted; owner consent changes require reconnect.
    fn validate_cache_update(&self, update: CredentialCacheUpdate<'_>) -> IntegrationResult<()>;
    fn injection(&self, payload: &SecretString) -> IntegrationResult<TypedInjection>;
}
