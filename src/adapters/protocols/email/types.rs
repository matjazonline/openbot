use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    transport::{
        EndpointNamespace, ExternalEndpointKey, ExternalMessageKey, IdentityNamespace,
        IdentitySubject, QualifiedIdentity, TransportKind, TransportValueError,
    },
    value_objects::{ChannelSlug, EmailAddress, MessageId},
};

/// A database is one email-identity namespace. Deployments do not share a database, so using a
/// stable discriminator lets account/bootstrap and ingress writers resolve the same mailbox
/// without smuggling runtime host configuration into persistence.
pub const EMAIL_IDENTITY_NAMESPACE: &str = "email";

/// The canonical inbound interface of one channel, as a binding endpoint key.
///
/// # Why the key is a bare local part
///
/// A channel answers at `{channel}@{company}.{app_domain}`, and only the middle piece --
/// `{channel}` -- is stable. `app_domain` is deployment configuration, and `companies.slug` is
/// editable in company settings. Putting either into the stored key means the key has to be
/// rewritten whenever they change, and every writer that forgets strands the binding at an address
/// that no longer resolves.
///
/// So the mutable and configured parts are kept out of it. The *namespace* carries the tenant as
/// its immutable company id, and the key carries the local part. This is the same shape the
/// installed transports already use -- a Slack binding's namespace is its workspace, the scope in
/// which a conversation id is unique -- rather than a special case for email.
///
/// What this leaves is exactly what a lookup has in hand:
/// `(company_id, channel_slug)`. Inbound routing parses the host off and resolves the company
/// before it can authorize anything anyway
/// ([`EmailChannelSelectorParser`](super::EmailChannelSelectorParser)), and the display form stays
/// a rendering concern where `Channel::inbound_address` already builds it from all three parts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmailEndpointKey(ExternalEndpointKey);

impl EmailEndpointKey {
    /// The channel's canonical local part.
    pub fn canonical(channel: &ChannelSlug) -> Result<Self, TransportValueError> {
        Ok(Self(ExternalEndpointKey::parse(channel.to_string())?))
    }

    /// The scope the local part is unique within: this company, by its immutable id.
    ///
    /// A slug would read better in `psql` and would be wrong -- it is the value that changes.
    pub fn namespace(company_id: Uuid) -> EndpointNamespace {
        EndpointNamespace::parse(company_id.to_string())
            .expect("a UUID is a valid endpoint namespace")
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_external(self) -> ExternalEndpointKey {
        self.0
    }
}

/// An email identity after adapter-owned mailbox parsing and case normalization.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct EmailIdentity(IdentitySubject);

#[derive(Debug, thiserror::Error)]
pub enum EmailIdentityError {
    #[error("invalid email address")]
    InvalidAddress,
    #[error(transparent)]
    InvalidSubject(#[from] TransportValueError),
}

impl<'de> Deserialize<'de> for EmailIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let address = String::deserialize(deserializer)?;
        Self::parse(EmailAddress::from(address)).map_err(serde::de::Error::custom)
    }
}

impl EmailIdentity {
    pub fn parse(address: EmailAddress) -> Result<Self, EmailIdentityError> {
        let mailbox: Mailbox = address
            .as_str()
            .trim()
            .parse()
            .map_err(|_| EmailIdentityError::InvalidAddress)?;
        let normalized = mailbox.email.to_string().to_ascii_lowercase();
        Ok(Self(IdentitySubject::parse(normalized)?))
    }

    pub fn subject(&self) -> &IdentitySubject {
        &self.0
    }

    pub fn qualify(self, namespace: IdentityNamespace) -> QualifiedIdentity {
        QualifiedIdentity::new(TransportKind::Email, namespace, self.0)
    }

    pub fn qualify_default(self) -> QualifiedIdentity {
        self.qualify(
            IdentityNamespace::parse(EMAIL_IDENTITY_NAMESPACE)
                .expect("the constant email identity namespace is valid"),
        )
    }
}

/// An RFC Message-ID represented as an opaque provider message key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct EmailMessageKey(ExternalMessageKey);

impl EmailMessageKey {
    pub fn parse(message_id: MessageId) -> Result<Self, TransportValueError> {
        Ok(Self(ExternalMessageKey::parse(
            message_id.as_str().trim().to_string(),
        )?))
    }

    pub fn into_external(self) -> ExternalMessageKey {
        self.0
    }

    pub fn as_external(&self) -> &ExternalMessageKey {
        &self.0
    }
}

impl<'de> Deserialize<'de> for EmailMessageKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let message_id = String::deserialize(deserializer)?;
        Self::parse(MessageId::from(message_id)).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_identity_normalization_is_case_insensitive() {
        let first = EmailIdentity::parse(EmailAddress::from("Person@Example.COM")).unwrap();
        let second = EmailIdentity::parse(EmailAddress::from("person@example.com")).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.subject().as_str(), "person@example.com");
    }

    #[test]
    fn a_channel_endpoint_key_holds_only_the_part_of_the_address_that_is_stable() {
        let acme = Uuid::new_v4();
        let key = EmailEndpointKey::canonical(&ChannelSlug::from("support")).unwrap();

        assert_eq!(key.as_str(), "support");
        assert_eq!(EmailEndpointKey::namespace(acme).as_str(), acme.to_string());
        // Two companies may both have a `support` channel. The key does not separate them; the
        // namespace does, and it is an id no rename can move.
        assert_eq!(
            key,
            EmailEndpointKey::canonical(&ChannelSlug::from("support")).unwrap()
        );
        assert_ne!(
            EmailEndpointKey::namespace(acme),
            EmailEndpointKey::namespace(Uuid::new_v4())
        );
    }

    #[test]
    fn malformed_email_is_rejected_at_the_adapter_boundary() {
        assert!(EmailIdentity::parse(EmailAddress::from("not an address")).is_err());
    }

    #[test]
    fn adapter_newtypes_round_trip_without_permissive_deserialization() {
        let identity = EmailIdentity::parse(EmailAddress::from("Person@Example.com")).unwrap();
        let encoded = serde_json::to_string(&identity).unwrap();
        assert_eq!(
            serde_json::from_str::<EmailIdentity>(&encoded).unwrap(),
            identity
        );
        assert!(serde_json::from_str::<EmailIdentity>("\"not an address\"").is_err());

        let key = EmailMessageKey::parse(MessageId::from("<message@example.com>")).unwrap();
        let encoded = serde_json::to_string(&key).unwrap();
        assert_eq!(
            serde_json::from_str::<EmailMessageKey>(&encoded).unwrap(),
            key
        );
        assert!(serde_json::from_str::<EmailMessageKey>("\"\"").is_err());
    }
}
