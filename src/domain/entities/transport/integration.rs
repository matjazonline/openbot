//! Provider accounts a company has installed, and the protocol interfaces a business channel
//! exposes through them.
//!
//! A [`Channel`](crate::entities::channel::Channel) is not an inbox and not a conversation. It
//! owns agents, policy and threads; a [`ChannelBinding`] is one protocol-facing interface onto it.
//! That is what lets a channel gain a second transport without a nullable column per provider,
//! and what lets one interface be paused while the rest of the channel keeps working.
//!
//! Nothing here carries a token. Credentials live in their own narrow store keyed by
//! [`IntegrationCredentialKind`], so an installation can be listed, logged and serialized without
//! a secret ever entering the projection.

use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    BindingAuditEventId, ChannelBindingId, EndpointNamespace, ExternalEndpointKey,
    ExternalTenantKey, InstallationId, InvalidTransportValue, TransportKind, stored_enum,
};
use crate::entities::creation::CreationProvenance;

stored_enum! {
    /// Whether a provider account can currently be used, and if not, why not.
    ///
    /// `Revoked` is terminal and records who revoked it; `ReauthorizationRequired` is the
    /// provider telling us the grant lapsed, and `Disabled` is a manager switching the
    /// integration off without giving up the grant.
    InstallationStatus as "installation status" {
        Active => "active",
        ReauthorizationRequired => "reauthorization_required",
        Revoked => "revoked",
        Disabled => "disabled",
    }
}

impl InstallationStatus {
    /// Whether traffic may flow through this installation right now.
    ///
    /// Every ingress and delivery query joins the installation and applies this rule in SQL, so
    /// revoking an installation stops its bindings immediately without rewriting a binding row.
    pub const fn is_usable(self) -> bool {
        matches!(self, Self::Active)
    }
}

stored_enum! {
    /// What a binding does with traffic.
    ///
    /// `Paused` still owns its external endpoint -- the conversation stays claimed by this
    /// channel -- which is what separates it from `Disabled` and `Orphaned`, both of which
    /// release the claim so the endpoint can be linked somewhere else.
    BindingStatus as "binding status" {
        Active => "active",
        Paused => "paused",
        Disabled => "disabled",
        Orphaned => "orphaned",
    }
}

impl BindingStatus {
    /// Whether the binding carries traffic.
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Whether the binding still claims its external endpoint.
    ///
    /// The partial unique indexes in the migration are defined over exactly this set, so the two
    /// statements of the rule cannot drift; `binding_status_live_set_matches_sql` proves it.
    pub const fn holds_endpoint_claim(self) -> bool {
        matches!(self, Self::Active | Self::Paused)
    }
}

stored_enum! {
    /// Who may read and write through this interface.
    ///
    /// `ChannelAcl` defers entirely to the channel's principal grants.
    /// `ConversationMembersReadAndParticipate` is the explicit, audited grant that linking a
    /// private provider conversation makes to every current *and future* member of it -- the name
    /// is the one `docs/transport_architecture.md` fixes, spelled out because "members" alone does
    /// not say that it also lets them submit messages.
    BindingAccessPolicy as "binding access policy" {
        ChannelAcl => "channel_acl",
        ConversationMembersReadAndParticipate => "conversation_members_read_and_participate",
    }
}

stored_enum! {
    /// Whether this interface may start conversations or only answer them.
    BindingDeliveryPolicy as "binding delivery policy" {
        /// Replies only, into a conversation someone else opened.
        ReplyOnly => "reply_only",
        /// Replies plus agent-initiated outreach on this interface.
        ReplyAndInitiate => "reply_and_initiate",
    }
}

stored_enum! {
    /// Why a binding changed state. Typed, because an audit reason recovered by parsing a
    /// free-text error is not an audit reason.
    BindingChangeReason as "binding change reason" {
        /// A manager asked for it in the product.
        ManagerRequest => "manager_request",
        /// The provider account behind the binding stopped being usable.
        InstallationRevoked => "installation_revoked",
        /// The external conversation or address no longer exists.
        EndpointRemoved => "endpoint_removed",
        /// The read grant that made the link legitimate was withdrawn.
        AccessRevoked => "access_revoked",
        /// The business channel itself was switched off.
        ChannelDisabled => "channel_disabled",
        /// Provider state and stored state disagree; a human has to look.
        ProviderDrift => "provider_drift",
    }
}

