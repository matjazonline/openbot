//! Turning one stored `email.received` event into a canonical commit.
//!
//! The webhook that stored the event did no provider work and made no routing decision: it proved
//! the request was Resend's, found the tenant it names, and answered. Everything expensive is
//! here, under the inbound worker's fenced lease, where a failure costs an attempt with backoff
//! instead of a webhook timeout and a redelivery of work already half-done.
//!
//! Three fetches make one mail: the stored envelope names an `email_id`, the retrieve endpoint
//! turns that into a signed URL, and the URL yields the raw MIME. From there this is the same path
//! the SMTP listener runs -- same parser, same ingress adapter, same preflight -- because the only
//! thing Resend changes is how the bytes arrived.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::warn;

use crate::{
    adapters::{
        protocols::email::{
            AuthenticationResults, EmailIngressAdapter, EmailIngressTrust, VerifiedEmailAuth,
            ingress::PendingEmailAttachments, parse_raw_mime_to_payload,
        },
        resend_api::{
            accounts::{CompanyResendApiAccount, CompanyResendApiClients},
            client::{ReceivedEmail, ResendApi, ResendApiError},
        },
        storage::FileStorage,
    },
    entities::transport::{InboundEventErrorClass, InboundEventIgnoreReason, TransportKind},
    infra::config::AppConfig,
    transport::{InboundEventDecodeOutcome, InboundEventDecoder, InboundEventRecord},
    use_cases::thread::{
        InboundMessage, InboundPreflight, IngressOrigin, ReplyDelivery, ThreadUseCases,
    },
};

/// The event this decoder acts on. Every other Resend event type -- deliveries, bounces, opens --
/// is ignored, and the webhook already declines to store one.
const RECEIVED_EVENT_TYPE: &str = "email.received";

/// The `email.received` envelope, reduced to the fields that decide anything.
///
/// Everything else Resend sends -- the subject, the recipient lists, the attachment metadata --
/// is read from the raw MIME instead. Two sources for one fact is two sources that can disagree,
/// and the MIME is the one that was actually signed by the sender.
#[derive(Debug, Clone, Deserialize)]
struct ReceivedEvent {
    #[serde(rename = "type")]
    event_type: String,
    data: ReceivedEventData,
}

#[derive(Debug, Clone, Deserialize)]
struct ReceivedEventData {
    email_id: String,
}

pub struct ResendApiInboundDecoder {
    /// The client is built per event, from the credential of the company the event belongs to.
    /// The mail behind a webhook is fetched with the *receiving* account's key, so a decoder
    /// holding one deployment-wide client would be a decoder that could read one tenant's mail
    /// with another tenant's authority.
    clients: Arc<CompanyResendApiClients>,
    config: Arc<AppConfig>,
    thread_use_cases: Arc<ThreadUseCases>,
    file_storage: Option<Arc<dyn FileStorage>>,
}

impl ResendApiInboundDecoder {
    pub fn new(
        clients: Arc<CompanyResendApiClients>,
        config: Arc<AppConfig>,
        thread_use_cases: Arc<ThreadUseCases>,
        file_storage: Option<Arc<dyn FileStorage>>,
    ) -> Self {
        Self {
            clients,
            config,
            thread_use_cases,
            file_storage,
        }
    }
}

/// Fetch the mail this event names, as raw MIME.
///
/// Two calls rather than one because the retrieve endpoint answers with a signed CDN URL rather
/// than the bytes; the URL is short-lived, which is why this happens at decode time and not when
/// the event was stored.
async fn fetch_raw_mime(
    api: &dyn ResendApi,
    email_id: &str,
) -> Result<(ReceivedEmail, Vec<u8>), Failure> {
    let email = api
        .retrieve_received(email_id)
        .await
        .map_err(|error| Failure::from_provider("retrieve a received mail", error))?;
    let Some(raw) = email.raw.as_ref() else {
        // Resend knows the mail but is not offering its bytes. Nothing here can produce them, and
        // asking again would ask the same question.
        return Err(Failure::terminal(
            InboundEventErrorClass::InvalidPayload,
            "the received mail carries no raw representation".to_string(),
        ));
    };
    let bytes = api
        .download_raw(&raw.download_url)
        .await
        .map_err(|error| Failure::from_provider("download a received mail", error))?;
    Ok((email, bytes))
}

#[async_trait]
impl InboundEventDecoder for ResendApiInboundDecoder {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }

    async fn decode(&self, event: &InboundEventRecord) -> InboundEventDecodeOutcome {
        match self.decode_event(event).await {
            Ok(outcome) => outcome,
            Err(failure) => failure.into_outcome(),
        }
    }
}

