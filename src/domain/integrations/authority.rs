use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ApprovedDestination(String);
impl ApprovedDestination {
    pub fn parse(value: impl Into<String>) -> IntegrationResult<Self> {
        let value = value.into();
        if value.len() > 2048 {
            return Err(IntegrationError::Invalid);
        }
        let parsed = url::Url::parse(&value).map_err(|_| IntegrationError::Invalid)?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(Self(parsed.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for ApprovedDestination {
    type Error = IntegrationError;
    fn try_from(value: String) -> IntegrationResult<Self> {
        Self::parse(value)
    }
}
impl From<ApprovedDestination> for String {
    fn from(value: ApprovedDestination) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UseGrant {
    pub host_scope: HostBinding,
    pub purpose: InvocationKind,
    pub id: UseGrantId,
    pub account: AccountId,
    pub owner: AccountOwner,
    pub client: OwnerNamespace,
    pub binding: HostBinding,
    pub app: AppId,
    pub app_policy: AppPolicyDigest,
    pub destination: ApprovedDestination,
    pub account_authorization: AuthorizationRevision,
    pub revision: GrantRevision,
    pub enabled: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationKind {
    DeterministicHttp,
    DeterministicMcp,
    ModelTool,
    DirectTool,
    SealedSample,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationSnapshot {
    pub account: AccountId,
    pub grant: UseGrantId,
    pub client: OwnerNamespace,
    pub binding: HostBinding,
    pub app: AppId,
    pub app_policy: AppPolicyDigest,
    pub destination: ApprovedDestination,
    pub account_authorization: AuthorizationRevision,
    pub grant_revision: GrantRevision,
    pub connection_revision: ConnectionRevision,
    pub catalog_revision: CatalogRevision,
}
/// Facts loaded by the host guard, never accepted from workflow arguments.
#[derive(Debug, Clone, Copy)]
pub struct HostAuthorityFacts {
    pub owner_is_member: bool,
    pub connection_enabled: bool,
    pub action_authorized: bool,
    pub server_created_provenance: bool,
    pub kind: InvocationKind,
    pub connection_revision: ConnectionRevision,
    pub catalog_revision: CatalogRevision,
}
/// Current module facts, shared by the host gate and locked credential resolution.
pub fn authorize_credential(
    account: &ConnectedAccount,
    grant: &UseGrant,
    expected: &AuthorizationSnapshot,
    kind: InvocationKind,
) -> IntegrationResult<()> {
    if !matches!(
        kind,
        InvocationKind::DeterministicHttp | InvocationKind::DeterministicMcp
    ) || account.status != AccountStatus::Ready
        || !grant.enabled
        || grant.purpose != kind
        || account.id != grant.account
        || account.owner != grant.owner
        || grant.client != account.owner.namespace
        || expected.client != grant.client
        || expected.account != account.id
        || expected.grant != grant.id
        || expected.binding != grant.binding
        || expected.app != grant.app
        || expected.app_policy != grant.app_policy
        || expected.destination != grant.destination
        || expected.account_authorization != account.authorization_revision
        || grant.account_authorization != account.authorization_revision
        || expected.grant_revision != grant.revision
    {
        return Err(IntegrationError::Denied);
    }
    Ok(())
}
pub fn authorize_dispatch(
    account: &ConnectedAccount,
    grant: &UseGrant,
    expected: &AuthorizationSnapshot,
    host: HostAuthorityFacts,
) -> IntegrationResult<()> {
    if !host.server_created_provenance
        || !host.owner_is_member
        || !host.connection_enabled
        || !host.action_authorized
        || expected.connection_revision != host.connection_revision
        || expected.catalog_revision != host.catalog_revision
    {
        return Err(IntegrationError::Denied);
    }
    authorize_credential(account, grant, expected, host.kind)
}

#[derive(Debug, Clone, Copy)]
pub struct GrantAuthorityFacts {
    pub owner_is_member: bool,
    pub administrator_can_edit: bool,
    pub owner_confirmed: bool,
}
pub fn authorize_grant(
    owner: &AccountOwner,
    account: &ConnectedAccount,
    binding_client: &OwnerNamespace,
    facts: GrantAuthorityFacts,
) -> IntegrationResult<()> {
    if owner != &account.owner
        || binding_client != &owner.namespace
        || account.status != AccountStatus::Ready
        || !facts.owner_is_member
        || !facts.administrator_can_edit
        || !facts.owner_confirmed
    {
        return Err(IntegrationError::Denied);
    }
    Ok(())
}

/// A host stores only this nonsecret, versioned reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationReference {
    pub account: AccountId,
    pub grant: UseGrantId,
    pub authorization: AuthorizationRevision,
    pub grant_revision: GrantRevision,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "source",
    content = "reference",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CredentialSource {
    None,
    ConnectionBearer,
    Integration(IntegrationReference),
}
