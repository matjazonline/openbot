//! The email transport, end to end: address syntax, MIME parsing, and the ingress adapter that
//! turns one arriving mail into a canonical [`InboundEnvelope`](crate::transport::InboundEnvelope).
//!
//! Egress is the same boundary in reverse: [`EmailRenderer`] turns a resolved delivery into one
//! frozen RFC 5322 message and [`EmailSender`] posts it, so the application hands over canonical
//! content and a destination and never composes a header itself.
//!
//! Everything email-shaped stops here. The application layer receives qualified identities,
//! bounded content, typed policy facts and channel selectors; it never sees a header, a MIME part
//! or a [`ParsedEmail`](parser::ParsedEmail).

pub mod attachments;
pub mod egress;
pub mod ingress;
pub mod mail;
pub mod parser;
mod selector;
#[cfg(test)]
pub mod test_support;
mod types;

pub use egress::{EmailRenderer, EmailSender, OUTBOUND_EMAIL_VERSION, OutboundEmailV1};
pub use ingress::{EmailIngressAdapter, EmailIngressError, EmailIngressTrust, VerifiedEmailAuth};
pub use mail::{
    DisabledMailTransport, MailHeader, MailMessage, MailTransport, SmtpConfirmationSender,
};
pub use selector::{
    EmailChannelSelection, EmailChannelSelectorParser, EmailDeliveryHints, EmailDeliveryMode,
    EmailRecipientDestination,
};
pub use types::{
    EMAIL_IDENTITY_NAMESPACE, EmailEndpointKey, EmailIdentity, EmailIdentityError, EmailMessageKey,
};