impl ResendApiInboundDecoder {
    /// The decode, with one exit for every failure.
    ///
    /// Split from the trait method so each step can use `?` rather than nesting; the trait method
    /// is the only place a [`Failure`] becomes an [`InboundEventDecodeOutcome`].
    async fn decode_event(
        &self,
        event: &InboundEventRecord,
    ) -> Result<InboundEventDecodeOutcome, Failure> {
        let Some(email_id) = event_email_id(event)? else {
            return Ok(InboundEventDecodeOutcome::Ignore(
                InboundEventIgnoreReason::UnsupportedEvent,
            ));
        };
        // Whose account this event arrived into. A company that has disconnected Resend, or
        // switched it off, since the event was stored has no key to fetch the mail with -- and
        // this deployment has no other key it would be right to use.
        let account = self
            .clients
            .account_for(event.company_id)
            .await
            .map_err(Failure::from_app)?
            .ok_or_else(|| {
                Failure::terminal(
                    InboundEventErrorClass::UnsupportedTransport,
                    "this company has no enabled Resend integration".to_string(),
                )
            })?;
        let (inbound, attachments) = read_mail(&account, &self.config, event, &email_id).await?;

        let preflight = self
            .thread_use_cases
            .preflight_inbound(inbound)
            .await
            .map_err(Failure::from_app)?;

        let mut prepared = match preflight {
            InboundPreflight::Rejected(result) => {
                // A refused message still owes its sender an answer, and the bounce goes on the
                // same delivery queue as everything else -- awaited here rather than detached, so
                // a failure to queue it fails this attempt instead of vanishing.
                self.thread_use_cases
                    .handle_bounce_dispatch(&result)
                    .await
                    .map_err(Failure::from_app)?;
                return Ok(InboundEventDecodeOutcome::Ignore(ignore_reason(&result)));
            }
            InboundPreflight::Accepted(prepared) => prepared,
        };

        let persisted = attachments
            .persist(&self.config, self.file_storage.as_deref())
            .await
            .map_err(|error| {
                Failure::terminal(InboundEventErrorClass::InvalidPayload, error.to_string())
            })?;
        prepared.replace_attachments(
            persisted.metadata,
            persisted.stored_count,
            persisted.failed_count,
        );
        Ok(InboundEventDecodeOutcome::Message(Box::new(
            prepared.into_commit_request(),
        )))
    }
}

/// Fetch the mail this event names and read it into the canonical ingress vocabulary.
///
/// Everything up to the point the application is first asked a question, and a free function for
/// exactly that reason: the provider calls, the verdicts, the parse and the two keys the worker
/// will check need a scripted API and a configuration, and nothing else -- no thread store, no
/// channel, no tenant.
async fn read_mail(
    account: &CompanyResendApiAccount,
    config: &AppConfig,
    event: &InboundEventRecord,
    email_id: &str,
) -> Result<(InboundMessage, PendingEmailAttachments), Failure> {
    let (email, raw_mime) = fetch_raw_mime(account.api.as_ref(), email_id).await?;

    // The verdicts are the receiving MTA's, and only the one this deployment named may make
    // them. A message whose top `Authentication-Results` is missing, or belongs to anyone
    // else, arrives with every verdict `Unknown`, and `guard_ingress` refuses it below.
    let results = AuthenticationResults::from_raw_mime(&raw_mime, &account.authserv_id);
    // `received_for` is the address Resend accepted the mail *for*, which is the platform
    // address even when a mailing list rewrote `To:`. It is the routing fact; `to` is what the
    // message claims.
    let recipient = email
        .received_for
        .first()
        .or_else(|| email.to.first())
        .cloned()
        .unwrap_or_default();
    let payload = parse_raw_mime_to_payload(
        &raw_mime,
        Some(&email.from),
        Some(&recipient),
        std::slice::from_ref(&recipient),
        results.spf,
        results.dkim,
        results.dmarc,
    );

    let accepted = EmailIngressAdapter::for_config(config)
        .accept(
            payload,
            EmailIngressTrust::Verified(VerifiedEmailAuth {
                spf: results.spf,
                dkim: results.dkim,
                dmarc: results.dmarc,
                spam_score: None,
            }),
        )
        .map_err(|error| {
            Failure::terminal(InboundEventErrorClass::InvalidPayload, error.to_string())
        })?;

    let (mut inbound, attachments) =
        accepted.into_preflight_parts(IngressOrigin::ExternalTransport, ReplyDelivery::Send);
    // The commit has to name the event it completes, and share its correlation id: the worker
    // refuses a request that does either differently, because the event row and the message it
    // became are one piece of work and a log line has to be able to join them. The mail's own
    // correlation header therefore does not survive a Resend hop -- the durable event was
    // created before anything had read the mail, and its id is the one every line from the
    // webhook onward already carries.
    inbound.draft.event_key = Some(event.external_event_key.clone());
    inbound.draft.correlation_id = event.correlation_id;
    Ok((inbound, attachments))
}

