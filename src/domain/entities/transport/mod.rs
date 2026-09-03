//! Protocol-neutral transport identifiers and routing intent.
//!
//! Provider-specific syntax is validated by the owning adapter.  The types in this module make
//! qualification and bounds unavoidable once a value crosses into the domain.

mod integration;

pub use integration::{
    BindingAccessPolicy, BindingAccessSnapshot, BindingAuditAction, BindingAuditEvent,
    BindingAuditMetadata, BindingChangeReason, BindingDeliveryPolicy, BindingStatus,
    ChannelBinding, InstallationStatus, IntegrationCredentialKind, IntegrationInstallation,
    InvalidTransportValue,
};

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    message::MessageParticipantKind,
    value_objects::{ChannelSlug, CompanySlug, EmailAddress},
};

/// A deliberately supported transport. Adding a variant requires an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Email,
    Slack,
}

impl TransportKind {
    /// Every supported transport, so a test can assert the whole set against the database at once.
    pub const ALL: &'static [Self] = &[Self::Email, Self::Slack];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Slack => "slack",
        }
    }

    /// Whether reaching this transport needs a company-scoped provider account.
    ///
    /// Email is a *deployment* transport: the server owns its own MX namespace, so a channel is
    /// reachable the moment it has an address. Slack is an *installed* transport: nothing can be
    /// read or sent until a workspace has granted this company an installation.
    ///
    /// `transport_requires_installation()` in the migration is the same decision in SQL, and
    /// `rust_and_sql_agree_on_which_transports_require_an_installation` proves the two agree for
    /// every variant -- a new transport that disagrees fails that test rather than silently
    /// letting a binding exist with no credential behind it.
    pub const fn requires_installation(self) -> bool {
        match self {
            Self::Email => false,
            Self::Slack => true,
        }
    }
}

impl fmt::Display for TransportKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for TransportKind {
    type Err = UnsupportedTransport;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "email" => Ok(Self::Email),
            "slack" => Ok(Self::Slack),
            _ => Err(UnsupportedTransport(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported transport '{0}'")]
pub struct UnsupportedTransport(String);

impl UnsupportedTransport {
    /// Names a transport this deployment cannot speak. Also raised for a [`TransportKind`] with no
    /// registered adapter, which is the same fact one step later.
    pub fn new(transport: impl Into<String>) -> Self {
        Self(transport.into())
    }
}

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub const fn new(value: Uuid) -> Self {
                Self(value)
            }

            pub fn random() -> Self {
                Self(Uuid::new_v4())
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.0, formatter)
            }
        }
    };
}

// Re-exported so sibling entity modules can mint their own id newtypes from the same definition
// rather than hand-rolling a fourth variant of it.
pub(crate) use uuid_id;

uuid_id!(InstallationId);
uuid_id!(ChannelBindingId);
uuid_id!(BindingAuditEventId);
uuid_id!(PrincipalId);
uuid_id!(ParticipantIdentityId);
uuid_id!(DeliveryId);
// `InboundEventId` names one row of the durable inbound inbox: a bounded, authenticated provider
// event awaiting ingestion. Declared here with the other correlation ids so a lease over it is
// typed from the day the port that claims it is written.
uuid_id!(InboundEventId);

pub const MAX_IDENTITY_NAMESPACE_BYTES: usize = 255;
pub const MAX_IDENTITY_SUBJECT_BYTES: usize = 320;
pub const MAX_ENDPOINT_NAMESPACE_BYTES: usize = 255;
pub const MAX_EXTERNAL_TENANT_KEY_BYTES: usize = 255;
pub const MAX_EXTERNAL_ENDPOINT_KEY_BYTES: usize = 512;
pub const MAX_EXTERNAL_EVENT_KEY_BYTES: usize = 512;
pub const MAX_EXTERNAL_THREAD_KEY_BYTES: usize = 998;
pub const MAX_EXTERNAL_MESSAGE_KEY_BYTES: usize = 998;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransportValueError {
    #[error("value must not be empty")]
    Empty,
    #[error("value exceeds its {max_bytes}-byte limit")]
    TooLong { max_bytes: usize },
    #[error("value contains a control character")]
    ControlCharacter,
}

