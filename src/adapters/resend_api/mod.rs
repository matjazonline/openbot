//! Resend: sending mail over its HTTP API, and receiving mail through its signed webhook.
//!
//! Resend is a *provider* for the existing email transport, not a transport of its own. Nothing
//! here decides what a mail says, how it threads, or whether it may be ingested: `protocols::email`
//! already owns all of that, and this module is the two ends of the wire it runs over.
//!
//! The asymmetry with SMTP is worth stating once. Outbound, an API that answers status codes and
//! honours an idempotency key can tell a definite refusal from an ambiguous one, which a relay
//! cannot -- see [`transport`]. Inbound, the webhook carries only metadata, so the mail itself
//! takes two further provider calls; that is why the route stores the event and answers, and the
//! work happens in [`inbound`] under the inbound worker's lease.

pub mod accounts;
pub mod client;
pub mod inbound;
pub mod signature;
#[cfg(test)]
pub mod test_support;
pub mod transport;

pub use accounts::{CompanyResendApiAccount, CompanyResendApiClients, ResendApiCompanyTransports};
pub use client::{ReceivedEmail, ReqwestResendApiClient, ResendApi, ResendApiError};
pub use inbound::ResendApiInboundDecoder;
pub use signature::{decode_signing_secret, verify_svix_signature_at};
pub use transport::ResendApiMailTransport;
