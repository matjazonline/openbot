use super::*;
use crate::application::integrations::{
    BindingApp, ConnectApp, ConnectAttempt, DiscoveredCatalog, ExecutionOutcome, GrantInspection,
};
use crate::domain::integrations::authority::UseGrant;

/// These finite error codes contain no remote messages or credential-bearing diagnostics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    NotFound,
    Denied,
    Conflict,
    Invalid,
    Limit,
    Unavailable,
    Credential,
    Storage,
}
impl From<IntegrationError> for Failure {
    fn from(error: IntegrationError) -> Self {
        match error {
            IntegrationError::NotFound => Self::NotFound,
            IntegrationError::Denied => Self::Denied,
            IntegrationError::Conflict => Self::Conflict,
            IntegrationError::Invalid => Self::Invalid,
            IntegrationError::Limit => Self::Limit,
            IntegrationError::Unavailable => Self::Unavailable,
            IntegrationError::Credential => Self::Credential,
            IntegrationError::Storage => Self::Storage,
        }
    }
}

// No private credential/resolver result or runtime capability has a projection here.
#[derive(Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    BindingApps {
        apps: Vec<BindingApp>,
    },
    Accounts {
        page: AccountPage,
    },
    Account {
        account: Box<ConnectedAccount>,
    },
    Grants {
        inspection: GrantInspection,
    },
    Grant {
        grant: UseGrant,
    },
    ConnectableApps {
        apps: Vec<ConnectApp>,
    },
    ConnectAttempt {
        attempt: ConnectAttempt,
    },
    /// Data only: decoding does not create a trusted browser navigation capability.
    ConnectStarted {
        attempt: ConnectAttempt,
        authorization_url: AuthorizationUrlData,
    },
    Catalog {
        catalog: DiscoveredCatalog,
    },
    Execution {
        outcome: ExecutionOutcome,
    },
    Done {},
    Failure {
        failure: Failure,
    },
}
impl Reply {
    pub(super) fn validate(&self) -> IntegrationResult<()> {
        match self {
            Self::Accounts { page } if page.accounts.len() > MAX_PAGE_SIZE => {
                Err(IntegrationError::Limit)
            }
            Self::Grants { inspection } if inspection.grants.len() > MAX_PAGE_SIZE => {
                Err(IntegrationError::Limit)
            }
            _ => Ok(()),
        }
    }
}
