//! Sending one frozen mail through the Resend HTTP API.
//!
//! The interesting difference from SMTP is not the protocol, it is what a failure means. A relay
//! answers one `Err` for a refused connection and for a lost acknowledgement alike, so every
//! failure has to be treated as possibly-sent. Resend answers a status code, and honours an
//! `Idempotency-Key`; both of those turn guesses into facts, and this module is where that is
//! converted into the arms of [`MailSendOutcome`].

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use tracing::warn;

use crate::{
    adapters::{
        protocols::email::{MailMessage, MailSendOutcome, MailTransport},
        resend::client::{ResendApi, ResendError, ResendSendRequest},
    },
    entities::{
        transport::{ExternalMessageKey, FailureClass},
        value_objects::MessageId,
    },
};

/// Resend's own cap on an `Idempotency-Key`.
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;

/// The headers this adapter builds from typed fields, and therefore refuses to take from the
/// renderer's free-form header list.
///
/// Not a tidiness rule. Every one of these is threading or envelope state; two copies of
/// `In-Reply-To` in one request is a mail whose thread depends on which the provider happened to
/// keep, and that is not a coin this deployment should be flipping.
const RESERVED_HEADERS: [&str; 4] = ["message-id", "in-reply-to", "references", "subject"];

/// Posts one mail to `POST /emails`.
pub struct ResendMailTransport {
    api: Arc<dyn ResendApi>,
}

impl ResendMailTransport {
    pub fn new(api: Arc<dyn ResendApi>) -> Self {
        Self { api }
    }
}

#[async_trait]
impl MailTransport for ResendMailTransport {
    async fn send(&self, mail: MailMessage) -> MailSendOutcome {
        let message_id = mail.message_id.clone();
        let request = build_request(mail);
        match self.api.send_email(&request).await {
            Ok(response) => MailSendOutcome::Accepted {
                provider_key: accepted_key(message_id.as_ref(), &response.id),
            },
            // A definite refusal: a malformed address, an unverified sending domain, a revoked
            // key. Re-sending the identical body earns the identical answer.
            Err(ResendError::Refused { status, detail }) => MailSendOutcome::Rejected {
                class: refusal_class(status),
                detail: format!("resend refused the send ({status}): {detail}"),
            },
            Err(ResendError::RateLimited {
                retry_after,
                detail,
            }) => MailSendOutcome::RateLimited {
                retry_after,
                detail,
            },
            // The one place this transport is allowed to differ from SMTP's conservatism. A 5xx,
            // a dropped connection or a timeout may all have been acted on -- but the retry
            // replays the same `Idempotency-Key`, which is what turns "may have been sent" from a
            // duplicate risk into a request the provider will recognise and not send twice.
            Err(ResendError::Unavailable { detail }) => MailSendOutcome::Retryable {
                class: FailureClass::ProviderFault,
                detail,
            },
            // The request was accepted or refused and we could not read which. Nothing about an
            // idempotency key makes an unreadable answer safe to act on, so this stays ambiguous.
            Err(error @ (ResendError::TooLarge { .. } | ResendError::Malformed { .. })) => {
                MailSendOutcome::Unknown {
                    class: FailureClass::ProviderFault,
                    detail: error.to_string(),
                }
            }
        }
    }
}

/// The provider key to record, given what we asked to send under and what Resend answered.
///
/// `None` means "the rendered `Message-ID` still stands", which is the answer whenever Resend
/// honoured the header we set. It returns its own opaque id in every response, and that id is not
/// an RFC `Message-ID`, so it is recorded only when there was no `Message-ID` to honour -- a
/// platform notice that threads onto nothing.
fn accepted_key(message_id: Option<&MessageId>, provider_id: &str) -> Option<ExternalMessageKey> {
    if message_id.is_some() {
        return None;
    }
    ExternalMessageKey::parse(provider_id)
        .inspect_err(|error| {
            warn!(%error, "Resend returned an id that cannot be stored as a provider key");
        })
        .ok()
}

/// Which refusal this was, so the metric can tell a revoked key from a bad address.
fn refusal_class(status: u16) -> FailureClass {
    match status {
        401 | 403 => FailureClass::Authentication,
        404 | 410 => FailureClass::DestinationUnavailable,
        _ => FailureClass::InvalidPayload,
    }
}

/// One [`MailMessage`] as a Resend send request.
///
/// The threading headers travel as `headers` entries rather than as fields, because Resend has no
/// field for them and because `EmailRenderer` has already decided their exact contents -- the
/// `References` chain that ends at `In-Reply-To`, the `Re:` that appears exactly once. Nothing
/// here re-derives any of it.
fn build_request(mail: MailMessage) -> ResendSendRequest {
    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    if let Some(message_id) = mail.message_id.as_ref() {
        headers.insert("Message-ID".to_string(), message_id.to_string());
    }
    if let Some(in_reply_to) = mail.in_reply_to.as_ref().filter(|id| !id.is_empty()) {
        headers.insert("In-Reply-To".to_string(), in_reply_to.to_string());
    }
    if !mail.references.is_empty() {
        headers.insert(
            "References".to_string(),
            mail.references
                .iter()
                .map(MessageId::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    // The renderer's own set: `Auto-Submitted`, `X-Auto-Response-Suppress`, the correlation id and
    // the `X-MailAgents-*` hop trace. Losing any of them costs loop protection, so they are copied
    // rather than filtered -- but not over a header this function already decided.
    for header in mail.headers {
        if RESERVED_HEADERS.contains(&header.name.to_ascii_lowercase().as_str()) {
            warn!(
                header = %header.name,
                "Ignoring a rendered header that the Resend request builds from a typed field"
            );
            continue;
        }
        headers.insert(header.name, header.value);
    }

    ResendSendRequest {
        from: match mail.from_name.as_deref().map(str::trim).filter(|name| {
            // A display name carrying a quote or an angle bracket would change which address this
            // parses as. Dropping the name costs cosmetics; keeping it costs the envelope.
            !name.is_empty() && !name.contains(['"', '<', '>', '\r', '\n'])
        }) {
            Some(name) => format!("{name} <{}>", mail.from),
            None => mail.from.to_string(),
        },
        to: mail.recipients_to.iter().map(ToString::to_string).collect(),
        cc: mail.recipients_cc.iter().map(ToString::to_string).collect(),
        subject: mail.subject,
        text: mail.body_text,
        headers,
        // Deterministic in the delivery, not in the attempt: `EmailRenderer::message_id_for` is a
        // pure function of the part key, so every retry of one delivery replays one key and Resend
        // recognises the repeat instead of sending a second mail.
        idempotency_key: mail
            .message_id
            .map(|id| id.to_string())
            .filter(|key| key.len() <= MAX_IDEMPOTENCY_KEY_BYTES),
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
