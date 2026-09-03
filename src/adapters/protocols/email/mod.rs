//! The email transport, end to end: address syntax, MIME parsing, and the ingress adapter that
//! turns one arriving mail into a canonical [`InboundEnvelope`](crate::transport::InboundEnvelope).
//!
//! Everything email-shaped stops here. The application layer receives qualified identities,
//! bounded content, typed policy facts and channel selectors; it never sees a header, a MIME part
//! or a [`ParsedEmail`](parser::ParsedEmail).

pub mod attachments;
pub mod ingress;
pub mod parser;
mod selector;
mod types;

pub use ingress::{EmailIngressAdapter, EmailIngressError, EmailIngressTrust, VerifiedEmailAuth};
pub use selector::{
    EmailChannelSelection, EmailChannelSelectorParser, EmailDeliveryHints, EmailDeliveryMode,
    EmailRecipientDestination,
};
pub use types::{
    EMAIL_IDENTITY_NAMESPACE, EmailEndpointKey, EmailIdentity, EmailIdentityError, EmailMessageKey,
};
