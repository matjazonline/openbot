//! Code-owned app restrictions supplement the exact destination approved by the owner.
use crate::domain::integrations::{IntegrationError, IntegrationResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ApiPathPrefix(String);
impl ApiPathPrefix {
    pub fn parse(value: impl Into<String>) -> IntegrationResult<Self> {
        let value = value.into();
        if value.len() > 1024
            || !value.starts_with('/')
            || !value.ends_with('/')
            || value.starts_with("//")
            || value.contains(['%', '?', '#', '\\'])
            || value.split('/').any(|part| matches!(part, "." | ".."))
            || value.bytes().any(|b| !(33..=126).contains(&b))
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(Self(value))
    }
}
impl TryFrom<String> for ApiPathPrefix {
    type Error = IntegrationError;
    fn try_from(value: String) -> IntegrationResult<Self> {
        Self::parse(value)
    }
}
impl From<ApiPathPrefix> for String {
    fn from(value: ApiPathPrefix) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppTransportPolicy {
    OwnerApproved,
    OwnerApprovedHttp,
    HttpPaths {
        prefixes: Vec<ApiPathPrefix>,
    },
    /// The registration's destinations must contain the exact verified endpoint.
    McpEndpoint,
}
impl AppTransportPolicy {
    pub(crate) fn kind(
        &self,
        kind: crate::domain::integrations::authority::InvocationKind,
    ) -> IntegrationResult<()> {
        use crate::domain::integrations::authority::InvocationKind;
        match kind {
            InvocationKind::DeterministicHttp if !matches!(self, Self::McpEndpoint) => Ok(()),
            InvocationKind::DeterministicMcp => self.mcp(),
            _ => Err(IntegrationError::Denied),
        }
    }
    pub(crate) fn validate_registration(&self, destinations: usize) -> IntegrationResult<()> {
        match self {
            Self::HttpPaths { prefixes }
                if destinations == 0 || prefixes.is_empty() || prefixes.len() > 16 =>
            {
                Err(IntegrationError::Invalid)
            }
            Self::McpEndpoint if destinations == 0 => Err(IntegrationError::Invalid),
            _ => Ok(()),
        }
    }
    pub(crate) fn http(&self, url: &url::Url) -> IntegrationResult<()> {
        match self {
            Self::OwnerApproved | Self::OwnerApprovedHttp => Ok(()),
            Self::HttpPaths { prefixes }
                if url.scheme() == "https"
                    && !url.path().contains('%')
                    && prefixes
                        .iter()
                        .any(|prefix| url.path().starts_with(&prefix.0)) =>
            {
                Ok(())
            }
            _ => Err(IntegrationError::Denied),
        }
    }
    pub(crate) fn mcp(&self) -> IntegrationResult<()> {
        match self {
            Self::OwnerApproved | Self::McpEndpoint => Ok(()),
            Self::HttpPaths { .. } | Self::OwnerApprovedHttp => Err(IntegrationError::Denied),
        }
    }
}
