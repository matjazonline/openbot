use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    company_member::CompanyMembership,
    transport::{
        IdentityNamespace, IdentitySubject, ParticipantIdentityId, PrincipalId, TransportKind,
    },
    value_objects::EmailAddress,
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
    EmailIngress,
    SlackEvent,
    SlackProfileClaim,
    System,
}

impl IdentityProvenance {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Agent => "agent",
            Self::ChannelAllowlist => "channel_allowlist",
            Self::EmailIngress => "email_ingress",
            Self::SlackEvent => "slack_event",
            Self::SlackProfileClaim => "slack_profile_claim",
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
            "email_ingress" => Ok(Self::EmailIngress),
            "slack_event" => Ok(Self::SlackEvent),
            "slack_profile_claim" => Ok(Self::SlackProfileClaim),
            "system" => Ok(Self::System),
            _ => Err(InvalidParticipantValue::new("identity provenance", value)),
        }
    }
}

/// Versioned claims are enrichment only. In particular, a Slack profile email is never a key
/// used to merge principals or to grant access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdentityClaimMetadata {
    Observation {
        version: u8,
    },
    Account {
        version: u8,
    },
    SlackProfile {
        version: u8,
        profile_email: Option<EmailAddress>,
    },
}

impl IdentityClaimMetadata {
    pub const fn observation() -> Self {
        Self::Observation { version: 1 }
    }

    pub const fn account() -> Self {
        Self::Account { version: 1 }
    }

    /// A profile email a provider told us about. It is recorded so an operator can see what was
    /// claimed, and is deliberately not a key: nothing resolves, merges or authorizes by it.
    pub const fn slack_profile(profile_email: Option<EmailAddress>) -> Self {
        Self::SlackProfile {
            version: 1,
            profile_email,
        }
    }

    pub const fn version(&self) -> u8 {
        match self {
            Self::Observation { version }
            | Self::Account { version }
            | Self::SlackProfile { version, .. } => *version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantIdentity {
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
    EmailAllowlist,
    Manager,
    SlackConversation,
    System,
}

impl GrantProvenance {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmailAllowlist => "email_allowlist",
            Self::Manager => "manager",
            Self::SlackConversation => "slack_conversation",
            Self::System => "system",
        }
    }
}

impl FromStr for GrantProvenance {
    type Err = InvalidParticipantValue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "email_allowlist" => Ok(Self::EmailAllowlist),
            "manager" => Ok(Self::Manager),
            "slack_conversation" => Ok(Self::SlackConversation),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