/// The single definition of "bounded, non-empty, no control characters".
///
/// `pub(crate)` rather than private because [`bounded_string`] is re-exported for the application
/// layer's own bounded values, and a second copy of these three rules is exactly how one of them
/// drifts.
pub(crate) fn validate_bounded(value: &str, max_bytes: usize) -> Result<(), TransportValueError> {
    if value.trim().is_empty() {
        return Err(TransportValueError::Empty);
    }
    if value.len() > max_bytes {
        return Err(TransportValueError::TooLong { max_bytes });
    }
    if value.chars().any(char::is_control) {
        return Err(TransportValueError::ControlCharacter);
    }
    Ok(())
}

macro_rules! bounded_string {
    ($name:ident, $max:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(
                value: impl Into<String>,
            ) -> Result<Self, $crate::entities::transport::TransportValueError> {
                let value = value.into();
                $crate::entities::transport::validate_bounded(&value, $max)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = $crate::entities::transport::TransportValueError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let value = <String as ::serde::Deserialize>::deserialize(deserializer)?;
                Self::parse(value).map_err(::serde::de::Error::custom)
            }
        }
    };
}

// Re-exported for the same reason as `uuid_id`: the application layer's transport contracts mint
// their own bounded strings, and they must be bounded by these rules rather than by a second copy.
pub(crate) use bounded_string;

bounded_string!(IdentityNamespace, MAX_IDENTITY_NAMESPACE_BYTES);
bounded_string!(EndpointNamespace, MAX_ENDPOINT_NAMESPACE_BYTES);
bounded_string!(ExternalTenantKey, MAX_EXTERNAL_TENANT_KEY_BYTES);
bounded_string!(ExternalEndpointKey, MAX_EXTERNAL_ENDPOINT_KEY_BYTES);
bounded_string!(IdentitySubject, MAX_IDENTITY_SUBJECT_BYTES);
bounded_string!(ExternalEventKey, MAX_EXTERNAL_EVENT_KEY_BYTES);
bounded_string!(ExternalThreadKey, MAX_EXTERNAL_THREAD_KEY_BYTES);
bounded_string!(ExternalMessageKey, MAX_EXTERNAL_MESSAGE_KEY_BYTES);

/// Which addressing role one identity held on a message.
///
/// Transport vocabulary: what an adapter states about an arriving message, what the reply planner
/// reads back, and what a queued run records about each channel it drives. It lives in the domain
/// because `background_tasks`' channel targets carry it, and the domain may not reach upward for a
/// type its own entities hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecipientRole {
    To,
    Cc,
}

impl RecipientRole {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::To => "to",
            Self::Cc => "cc",
        }
    }

    /// The role a stored `to`/`cc` string names.
    ///
    /// Fallible rather than defaulting to `To`: a row whose role this build does not recognise is
    /// a schema/deploy mismatch, and silently treating it as a direct recipient would run an agent
    /// for a channel that was only copied.
    pub fn parse(value: &str) -> Result<Self, UnknownRecipientRole> {
        match value.trim().to_ascii_lowercase().as_str() {
            "to" => Ok(Self::To),
            "cc" => Ok(Self::Cc),
            _ => Err(UnknownRecipientRole(value.to_string())),
        }
    }

    /// How this role is stored on the canonical message.
    pub const fn participant_kind(self) -> MessageParticipantKind {
        match self {
            Self::To => MessageParticipantKind::To,
            Self::Cc => MessageParticipantKind::Cc,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown recipient role '{0}'")]
pub struct UnknownRecipientRole(String);

impl From<UnknownRecipientRole> for crate::app_error::AppError {
    fn from(error: UnknownRecipientRole) -> Self {
        Self::Internal(error.to_string())
    }
}

/// A provider identity qualified by the scope in which its subject is unique.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QualifiedIdentity {
    transport: TransportKind,
    namespace: IdentityNamespace,
    subject: IdentitySubject,
}

impl QualifiedIdentity {
    /// Combines already-validated pieces. Protocol adapters own any normalization before this call.
    pub fn new(
        transport: TransportKind,
        namespace: IdentityNamespace,
        subject: IdentitySubject,
    ) -> Self {
        Self {
            transport,
            namespace,
            subject,
        }
    }

    pub const fn transport(&self) -> TransportKind {
        self.transport
    }

    pub const fn namespace(&self) -> &IdentityNamespace {
        &self.namespace
    }

    pub const fn subject(&self) -> &IdentitySubject {
        &self.subject
    }
}

/// Transport-neutral intent to address a business channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelSelector {
    CurrentCompany(ChannelSlug),
    Qualified {
        company: CompanySlug,
        channel: ChannelSlug,
    },
}

