//! SendGrid inbound mail, split at the same provider boundary as [`super::resend_api`].
//!
//! SendGrid is another provider for the email transport, not a transport of its own. Signature
//! verification and the provider's multipart envelope live here; canonical email parsing,
//! routing, policy and persistence remain in `protocols::email` and the thread use case.

pub mod inbound;
pub mod signature;

pub use inbound::{MAX_SENDGRID_REQUEST_BODY_BYTES, decode_sendgrid_request};
pub use signature::{verify_sendgrid_signature, verify_sendgrid_signature_at};
