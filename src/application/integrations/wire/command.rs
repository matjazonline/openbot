use super::*;
use crate::application::integrations::{
    AuthorizedInvocation, RequestedCapabilities, SafeReturnTarget, gateway::ReconnectTarget,
};
use crate::domain::integrations::authority::*;
use secrecy::{ExposeSecret, SecretString};

/// Sensitive submission only. This type cannot appear in Reply.
#[derive(Debug)]
pub struct SecretSubmission(SecretString);
impl SecretSubmission {
    pub fn new(value: SecretString) -> IntegrationResult<Self> {
        if value.expose_secret().is_empty() || value.expose_secret().len() > MAX_CREDENTIAL_BYTES {
            return Err(IntegrationError::Credential);
        }
        Ok(Self(value))
    }
    pub fn into_secret(self) -> SecretString {
        self.0
    }
}
impl Serialize for SecretSubmission {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.expose_secret())
    }
}
impl<'de> Deserialize<'de> for SecretSubmission {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(SecretString::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaticInput {
    pub key: AccountKey,
    pub app: AppId,
    pub metadata: AccountMetadata,
    pub value: SecretSubmission,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub after: Option<AccountId>,
    pub size: usize,
}
impl Page {
    pub fn local(&self) -> IntegrationResult<PageRequest> {
        PageRequest::new(self.after, self.size)
    }
}

// No Debug derivation: operation arguments/callbacks may contain private user data.
#[derive(Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    BindingApps {},
    AccountChoices {
        app: AppId,
        page: Page,
    },
    ListAccounts {
        page: Page,
    },
    GetAccount {
        account: AccountId,
    },
    CreateStatic {
        input: StaticInput,
    },
    ReplaceStatic {
        account: AccountId,
        expected: AccountRevision,
        input: StaticInput,
    },
    Disconnect {
        account: AccountId,
        expected: AuthorizationRevision,
    },
    DeleteAccount {
        account: AccountId,
        expected: AuthorizationRevision,
    },
    InspectGrants {
        account: AccountId,
        after: Option<UseGrantId>,
    },
    ConnectableApps {},
    BeginConnect {
        app: AppId,
        config: AuthConfigId,
        capabilities: RequestedCapabilities,
        return_target: SafeReturnTarget,
        reconnect: Option<ReconnectTarget>,
    },
    FinishConnect {
        provider: ProviderId,
        state: SecretSubmission,
        code: Option<SecretSubmission>,
        error: Option<CallbackError>,
    },
    CancelConnect {
        attempt: ConnectAttemptId,
    },
    GetConnectAttempt {
        attempt: ConnectAttemptId,
    },
    CreateGrant {
        account: AccountId,
        expected: AuthorizationRevision,
        scope: HostBinding,
        binding: HostBinding,
        app: AppId,
        destination: ApprovedDestination,
        purpose: InvocationKind,
        host_authority: HostAuthorityReference,
    },
    RevokeGrant {
        grant: UseGrantId,
        expected: GrantRevision,
    },
    ExecuteHttp {
        invocation: Box<AuthorizedInvocation>,
    },
    CallMcp {
        invocation: Box<AuthorizedInvocation>,
    },
    DiscoverMcp {
        authorization: Box<AuthorizationSnapshot>,
        host_authority: HostAuthorityReference,
    },
}
impl Command {
    pub fn invocation(&self) -> Option<&AuthorizedInvocation> {
        match self {
            Self::ExecuteHttp { invocation } | Self::CallMcp { invocation } => Some(invocation),
            _ => None,
        }
    }
    pub(super) fn validate(&self) -> IntegrationResult<()> {
        match self {
            Self::AccountChoices { page, .. } | Self::ListAccounts { page } => {
                page.local()?;
            }
            _ => {}
        }
        if let Some(invocation) = self.invocation() {
            invocation.target.validate()?;
        }
        Ok(())
    }
}
