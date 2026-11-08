//! Registry and account metadata for owner consent. Never credential-bearing.
use crate::domain::integrations::{authority::*, *};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingApp {
    pub id: AppId,
    pub provider: ProviderId,
    pub config: AuthConfigId,
    pub method: AuthMethod,
    pub schema: CredentialSchema,
    pub purposes: Vec<InvocationKind>,
    pub destinations: Vec<ApprovedDestination>,
    pub capabilities: Vec<Capability>,
}
impl BindingApp {
    pub fn accepts(&self, account: &ConnectedAccount) -> bool {
        account.status == AccountStatus::Ready
            && account.provider_id == self.provider
            && account.auth_config_id == self.config
            && account.auth_method == self.method
            && account.credential_schema == self.schema
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct AccountUseStatus {
    pub account: AccountId,
    pub app: AppId,
    pub label: String,
    pub status: AccountStatus,
    pub authorization_current: bool,
    pub grant_enabled: bool,
}
pub(crate) struct BindingStatusRequest<'a> {
    pub reference: &'a IntegrationReference,
    pub client: &'a OwnerNamespace,
    pub scope: &'a HostBinding,
    pub binding: &'a HostBinding,
}