/// A selector text that names no channel, or names one in a shape this platform cannot address.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidChannelSelector {
    #[error("a channel selector cannot be empty")]
    Empty,
    #[error("a channel selector is 'channel' or 'company/channel', not '{0}'")]
    Shape(String),
}

impl ChannelSelector {
    /// Read a selector as an agent or an operator writes it: `support`, or `acme/support`.
    ///
    /// This is the whole grammar. It deliberately does not accept an address: an address is one
    /// transport's way of reaching a channel, and parsing one here is how the routing key came to
    /// be an email string. A caller holding an address resolves it through that transport's
    /// adapter instead.
    pub fn parse(value: &str) -> Result<Self, InvalidChannelSelector> {
        let value = value.trim();
        if value.is_empty() {
            return Err(InvalidChannelSelector::Empty);
        }
        let mut parts = value.split('/');
        let first = parts.next().unwrap_or_default().trim();
        let second = parts.next().map(str::trim);
        if parts.next().is_some() || first.is_empty() || second.is_some_and(str::is_empty) {
            return Err(InvalidChannelSelector::Shape(value.to_string()));
        }
        Ok(match second {
            Some(channel) => Self::Qualified {
                company: CompanySlug::from(first.to_lowercase()),
                channel: ChannelSlug::from(channel.to_lowercase()),
            },
            None => Self::CurrentCompany(ChannelSlug::from(first.to_lowercase())),
        })
    }

    pub fn channel(&self) -> &ChannelSlug {
        match self {
            Self::CurrentCompany(channel) | Self::Qualified { channel, .. } => channel,
        }
    }

    pub fn company(&self) -> Option<&CompanySlug> {
        match self {
            Self::CurrentCompany(_) => None,
            Self::Qualified { company, .. } => Some(company),
        }
    }
}

impl fmt::Display for ChannelSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentCompany(channel) => write!(formatter, "{channel}"),
            Self::Qualified { company, channel } => write!(formatter, "{company}/{channel}"),
        }
    }
}

/// A destination outside the business-channel selector namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExternalDestination {
    Email(EmailAddress),
}

/// Binding-qualified source correlation for one inbound provider message.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InboundSource {
    pub binding_id: ChannelBindingId,
    pub event_key: Option<ExternalEventKey>,
    pub message_key: ExternalMessageKey,
    pub thread_key: ExternalThreadKey,
}

/// A binding-qualified message key that may identify the message being replied to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReplyMessageKeyCandidate {
    pub binding_id: ChannelBindingId,
    pub message_key: ExternalMessageKey,
}