/// The mail id this event names, or `None` when the event is not one this decoder acts on.
///
/// Free-standing and synchronous: it is the whole of what the stored bytes decide, and a test for
/// it needs no decoder, no configuration and no provider.
fn event_email_id(event: &InboundEventRecord) -> Result<Option<String>, Failure> {
    let envelope: ReceivedEvent = serde_json::from_slice(event.payload.as_bytes())
        .map_err(|error| Failure::terminal(InboundEventErrorClass::Decode, error.to_string()))?;
    if envelope.event_type != RECEIVED_EVENT_TYPE {
        return Ok(None);
    }
    Ok(Some(envelope.data.email_id))
}

/// Why an authenticated event produced no canonical message.
///
/// The inbox's vocabulary is transport-neutral and deliberately small, so several distinct email
/// refusals share one reason. What each one actually was is already recorded on the ingest result
/// the bounce was composed from; this is the census bucket.
fn ignore_reason(
    result: &crate::use_cases::thread::InboundIngestResult,
) -> InboundEventIgnoreReason {
    use crate::use_cases::thread::IngestRejection;
    match result.rejection {
        Some(IngestRejection::AutoReply) => InboundEventIgnoreReason::AutomatedSender,
        Some(
            IngestRejection::UnknownRecipient
            | IngestRejection::Undeliverable(_)
            | IngestRejection::SystemAddressAnswered,
        ) => InboundEventIgnoreReason::InactiveBinding,
        _ => InboundEventIgnoreReason::NotMessage,
    }
}

/// A decode that did not produce a message, and whether asking again could change that.
#[derive(Debug)]
struct Failure {
    class: InboundEventErrorClass,
    detail: String,
    retry: bool,
}

impl Failure {
    fn terminal(class: InboundEventErrorClass, detail: String) -> Self {
        Self {
            class,
            detail,
            retry: false,
        }
    }

    /// A provider call that did not answer usefully.
    ///
    /// The retry decision is the provider error's own: a rate limit or an outage will read
    /// differently in a minute, and a 404 for an email id or a body this build cannot parse will
    /// not. Deciding it here from a status code would be the same table written twice.
    fn from_provider(what: &str, error: ResendApiError) -> Self {
        let retry = error.is_transient();
        let class = match &error {
            ResendApiError::RateLimited { .. } => InboundEventErrorClass::RateLimited,
            ResendApiError::Unavailable { .. } => InboundEventErrorClass::ProviderFault,
            ResendApiError::Refused { .. } => InboundEventErrorClass::Routing,
            ResendApiError::TooLarge { .. } | ResendApiError::Malformed { .. } => {
                InboundEventErrorClass::InvalidPayload
            }
        };
        if !retry {
            warn!(%error, "Resend could not {what}, and asking again would not help");
        }
        Self {
            class,
            detail: format!("could not {what}: {error}"),
            retry,
        }
    }

    /// A failure of ours -- a database read, an object store. Retryable: nothing was committed.
    fn from_app(error: crate::app_error::AppError) -> Self {
        Self {
            class: InboundEventErrorClass::Internal,
            detail: error.to_string(),
            retry: true,
        }
    }

    fn into_outcome(self) -> InboundEventDecodeOutcome {
        let detail = bounded_detail(&self.detail);
        if self.retry {
            InboundEventDecodeOutcome::Retry {
                class: self.class,
                detail,
            }
        } else {
            InboundEventDecodeOutcome::Terminal {
                class: self.class,
                detail,
            }
        }
    }
}

/// A provider's own words, truncated to fit. Truncated rather than dropped: a detail that will not
/// fit must not be the reason a failure goes unrecorded.
fn bounded_detail(message: &str) -> crate::transport::InboundFailureDetail {
    use crate::transport::InboundFailureDetail;
    let mut detail = message.to_string();
    while InboundFailureDetail::parse(detail.clone()).is_err() && !detail.is_empty() {
        detail.truncate(detail.len().saturating_sub(detail.len() / 4 + 1));
    }
    InboundFailureDetail::parse(detail).unwrap_or_else(|_| {
        InboundFailureDetail::parse("a failure detail that could not be recorded")
            .expect("a fixed short string is within the failure-detail bound")
    })
}

#[cfg(test)]
#[path = "inbound_tests.rs"]
mod tests;
