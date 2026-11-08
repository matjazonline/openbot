//! One renewal protocol, with separate genuine business and configuration authorities.
use super::*;
use crate::application::integrations::OwnerDiscoveryContext;
use crate::domain::integrations::authority::AuthorizationSnapshot;

pub(super) struct CredentialUse<'a> {
    pub authorization: &'a AuthorizationSnapshot,
    pub deadline: DateTime<Utc>,
    authority: UseAuthority<'a>,
}
enum UseAuthority<'a> {
    Business(&'a AuthorizedInvocation, &'a ExecutionContext),
    Configuration(&'a OwnerDiscoveryContext),
}
impl<'a> CredentialUse<'a> {
    pub(super) fn business(i: &'a AuthorizedInvocation, c: &'a ExecutionContext) -> Self {
        Self {
            authorization: &i.authorization,
            deadline: i.deadline,
            authority: UseAuthority::Business(i, c),
        }
    }
    pub(super) fn configuration(c: &'a OwnerDiscoveryContext) -> Self {
        Self {
            authorization: &c.authorization,
            deadline: c.deadline,
            authority: UseAuthority::Configuration(c),
        }
    }
    pub(super) fn cancellation(&self) -> &tokio_util::sync::CancellationToken {
        match self.authority {
            UseAuthority::Business(_, c) => &c.cancellation,
            UseAuthority::Configuration(c) => &c.cancellation,
        }
    }
    pub(super) fn configuration_owner(&self) -> Option<&AccountOwner> {
        match self.authority {
            UseAuthority::Business(_, _) => None,
            UseAuthority::Configuration(c) => Some(c.owner.owner()),
        }
    }
    pub(super) async fn check(&self, vault: &PostgresVault) -> IntegrationResult<()> {
        match self.authority {
            UseAuthority::Business(i, c) => Box::pin(vault.final_dispatch_guard(i, c)).await,
            UseAuthority::Configuration(c) => vault.configuration_guard(c).await,
        }
    }
}