/// A binding-qualified thread key that may identify the conversation being replied to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReplyThreadKeyCandidate {
    pub binding_id: ChannelBindingId,
    pub thread_key: ExternalThreadKey,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;

    /// The grammar is two shapes, and an address is not one of them: accepting
    /// `support@acme.example.com` here is exactly how a transport's addressing became the
    /// platform's routing key.
    #[test]
    fn a_channel_selector_reads_a_channel_or_a_company_and_channel_and_nothing_else() {
        assert_eq!(
            ChannelSelector::parse("  Support "),
            Ok(ChannelSelector::CurrentCompany(ChannelSlug::from(
                "support"
            )))
        );
        assert_eq!(
            ChannelSelector::parse("Acme/Support"),
            Ok(ChannelSelector::Qualified {
                company: CompanySlug::from("acme"),
                channel: ChannelSlug::from("support"),
            })
        );

        for rejected in ["", "   ", "/", "support/", "/support", "a/b/c"] {
            assert!(
                ChannelSelector::parse(rejected).is_err(),
                "expected {rejected:?} to be refused"
            );
        }
    }

    /// Round-trips through the text the directory tool hands a model, so an agent copying a
    /// selector back gets the channel it was shown.
    #[test]
    fn a_selector_round_trips_through_its_displayed_form() {
        for selector in [
            ChannelSelector::CurrentCompany(ChannelSlug::from("support")),
            ChannelSelector::Qualified {
                company: CompanySlug::from("acme"),
                channel: ChannelSlug::from("support"),
            },
        ] {
            assert_eq!(
                ChannelSelector::parse(&selector.to_string()),
                Ok(selector.clone())
            );
        }
    }

    fn assert_round_trip<T>(value: T)
    where
        T: fmt::Debug + PartialEq + Serialize + DeserializeOwned,
    {
        let encoded = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<T>(&encoded).unwrap(), value);
    }

    #[test]
    fn bounded_values_reject_empty_control_and_oversized_input() {
        for value in ["", "   ", "line\nbreak"] {
            assert!(IdentityNamespace::parse(value).is_err());
            assert!(IdentitySubject::parse(value).is_err());
            assert!(ExternalEventKey::parse(value).is_err());
            assert!(ExternalThreadKey::parse(value).is_err());
            assert!(ExternalMessageKey::parse(value).is_err());
        }

        assert!(IdentityNamespace::parse("x".repeat(MAX_IDENTITY_NAMESPACE_BYTES + 1)).is_err());
        assert!(IdentitySubject::parse("x".repeat(MAX_IDENTITY_SUBJECT_BYTES + 1)).is_err());
        assert!(ExternalEventKey::parse("x".repeat(MAX_EXTERNAL_EVENT_KEY_BYTES + 1)).is_err());
        assert!(ExternalThreadKey::parse("x".repeat(MAX_EXTERNAL_THREAD_KEY_BYTES + 1)).is_err());
        assert!(ExternalMessageKey::parse("x".repeat(MAX_EXTERNAL_MESSAGE_KEY_BYTES + 1)).is_err());
    }

    #[test]
    fn slack_subjects_retain_case_and_installations_qualify_identity() {
        let subject = IdentitySubject::parse("UAbC123").unwrap();
        assert_eq!(subject.as_str(), "UAbC123");

        let first = QualifiedIdentity::new(
            TransportKind::Slack,
            IdentityNamespace::parse("installation-a").unwrap(),
            subject.clone(),
        );
        let second = QualifiedIdentity::new(
            TransportKind::Slack,
            IdentityNamespace::parse("installation-b").unwrap(),
            subject,
        );
        assert_ne!(first, second);
    }

    #[test]
    fn the_same_provider_key_in_two_bindings_is_distinct() {
        let key = ExternalMessageKey::parse("1712345678.123456").unwrap();
        let first = ReplyMessageKeyCandidate {
            binding_id: ChannelBindingId::random(),
            message_key: key.clone(),
        };
        let second = ReplyMessageKeyCandidate {
            binding_id: ChannelBindingId::random(),
            message_key: key,
        };
        assert_ne!(first, second);
    }

    #[test]
    fn validated_strings_round_trip_and_invalid_json_is_rejected() {
        assert_round_trip(InstallationId::random());
        assert_round_trip(ChannelBindingId::random());
        assert_round_trip(PrincipalId::random());
        assert_round_trip(ParticipantIdentityId::random());
        assert_round_trip(DeliveryId::random());
        assert_round_trip(IdentityNamespace::parse("installation-a").unwrap());
        assert_round_trip(IdentitySubject::parse("CaseSensitive").unwrap());
        assert_round_trip(ExternalEventKey::parse("event-1").unwrap());
        assert_round_trip(ExternalThreadKey::parse("thread-1").unwrap());
        assert_round_trip(ExternalMessageKey::parse("message-1").unwrap());
        assert_round_trip(QualifiedIdentity::new(
            TransportKind::Slack,
            IdentityNamespace::parse("installation-a").unwrap(),
            IdentitySubject::parse("U123").unwrap(),
        ));
        assert_round_trip(ChannelSelector::Qualified {
            company: CompanySlug::new("acme"),
            channel: ChannelSlug::new("support"),
        });
        assert_round_trip(ExternalDestination::Email(EmailAddress::from(
            "person@example.com",
        )));

        let binding_id = ChannelBindingId::random();
        assert_round_trip(InboundSource {
            binding_id,
            event_key: Some(ExternalEventKey::parse("event-1").unwrap()),
            message_key: ExternalMessageKey::parse("message-1").unwrap(),
            thread_key: ExternalThreadKey::parse("thread-1").unwrap(),
        });
        assert_round_trip(ReplyMessageKeyCandidate {
            binding_id,
            message_key: ExternalMessageKey::parse("message-1").unwrap(),
        });
        assert_round_trip(ReplyThreadKeyCandidate {
            binding_id,
            thread_key: ExternalThreadKey::parse("thread-1").unwrap(),
        });
        assert!(serde_json::from_str::<IdentitySubject>("\"\"").is_err());
        assert!(serde_json::from_str::<IdentitySubject>("\"   \"").is_err());
        assert!(serde_json::from_str::<ExternalMessageKey>("\"bad\\nkey\"").is_err());
    }
}