stored_enum! {
    /// One append-only lifecycle record's verb.
    BindingAuditAction as "binding audit action" {
        Linked => "linked",
        /// The interface stayed, its external address moved -- a channel renaming its slug.
        EndpointChanged => "endpoint_changed",
        Enabled => "enabled",
        Paused => "paused",
        Disabled => "disabled",
        DriftDetected => "drift_detected",
        Unlinked => "unlinked",
    }
}

stored_enum! {
    /// Which secret of an installation is being addressed.
    ///
    /// The kind is part of the credential's authenticated context, so a bot token cannot be read
    /// back through a request for the user token even by an operator writing SQL by hand.
    IntegrationCredentialKind as "integration credential kind" {
        BotAccessToken => "bot_access_token",
        BotRefreshToken => "bot_refresh_token",
        UserAccessToken => "user_access_token",
    }
}

/// A provider account one company has installed.
///
/// `external_tenant_key` is the provider's own identifier for the account -- a Slack team id.
/// It is unique across the whole deployment on purpose: one external workspace installing into
/// two app companies would let either company's managers see the other's conversations, and there
/// is no v1 requirement that needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationInstallation {
    pub id: InstallationId,
    pub company_id: Uuid,
    pub transport: TransportKind,
    pub external_tenant_key: ExternalTenantKey,
    pub display_name: String,
    pub status: InstallationStatus,
    /// What the provider actually granted, as the provider named it. Recorded so a missing scope
    /// is diagnosable without another round trip; never used as a substitute for handling the
    /// provider's own authorization errors.
    #[serde(default)]
    pub granted_scopes: Vec<String>,
    pub installed_by: CreationProvenance,
    pub installed_at: DateTime<Utc>,
    pub updated_by: CreationProvenance,
    pub updated_at: DateTime<Utc>,
    pub revoked_by: Option<CreationProvenance>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl IntegrationInstallation {
    pub const fn is_usable(&self) -> bool {
        self.status.is_usable()
    }
}

/// What was true about the external endpoint when a human confirmed the link.
///
/// Versioned and discriminated because it is persisted JSON: a rolling deploy can read a shape it
/// does not know yet, and must fail to decode rather than guess. It records confirmations, never
/// provider responses -- no member lists, no message content, no tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BindingAccessSnapshot {
    /// This deployment's own address namespace. Nothing external was confirmed because nothing
    /// external grants it: the server owns the domain.
    DeploymentEndpoint { version: u8 },
    /// A private provider conversation, as it was described at link time.
    ProviderConversation {
        version: u8,
        is_private: bool,
        is_shared: bool,
        member_count: u32,
    },
}

impl BindingAccessSnapshot {
    pub const fn deployment_endpoint() -> Self {
        Self::DeploymentEndpoint { version: 1 }
    }

    pub const fn provider_conversation(
        is_private: bool,
        is_shared: bool,
        member_count: u32,
    ) -> Self {
        Self::ProviderConversation {
            version: 1,
            is_private,
            is_shared,
            member_count,
        }
    }

    pub const fn version(&self) -> u8 {
        match self {
            Self::DeploymentEndpoint { version } | Self::ProviderConversation { version, .. } => {
                *version
            }
        }
    }
}

/// One protocol-facing interface onto a business channel.
///
/// `installation_id` is `None` exactly when the transport does not require one -- see
/// [`TransportKind::requires_installation`]. The database states the same rule as a `CHECK`, and
/// the composite foreign key proves the installation belongs to `company_id` *and* speaks
/// `transport`, so a binding cannot borrow another tenant's provider account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelBinding {
    pub id: ChannelBindingId,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub installation_id: Option<InstallationId>,
    pub transport: TransportKind,
    /// The scope in which `external_endpoint_key` is unique: a provider workspace for an
    /// installed transport, this deployment's address namespace for email.
    pub namespace: EndpointNamespace,
    pub external_endpoint_key: ExternalEndpointKey,
    pub display_label: String,
    pub access_policy: BindingAccessPolicy,
    pub delivery_policy: BindingDeliveryPolicy,
    pub status: BindingStatus,
    pub disabled_reason: Option<BindingChangeReason>,
    pub created_by: CreationProvenance,
    pub access_snapshot: BindingAccessSnapshot,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ChannelBinding {
    pub const fn is_active(&self) -> bool {
        self.status.is_active()
    }

    /// The audit payload for a lifecycle transition on this binding: safe identifiers plus the
    /// access decision that was confirmed. Built here so no call site can assemble its own and
    /// include something it should not.
    pub fn audit_metadata(&self) -> BindingAuditMetadata {
        BindingAuditMetadata {
            version: 1,
            transport: self.transport,
            installation_id: self.installation_id,
            external_endpoint_key: self.external_endpoint_key.clone(),
            access_policy: self.access_policy,
            access_snapshot: self.access_snapshot.clone(),
        }
    }
}

