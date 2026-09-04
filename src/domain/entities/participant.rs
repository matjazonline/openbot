use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    company_member::CompanyMembership,
    transport::{
        IdentityNamespace, IdentitySubject, ParticipantIdentityId, PrincipalId, QualifiedIdentity,
        TransportKind,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Person,
    Agent,
    External,
    System,
}

impl PrincipalKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Agent => "agent",
            Self::External => "external",
            Self::System => "system",
        }
    }
}

impl FromStr for PrincipalKind {
    type Err = InvalidParticipantValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "person" => Ok(Self::Person),
            "agent" => Ok(Self::Agent),
            "external" => Ok(Self::External),
            "system" => Ok(Self::System),
            _ => Err(InvalidParticipantValue::new("principal kind", value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: PrincipalId,
    pub company_id: Uuid,
    pub kind: PrincipalKind,
    pub user_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub display_label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityStatus {
    Observed,
    Verified,
    Disabled,
}

impl IdentityStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Verified => "verified",
            Self::Disabled => "disabled",
        }
    }
}

impl FromStr for IdentityStatus {
    type Err = InvalidParticipantValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "observed" => Ok(Self::Observed),
            "verified" => Ok(Self::Verified),
            "disabled" => Ok(Self::Disabled),
            _ => Err(InvalidParticipantValue::new("identity status", value)),
        }
    }
}

/// Auditable origin of an identity-to-principal association.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityProvenance {
    Account,
    Agent,
    ChannelAllowlist,
    TransportIngress,
    ProviderProfileClaim,
    System,
}

impl IdentityProvenance {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Agent => "agent",
            Self::ChannelAllowlist => "channel_allowlist",
            Self::TransportIngress => "transport_ingress",
            Self::ProviderProfileClaim => "provider_profile_claim",
            Self::System => "system",
        }
    }
}

impl FromStr for IdentityProvenance {
    type Err = InvalidParticipantValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "account" => Ok(Self::Account),
            "agent" => Ok(Self::Agent),
            "channel_allowlist" => Ok(Self::ChannelAllowlist),
            "transport_ingress" => Ok(Self::TransportIngress),
            "provider_profile_claim" => Ok(Self::ProviderProfileClaim),
            "system" => Ok(Self::System),
            _ => Err(InvalidParticipantValue::new("identity provenance", value)),
        }
    }
}

/// Versioned provider claims are enrichment only and are never a key used to merge principals or
/// grant access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdentityClaimMetadata {
    Observation {
        version: u8,
    },
    Account {
        version: u8,
    },
    ProviderProfile {
        version: u8,
        claimed_identity: Option<QualifiedIdentity>,
    },
}

impl IdentityClaimMetadata {
    pub const fn observation() -> Self {
        Self::Observation { version: 1 }
    }

    pub const fn account() -> Self {
        Self::Account { version: 1 }
    }

    /// A profile identity a provider told us about. It is recorded for diagnostics only.
    pub const fn provider_profile(claimed_identity: Option<QualifiedIdentity>) -> Self {
        Self::ProviderProfile {
            version: 1,
            claimed_identity,
        }
    }

    pub const fn version(&self) -> u8 {
        match self {
            Self::Observation { version }
            | Self::Account { version }
            | Self::ProviderProfile { version, .. } => *version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalIdentity {
    pub id: ParticipantIdentityId,
    pub company_id: Uuid,
    pub principal_id: PrincipalId,
    pub transport: TransportKind,
    pub namespace: IdentityNamespace,
    pub subject: IdentitySubject,
    pub display_label: Option<String>,
    pub status: IdentityStatus,
    pub claim_metadata: IdentityClaimMetadata,
    pub provenance: IdentityProvenance,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalCapability {
    Participate,
    View,
}

impl PrincipalCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Participate => "participate",
            Self::View => "view",
        }
    }
}

impl FromStr for PrincipalCapability {
    type Err = InvalidParticipantValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "participate" => Ok(Self::Participate),
            "view" => Ok(Self::View),
            _ => Err(InvalidParticipantValue::new("principal capability", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantProvenance {
    ConfiguredAllowlist,
    Manager,
    ConversationMembership,
    System,
}

impl GrantProvenance {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfiguredAllowlist => "configured_allowlist",
            Self::Manager => "manager",
            Self::ConversationMembership => "conversation_membership",
            Self::System => "system",
        }
    }
}

impl FromStr for GrantProvenance {
    type Err = InvalidParticipantValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "configured_allowlist" => Ok(Self::ConfiguredAllowlist),
            "manager" => Ok(Self::Manager),
            "conversation_membership" => Ok(Self::ConversationMembership),
            "system" => Ok(Self::System),
            _ => Err(InvalidParticipantValue::new("grant provenance", value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelPrincipalGrant {
    pub principal_id: PrincipalId,
    pub capability: PrincipalCapability,
    pub provenance: GrantProvenance,
    pub created_at: DateTime<Utc>,
}

/// All actor facts needed by channel policy. Transport syntax is deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrincipalAccessContext {
    pub principal_id: Option<PrincipalId>,
    pub membership: CompanyMembership,
}

impl PrincipalAccessContext {
    pub const fn external(principal_id: PrincipalId) -> Self {
        Self {
            principal_id: Some(principal_id),
            membership: CompanyMembership::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadPrincipalRole {
    Author,
    Participant,
}

impl ThreadPrincipalRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Author => "author",
            Self::Participant => "participant",
        }
    }
}

impl FromStr for ThreadPrincipalRole {
    type Err = InvalidParticipantValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "author" => Ok(Self::Author),
            "participant" => Ok(Self::Participant),
            _ => Err(InvalidParticipantValue::new("thread principal role", value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {field} '{value}'")]
pub struct InvalidParticipantValue {
    field: &'static str,
    value: String,
}

impl InvalidParticipantValue {
    fn new(field: &'static str, value: impl Into<String>) -> Self {
        Self {
            field,
            value: value.into(),
        }
    }
}

impl fmt::Display for PrincipalCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_payloads_require_the_current_version() {
        let old = r#"{"kind":"observation","version":0}"#;
        let claim: IdentityClaimMetadata = serde_json::from_str(old).unwrap();
        assert_ne!(claim.version(), 1);
    }
}
