use super::{AuthorizedInvocation, ExecutionContext, ExecutionOutcome};
use crate::domain::integrations::{
    authority::{ApprovedDestination, UseGrant},
    *,
};
use async_trait::async_trait;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Constructed only by a trusted local client adapter after authentication.
/// Deliberately not deserializable: an owner ID is data, not proof.
#[derive(Debug, Clone)]
pub struct VerifiedOwner {
    owner: AccountOwner,
    pub(crate) browser: Option<super::connect::ConnectBrowser>,
}
impl VerifiedOwner {
    pub(crate) fn from_verified_client(owner: AccountOwner) -> Self {
        Self {
            owner,
            browser: None,
        }
    }
    pub(crate) fn for_browser(
        owner: AccountOwner,
        browser: super::connect::ConnectBrowser,
    ) -> Self {
        Self {
            owner,
            browser: Some(browser),
        }
    }
    pub fn owner(&self) -> &AccountOwner {
        &self.owner
    }
}

#[derive(Debug)]
pub struct StaticAccountInput {
    pub key: AccountKey,
    pub app: AppId,
    pub metadata: AccountMetadata,
    pub value: SecretString,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantInspection {
    pub next: Option<UseGrantId>,
    pub grants: Vec<UseGrant>,
    pub attachment_available: bool,
}

#[async_trait]
pub trait ManagementGateway: Send + Sync {
    async fn binding_apps(&self) -> IntegrationResult<Vec<super::BindingApp>>;
    async fn account_choices(
        &self,
        owner: &VerifiedOwner,
        app: &AppId,
        page: PageRequest,
    ) -> IntegrationResult<AccountPage>;
    async fn list_accounts(
        &self,
        owner: &VerifiedOwner,
        page: PageRequest,
    ) -> IntegrationResult<AccountPage>;
    async fn get_account(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
    ) -> IntegrationResult<ConnectedAccount>;
    async fn create_static_account(
        &self,
        owner: &VerifiedOwner,
        input: StaticAccountInput,
    ) -> IntegrationResult<ConnectedAccount>;
    async fn replace_static_account(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
        expected: AccountRevision,
        input: StaticAccountInput,
    ) -> IntegrationResult<ConnectedAccount>;
    async fn disconnect_account(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
        expected: AuthorizationRevision,
    ) -> IntegrationResult<ConnectedAccount>;
    async fn delete_account(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
        expected: AuthorizationRevision,
    ) -> IntegrationResult<()>;
    async fn inspect_grants(
        &self,
        owner: &VerifiedOwner,
        id: AccountId,
        after: Option<UseGrantId>,
    ) -> IntegrationResult<GrantInspection>;
}

#[derive(Debug, Clone)]
pub struct BeginConnect {
    pub app: AppId,
    pub config: AuthConfigId,
    pub requested_capabilities: RequestedCapabilities,
    pub return_target: SafeReturnTarget,
    pub reconnect: Option<ReconnectTarget>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Vec<Capability>", into = "Vec<Capability>")]
pub struct RequestedCapabilities(Vec<Capability>);
impl TryFrom<Vec<Capability>> for RequestedCapabilities {
    type Error = IntegrationError;
    fn try_from(values: Vec<Capability>) -> IntegrationResult<Self> {
        if values.is_empty() || values.len() > 64 {
            return Err(IntegrationError::Limit);
        }
        Ok(Self(values))
    }
}
impl From<RequestedCapabilities> for Vec<Capability> {
    fn from(value: RequestedCapabilities) -> Self {
        value.0
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SafeReturnTarget(String);
impl SafeReturnTarget {
    pub fn parse(value: String) -> IntegrationResult<Self> {
        if value.len() > 1024
            || !value.starts_with("/ui/")
            || value.contains(['\\', '?', '#', '%'])
            || value.chars().any(char::is_control)
            || value
                .split('/')
                .any(|segment| matches!(segment, "." | ".."))
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for SafeReturnTarget {
    type Error = IntegrationError;
    fn try_from(value: String) -> IntegrationResult<Self> {
        Self::parse(value)
    }
}
impl From<SafeReturnTarget> for String {
    fn from(value: SafeReturnTarget) -> Self {
        value.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectStatus {
    Pending,
    Processing,
    Connected,
    Denied,
    Expired,
    Cancelled,
    MissingPermissions,
    MissingRefresh,
    IdentityMismatch,
    Conflict,
    ProviderError,
    Configuration,
    AdminBlocked,
}
impl ConnectStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Connected => "connected",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::MissingPermissions => "missing_permissions",
            Self::MissingRefresh => "missing_refresh",
            Self::IdentityMismatch => "identity_mismatch",
            Self::Conflict => "conflict",
            Self::ProviderError => "provider_error",
            Self::Configuration => "configuration",
            Self::AdminBlocked => "admin_blocked",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectTarget {
    pub account: AccountId,
    pub authorization: AuthorizationRevision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectApp {
    pub provider: ProviderId,
    pub id: AppId,
    pub config: AuthConfigId,
    pub capabilities: Vec<Capability>,
}
pub struct ConnectCallback {
    pub provider: ProviderId,
    pub state: String,
    pub code: Option<SecretString>,
    pub error: Option<String>,
}
pub struct ConnectWork {
    pub(crate) deadline: chrono::DateTime<chrono::Utc>,
    pub(crate) cancellation: tokio_util::sync::CancellationToken,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectAttempt {
    pub return_target: SafeReturnTarget,
    pub id: ConnectAttemptId,
    pub status: ConnectStatus,
    pub account: Option<AccountId>,
}
/// Produced by provider adapters only after allowlisting the authorization endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectStart {
    pub attempt: ConnectAttempt,
    pub authorization_url: SanitizedConnectUrl,
}
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct SanitizedConnectUrl(String);
impl SanitizedConnectUrl {
    pub fn from_registered_endpoint(
        value: String,
        registered_endpoint: &str,
    ) -> IntegrationResult<Self> {
        if value.len() > 4096 {
            return Err(IntegrationError::Limit);
        }
        let url = url::Url::parse(&value).map_err(|_| IntegrationError::Invalid)?;
        let registered =
            url::Url::parse(registered_endpoint).map_err(|_| IntegrationError::Invalid)?;
        if url.scheme() != "https"
            || url.origin() != registered.origin()
            || url.path() != registered.path()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query_pairs().any(|(key, _)| {
                matches!(
                    key.as_ref(),
                    "client_secret" | "access_token" | "refresh_token" | "code_verifier"
                )
            })
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(Self(url.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[async_trait]
pub trait ConnectGateway: Send + Sync {
    async fn connectable_apps(&self) -> IntegrationResult<Vec<ConnectApp>>;
    async fn finish_connect(
        &self,
        owner: &VerifiedOwner,
        callback: ConnectCallback,
        work: ConnectWork,
    ) -> IntegrationResult<ConnectAttempt>;
    async fn cancel_connect(
        &self,
        owner: &VerifiedOwner,
        id: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt>;

    async fn begin_connect(
        &self,
        owner: &VerifiedOwner,
        command: BeginConnect,
    ) -> IntegrationResult<ConnectStart>;
    async fn get_connect_attempt(
        &self,
        owner: &VerifiedOwner,
        id: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt>;
}

/// This confirmation is obtained by a trusted host adapter, with current authority.
/// It is not a browser-deserializable grant command.
#[derive(Debug)]
pub struct OwnerConfirmation {
    pub(crate) host_scope: HostBinding,
    pub(crate) purpose: crate::domain::integrations::authority::InvocationKind,
    pub(crate) expected_authorization: AuthorizationRevision,
    pub(crate) owner: VerifiedOwner,
    pub(crate) account: AccountId,
    pub(crate) binding: HostBinding,
    pub(crate) app: AppId,
    pub(crate) destination: ApprovedDestination,
    pub(crate) host_authority: HostAuthorityReference,
}
#[async_trait]
pub trait GrantGateway: Send + Sync {
    async fn create_use_grant(
        &self,
        confirmation: OwnerConfirmation,
    ) -> IntegrationResult<UseGrant>;
    async fn revoke_use_grant(
        &self,
        owner: &VerifiedOwner,
        grant: UseGrantId,
        expected: GrantRevision,
    ) -> IntegrationResult<()>;
}
#[async_trait]
pub trait ExecutionGateway: Send + Sync {
    async fn discover_mcp(
        &self,
        context: super::OwnerDiscoveryContext,
    ) -> IntegrationResult<super::DiscoveredCatalog>;
    async fn execute_http(
        &self,
        invocation: AuthorizedInvocation,
        context: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome>;
    async fn call_mcp(
        &self,
        invocation: AuthorizedInvocation,
        context: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome>;
}
pub trait IntegrationGateway:
    ManagementGateway + ConnectGateway + GrantGateway + ExecutionGateway
{
}
impl<T: ManagementGateway + ConnectGateway + GrantGateway + ExecutionGateway> IntegrationGateway
    for T
{
}

/// In-process composition; no listener and no token-export operation.
pub struct LocalIntegrationGateway {
    pub(crate) management: Arc<dyn ManagementGateway>,
    pub(crate) connect: Arc<dyn ConnectGateway>,
    pub(crate) grants: Arc<dyn GrantGateway>,
    pub(crate) execution: Arc<dyn ExecutionGateway>,
}
impl LocalIntegrationGateway {
    pub fn new(
        management: Arc<dyn ManagementGateway>,
        connect: Arc<dyn ConnectGateway>,
        grants: Arc<dyn GrantGateway>,
        execution: Arc<dyn ExecutionGateway>,
    ) -> Self {
        Self {
            management,
            connect,
            grants,
            execution,
        }
    }
}
#[async_trait]
impl ManagementGateway for LocalIntegrationGateway {
    async fn binding_apps(&self) -> IntegrationResult<Vec<super::BindingApp>> {
        self.management.binding_apps().await
    }
    async fn account_choices(
        &self,
        owner: &VerifiedOwner,
        app: &AppId,
        page: PageRequest,
    ) -> IntegrationResult<AccountPage> {
        self.management.account_choices(owner, app, page).await
    }
    async fn list_accounts(
        &self,
        o: &VerifiedOwner,
        p: PageRequest,
    ) -> IntegrationResult<AccountPage> {
        self.management.list_accounts(o, p).await
    }
    async fn get_account(
        &self,
        o: &VerifiedOwner,
        id: AccountId,
    ) -> IntegrationResult<ConnectedAccount> {
        self.management.get_account(o, id).await
    }
    async fn create_static_account(
        &self,
        o: &VerifiedOwner,
        i: StaticAccountInput,
    ) -> IntegrationResult<ConnectedAccount> {
        self.management.create_static_account(o, i).await
    }
    async fn replace_static_account(
        &self,
        o: &VerifiedOwner,
        id: AccountId,
        r: AccountRevision,
        i: StaticAccountInput,
    ) -> IntegrationResult<ConnectedAccount> {
        self.management.replace_static_account(o, id, r, i).await
    }
    async fn disconnect_account(
        &self,
        o: &VerifiedOwner,
        id: AccountId,
        r: AuthorizationRevision,
    ) -> IntegrationResult<ConnectedAccount> {
        self.management.disconnect_account(o, id, r).await
    }
    async fn delete_account(
        &self,
        o: &VerifiedOwner,
        id: AccountId,
        r: AuthorizationRevision,
    ) -> IntegrationResult<()> {
        self.management.delete_account(o, id, r).await
    }
    async fn inspect_grants(
        &self,
        o: &VerifiedOwner,
        id: AccountId,
        after: Option<UseGrantId>,
    ) -> IntegrationResult<GrantInspection> {
        self.management.inspect_grants(o, id, after).await
    }
}
#[async_trait]
impl ConnectGateway for LocalIntegrationGateway {
    async fn connectable_apps(&self) -> IntegrationResult<Vec<ConnectApp>> {
        self.connect.connectable_apps().await
    }
    async fn finish_connect(
        &self,
        o: &VerifiedOwner,
        c: ConnectCallback,
        w: ConnectWork,
    ) -> IntegrationResult<ConnectAttempt> {
        self.connect.finish_connect(o, c, w).await
    }
    async fn cancel_connect(
        &self,
        o: &VerifiedOwner,
        id: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt> {
        self.connect.cancel_connect(o, id).await
    }

    async fn begin_connect(
        &self,
        o: &VerifiedOwner,
        c: BeginConnect,
    ) -> IntegrationResult<ConnectStart> {
        self.connect.begin_connect(o, c).await
    }
    async fn get_connect_attempt(
        &self,
        o: &VerifiedOwner,
        id: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt> {
        self.connect.get_connect_attempt(o, id).await
    }
}
#[async_trait]
impl GrantGateway for LocalIntegrationGateway {
    async fn create_use_grant(&self, c: OwnerConfirmation) -> IntegrationResult<UseGrant> {
        self.grants.create_use_grant(c).await
    }
    async fn revoke_use_grant(
        &self,
        o: &VerifiedOwner,
        id: UseGrantId,
        r: GrantRevision,
    ) -> IntegrationResult<()> {
        self.grants.revoke_use_grant(o, id, r).await
    }
}
#[async_trait]
impl ExecutionGateway for LocalIntegrationGateway {
    async fn discover_mcp(
        &self,
        context: super::OwnerDiscoveryContext,
    ) -> IntegrationResult<super::DiscoveredCatalog> {
        self.execution.discover_mcp(context).await
    }
    async fn execute_http(
        &self,
        i: AuthorizedInvocation,
        c: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        self.execution.execute_http(i, c).await
    }
    async fn call_mcp(
        &self,
        i: AuthorizedInvocation,
        c: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        self.execution.call_mcp(i, c).await
    }
}
