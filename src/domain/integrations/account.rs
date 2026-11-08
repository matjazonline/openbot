use super::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountOwner {
    pub namespace: OwnerNamespace,
    pub subject: OwnerSubject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    StaticSecret,
    OAuthAuthorizationCode,
}
impl AuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StaticSecret => "static_secret",
            Self::OAuthAuthorizationCode => "oauth_authorization_code",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Ready,
    ReconnectRequired,
    Disconnected,
}
impl AccountStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::ReconnectRequired => "reconnect_required",
            Self::Disconnected => "disconnected",
        }
    }
}

/// Only owner-visible display fields. Tokens and ciphertext have no metadata slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MetadataFields", into = "MetadataFields")]
pub struct AccountMetadata(MetadataFields);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataFields {
    pub label: String,
    pub description: Option<String>,
}
impl AccountMetadata {
    pub fn new(label: String, description: Option<String>) -> IntegrationResult<Self> {
        Self::try_from(MetadataFields { label, description })
    }
    pub fn label(&self) -> &str {
        &self.0.label
    }
    pub fn description(&self) -> Option<&str> {
        self.0.description.as_deref()
    }
}
impl TryFrom<MetadataFields> for AccountMetadata {
    type Error = IntegrationError;
    fn try_from(value: MetadataFields) -> IntegrationResult<Self> {
        if value.label.trim().is_empty()
            || value.label.len() > 256
            || serde_json::to_vec(&value)
                .map_err(|_| IntegrationError::Invalid)?
                .len()
                > MAX_METADATA_BYTES
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(Self(value))
    }
}
impl From<AccountMetadata> for MetadataFields {
    fn from(value: AccountMetadata) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalIdentity {
    pub kind: ExternalIdentityKind,
    pub subject: ExternalSubject,
    pub authorization_key: AuthorizationKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectedAccount {
    pub id: AccountId,
    pub owner: AccountOwner,
    pub key: AccountKey,
    pub provider_id: ProviderId,
    pub auth_config_id: AuthConfigId,
    pub auth_method: AuthMethod,
    pub credential_schema: CredentialSchema,
    pub external_identity: Option<ExternalIdentity>,
    pub revision: AccountRevision,
    pub authorization_revision: AuthorizationRevision,
    pub status: AccountStatus,
    pub metadata: AccountMetadata,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialChange {
    Replace,
    Reconnect,
    Disconnect,
    Refresh,
    RenewalState,
    Rewrap,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialVersions {
    pub revision: AccountRevision,
    pub authorization: AuthorizationRevision,
    pub generation: CredentialGeneration,
}
impl CredentialVersions {
    pub const INITIAL: Self = Self {
        revision: AccountRevision::INITIAL,
        authorization: AuthorizationRevision::INITIAL,
        generation: CredentialGeneration::INITIAL,
    };
    /// One transition table for all storage writers. Rewrap is ciphertext-only CAS.
    pub fn changed(self, change: CredentialChange) -> IntegrationResult<Self> {
        if change == CredentialChange::Rewrap {
            return Ok(self);
        }
        Ok(Self {
            revision: self.revision.next()?,
            authorization: if matches!(
                change,
                CredentialChange::Refresh | CredentialChange::RenewalState
            ) {
                self.authorization
            } else {
                self.authorization.next()?
            },
            generation: if change == CredentialChange::RenewalState {
                self.generation
            } else {
                self.generation.next()?
            },
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountPage {
    pub accounts: Vec<ConnectedAccount>,
    pub next: Option<AccountId>,
}
#[derive(Debug, Clone, Copy)]
pub struct PageRequest {
    after: Option<AccountId>,
    size: usize,
}
impl PageRequest {
    pub fn new(after: Option<AccountId>, size: usize) -> IntegrationResult<Self> {
        if size == 0 || size > MAX_PAGE_SIZE {
            return Err(IntegrationError::Limit);
        }
        Ok(Self { after, size })
    }
    pub fn after(self) -> Option<AccountId> {
        self.after
    }
    pub fn size(self) -> usize {
        self.size
    }
}