/// Bounded, safe context for one audit record. Deliberately not a place to put a provider
/// response: everything here is an identifier this deployment already stores in the clear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingAuditMetadata {
    pub version: u8,
    pub transport: TransportKind,
    pub installation_id: Option<InstallationId>,
    pub external_endpoint_key: ExternalEndpointKey,
    pub access_policy: BindingAccessPolicy,
    pub access_snapshot: BindingAccessSnapshot,
}

/// One append-only record of a binding lifecycle transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingAuditEvent {
    pub id: BindingAuditEventId,
    pub company_id: Uuid,
    pub binding_id: ChannelBindingId,
    pub action: BindingAuditAction,
    pub reason: Option<BindingChangeReason>,
    pub actor: CreationProvenance,
    pub metadata: BindingAuditMetadata,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `as_str` writes the database column, serde writes the persisted JSON, and `FromStr` reads
    /// both back. All three have to agree on one spelling per variant: a serde name that drifts
    /// from `as_str` would put a value into `binding_audit_events.metadata` that the reader for
    /// that column cannot parse, on rows that already exist.
    fn assert_one_spelling<T>(variants: &[T])
    where
        T: Copy + fmt::Debug + PartialEq + FromStr + Serialize + serde::de::DeserializeOwned,
        T::Err: fmt::Debug,
    {
        for variant in variants {
            let stored = serde_json::to_value(variant).unwrap();
            let spelling = stored.as_str().expect("a stored enum encodes as a string");
            assert_eq!(&T::from_str(spelling).unwrap(), variant);
            assert_eq!(
                &serde_json::from_value::<T>(stored.clone()).unwrap(),
                variant
            );
        }
    }

    #[test]
    fn stored_enums_have_one_spelling_across_sql_serde_and_parsing() {
        assert_one_spelling(InstallationStatus::ALL);
        assert_one_spelling(BindingStatus::ALL);
        assert_one_spelling(BindingAccessPolicy::ALL);
        assert_one_spelling(BindingDeliveryPolicy::ALL);
        assert_one_spelling(BindingChangeReason::ALL);
        assert_one_spelling(BindingAuditAction::ALL);
        assert_one_spelling(IntegrationCredentialKind::ALL);

        assert!(BindingStatus::from_str("enabled").is_err());
    }

    #[test]
    fn only_active_and_paused_bindings_claim_their_endpoint() {
        assert!(BindingStatus::Active.holds_endpoint_claim());
        assert!(BindingStatus::Paused.holds_endpoint_claim());
        assert!(!BindingStatus::Disabled.holds_endpoint_claim());
        assert!(!BindingStatus::Orphaned.holds_endpoint_claim());
        assert!(BindingStatus::Active.is_active());
        assert!(!BindingStatus::Paused.is_active());
    }

    #[test]
    fn an_access_snapshot_of_an_unknown_shape_fails_to_decode() {
        let known = r#"{"kind":"provider_conversation","version":1,"is_private":true,
                        "is_shared":false,"member_count":4}"#;
        assert_eq!(
            serde_json::from_str::<BindingAccessSnapshot>(known).unwrap(),
            BindingAccessSnapshot::provider_conversation(true, false, 4)
        );
        assert!(serde_json::from_str::<BindingAccessSnapshot>(r#"{"kind":"mailbox"}"#).is_err());
    }

    #[test]
    fn an_error_names_the_field_without_inventing_a_variant() {
        let error = IntegrationCredentialKind::from_str("signing_secret").unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid integration credential kind 'signing_secret'"
        );
    }
}
