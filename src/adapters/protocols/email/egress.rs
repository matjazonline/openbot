//! Email egress: freezing one delivery into an RFC 5322 message, and posting it.
//!
//! The split matters. [`EmailRenderer`] makes every mail decision -- the `From` mailbox, the `Re:`
//! prefix, which addresses survive the `Cc` line, the `Message-ID`, the loop-control headers --
//! deterministically and with no I/O, and freezes the answer as one part. [`EmailSender`] decodes
//! that part and hands it to the transport. Nothing about the message is decided at send time, so
//! a retry three hours later sends the bytes that were agreed when the delivery was queued rather
//! than re-deriving them from a channel that has since been renamed.
//!
//! That is also what fixes the seam this replaces. `EmailEgressAdapter` was handed a normalized
//! message carrying neither a channel name nor a company slug, and reconstructed placeholders for
//! both; the renderer here is handed a resolved
//! [`EmailDeliveryContext`](crate::transport::EmailDeliveryContext) with the real mailbox in it.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        correlation::{CORRELATION_HEADER, CorrelationId},
        transport::{ExternalMessageKey, FailureClass, TransportKind},
        value_objects::{EmailAddress, MessageId},
    },
    services::outbound_dispatcher::{MailHeader, MailMessage, MailTransport},
    transport::{
        ContentDigest, DeliveryEnvelope, DeliveryRecord, EmailDeliveryContext, FailureDetail,
        InternalMailRelay, InternalRelayMail, PartIndex, PartKey, ProviderSendOutcome,
        RelayDisposition, RenderedPart, TransportPayload, TransportRenderer, TransportSender,
    },
};

/// The version of the frozen email payload. Bumping it makes every already-queued part fail to
/// decode loudly instead of being read as a shape it is not.
pub const OUTBOUND_EMAIL_VERSION: u16 = 1;

/// Email renders exactly one part.
///
/// Not a limitation to be lifted later: a mail message has no length bound that would justify
/// splitting one answer into several mails, and doing so would break threading for every client.
/// The multi-part machinery exists for chat providers, which do.
const EMAIL_PART_INDEX: PartIndex = PartIndex::new(0);

/// One RFC 5322 message, frozen. This is what the delivery part payload holds.
///
/// Every mail decision is already made here: the mailbox it comes from, the `Re:` prefix, which
/// addresses survived the `Cc` line, the `Message-ID` a retry will reuse. Nothing is a credential
/// and nothing is an authorization header -- the transport supplies its own authentication from
/// configuration, and `relay` carries only channel ids this deployment already stores in the clear.
///
/// The wire headers are *derived* from `relay` and `correlation_id` rather than stored beside
/// them. Storing both would be one fact written twice in the same object, and the derivation is a
/// pure function of frozen values, so a retry three hours later still emits the same header block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundEmailV1 {
    pub from: EmailAddress,
    pub from_name: Option<String>,
    pub recipients_to: Vec<EmailAddress>,
    pub recipients_cc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: String,
    pub message_id: MessageId,
    pub in_reply_to: Option<MessageId>,
    pub references: Vec<MessageId>,
    /// The chain this mail belongs to, stamped onto the wire so a recipient channel stays on it.
    pub correlation_id: CorrelationId,
    /// Loop control for mail one channel's agent sends. `None` for a platform notice, which
    /// carries no `X-MailAgents-*` headers and so cannot be answered into a loop.
    pub relay: Option<OutboundRelayV1>,
}

/// The inter-channel hop budget this mail carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundRelayV1 {
    pub source_channel_id: Uuid,
    /// Hops already taken. The wire header carries this plus one; ingress refuses beyond
    /// [`MAX_INGRESS_HOPS`](crate::transport::MAX_INGRESS_HOPS).
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
}

impl OutboundEmailV1 {
    fn into_mail_message(self) -> MailMessage {
        let headers = self.wire_headers();
        MailMessage {
            from: self.from,
            from_name: self.from_name,
            recipients_to: self.recipients_to,
            recipients_cc: self.recipients_cc,
            subject: self.subject,
            body_text: self.body_text,
            message_id: Some(self.message_id),
            in_reply_to: self.in_reply_to,
            references: self.references,
            headers,
        }
    }

    /// The headers this mail goes out with.
    ///
    /// `Auto-Submitted: auto-replied` is on every one of them, because `check_inbound_guards`
    /// refuses mail carrying it -- which is what stops two channels answering each other for ever.
    /// The `X-MailAgents-*` set is present only for a relayed channel message: a platform notice
    /// with no relay trace is deliberately unanswerable, so it offers a recipient no hop count to
    /// continue.
    fn wire_headers(&self) -> Vec<MailHeader> {
        let header = |name: &str, value: String| MailHeader {
            name: name.to_string(),
            value,
        };
        let mut headers = vec![
            header("Auto-Submitted", "auto-replied".to_string()),
            header("X-Auto-Response-Suppress", "All".to_string()),
        ];
        let (Some(relay), Some((hop_count, trace))) = (self.relay.as_ref(), self.wire_relay())
        else {
            return headers;
        };

        headers.push(header(
            "X-MailAgents-Channel-ID",
            relay.source_channel_id.to_string(),
        ));
        // The hop this mail *is*, not the hop it answers: ingress compares the received value
        // against `MAX_INGRESS_HOPS`, so incrementing here is what makes the budget finite.
        headers.push(header("X-MailAgents-Hop-Count", hop_count.to_string()));
        headers.push(header(CORRELATION_HEADER, self.correlation_id.to_string()));
        headers.push(header(
            "X-MailAgents-Trace",
            trace
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ));
        headers
    }

    /// This mail as the internal relay reads it.
    ///
    /// The hop count and the trace are the *wire* values -- what the headers would have carried --
    /// because the relay is a transport and what a transport delivers is what it would have sent.
    /// Handing over the pre-increment values instead would make an internal hop free, and the
    /// inter-channel loop budget would never be spent.
    fn as_relay_mail(&self) -> Option<InternalRelayMail<'_>> {
        let (hop_count, _) = self.wire_relay()?;
        Some(InternalRelayMail {
            from: &self.from,
            recipient_to: self.recipients_to.first()?,
            subject: &self.subject,
            body_text: &self.body_text,
            message_id: &self.message_id,
            in_reply_to: self.in_reply_to.as_ref(),
            references: &self.references,
            source_channel_id: self.relay.as_ref()?.source_channel_id,
            hop_count,
            trace: self.wire_relay()?.1,
            correlation_id: self.correlation_id,
        })
    }

    /// The hop count and channel trace this mail carries, as one decision.
    ///
    /// Read by the headers and by the internal relay, so a hop that leaves over SMTP and one that
    /// is ingested in process are counted identically. Two copies of this arithmetic is how an
    /// internal loop becomes cheaper than an external one.
    fn wire_relay(&self) -> Option<(u32, Vec<Uuid>)> {
        let relay = self.relay.as_ref()?;
        let mut trace = relay.trace_channels.clone();
        if !trace.contains(&relay.source_channel_id) {
            trace.push(relay.source_channel_id);
        }
        Some((relay.hop_count + 1, trace))
    }
}

/// Freezes one delivery into a single outbound mail.
///
/// Stateless apart from the deployment's own domain, which is what a `Message-ID` is qualified by.
#[derive(Debug, Clone)]
pub struct EmailRenderer {
    app_domain_name: Arc<str>,
}

impl EmailRenderer {
    pub fn new(app_domain_name: impl AsRef<str>) -> Self {
        Self {
            app_domain_name: Arc::from(app_domain_name.as_ref()),
        }
    }

    /// The `Message-ID` one part always goes out under.
    ///
    /// A pure function of the part key, which is itself derived from the delivery's idempotency
    /// key. Two consequences, both load-bearing: every attempt at the same logical delivery goes
    /// out under one `Message-ID`, so a recipient's client threads a retried mail onto the
    /// original rather than beside it; and whoever queued the delivery can compute the same value
    /// before anything is sent, which is what lets the canonical message and the mail that carries
    /// it agree without waiting for delivery.
    pub fn message_id_for(&self, part_key: &PartKey) -> MessageId {
        let digest = Sha256::digest(part_key.as_str().as_bytes());
        let local_part: String = digest
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        MessageId::from(format!("<delivery-{local_part}@{}>", self.app_domain_name))
    }

    /// The part key one email delivery always freezes under.
    ///
    /// Derived from the delivery's stable idempotency key rather than from its id, because the id
    /// is minted by whoever won the insert race and the key is what both racers computed.
    pub fn part_key(idempotency_key: &str) -> AppResult<PartKey> {
        PartKey::parse(format!("email:{idempotency_key}"))
            .map_err(|error| AppError::Internal(format!("Could not key an email part: {error}")))
    }
}

impl TransportRenderer for EmailRenderer {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }

    fn render(&self, envelope: &DeliveryEnvelope) -> AppResult<Vec<RenderedPart>> {
        let context = envelope.context.email().ok_or_else(|| {
            AppError::Internal(format!(
                "The email renderer was handed a {} delivery context",
                envelope.context.transport()
            ))
        })?;

        let key = Self::part_key(envelope.intent.key.as_str())?;
        let message_id = self.message_id_for(&key);
        let body_text = envelope.content.body_text.clone();
        let email = OutboundEmailV1 {
            from: context.from.clone(),
            from_name: context.from_name.clone(),
            recipients_to: vec![context.recipient_to.clone()],
            recipients_cc: self.copied_addresses(context),
            subject: reply_subject(&envelope.content.subject),
            body_text: body_text.clone(),
            message_id: message_id.clone(),
            in_reply_to: context.in_reply_to.clone(),
            references: thread_references(context),
            correlation_id: envelope.correlation_id,
            relay: context.relay.as_ref().map(|relay| OutboundRelayV1 {
                source_channel_id: relay.source_channel_id,
                hop_count: relay.hop_count,
                trace_channels: relay.trace_channels.clone(),
            }),
        };

        Ok(vec![RenderedPart {
            index: EMAIL_PART_INDEX,
            key,
            payload: TransportPayload::encode(TransportKind::Email, OUTBOUND_EMAIL_VERSION, &email)
                .map_err(|error| {
                    AppError::Internal(format!("Could not freeze an outbound email: {error}"))
                })?,
            // Over the body alone, because that is what a reconciliation lookup can compare
            // against a message it finds at the provider.
            digest: ContentDigest::sha256_of(body_text.as_bytes()),
        }])
    }

    /// Mail's provider key is the `Message-ID` this renderer chose, so it is known before the
    /// relay is ever called.
    fn predicted_provider_key(&self, part: &RenderedPart) -> Option<ExternalMessageKey> {
        ExternalMessageKey::parse(self.message_id_for(&part.key).as_str()).ok()
    }
}

impl EmailRenderer {
    /// Who the mail is copied to, minus the addresses that must not appear on a `Cc` line.
    ///
    /// The recipient and the sender are removed because naming them twice is how a mail client
    /// shows a duplicate; every address inside this deployment's own domain is removed because a
    /// platform address on a `Cc` line is an inbound message waiting to happen, and the pipeline
    /// that wanted one has its own delivery.
    fn copied_addresses(&self, context: &EmailDeliveryContext) -> Vec<EmailAddress> {
        let domain_suffix = format!(".{}", self.app_domain_name);
        let mut copied: Vec<EmailAddress> = Vec::new();
        for address in &context.recipients_cc {
            let trimmed = address.trim().to_ascii_lowercase();
            if trimmed.eq_ignore_ascii_case(&context.recipient_to)
                || trimmed.eq_ignore_ascii_case(&context.from)
                || trimmed.ends_with(&domain_suffix)
                || trimmed == *self.app_domain_name
                || copied.iter().any(|seen| seen.eq_ignore_ascii_case(address))
            {
                continue;
            }
            copied.push(address.clone());
        }
        copied
    }
}

/// The `References:` chain, with the parent appended when it is not already in it.
fn thread_references(context: &EmailDeliveryContext) -> Vec<MessageId> {
    let mut references = context.references.clone();
    if let Some(parent) = context.in_reply_to.as_ref()
        && !references.contains(parent)
    {
        references.push(parent.clone());
    }
    references
}

/// `Re:` exactly once, however the subject arrived.
fn reply_subject(subject: &str) -> String {
    let trimmed = subject.trim();
    if trimmed.is_empty() {
        return "Re:".to_string();
    }
    if trimmed.len() >= 3 && trimmed[..3].eq_ignore_ascii_case("re:") {
        return trimmed.to_string();
    }
    format!("Re: {trimmed}")
}

/// Posts one frozen part: in process when the recipient is one of this deployment's own channels,
/// and over SMTP otherwise.
///
/// The transport is shared, not built per message: one `lettre` relay holds its TLS configuration,
/// its timeouts and its connection reuse, and constructing a new one per send would discard all
/// three and open a fresh TLS handshake for every mail.
pub struct EmailSender {
    transport: Arc<dyn MailTransport>,
    internal: Arc<dyn InternalMailRelay>,
}

impl EmailSender {
    pub fn new(transport: Arc<dyn MailTransport>, internal: Arc<dyn InternalMailRelay>) -> Self {
        Self {
            transport,
            internal,
        }
    }

    /// Try the in-process path first, and report what it decided.
    ///
    /// A mail with no relay trace is never internal: a platform notice comes from the deployment
    /// rather than from a channel, so it has no source channel to authenticate the hop against.
    async fn relay_internally(
        &self,
        email: &OutboundEmailV1,
        delivery: &DeliveryRecord,
    ) -> Option<ProviderSendOutcome> {
        let mail = email.as_relay_mail()?;
        match self.internal.relay_internal(&mail).await {
            Ok(RelayDisposition::Relayed) => Some(ProviderSendOutcome::Delivered {
                provider_key: provider_key(&email.message_id, delivery),
            }),
            Ok(RelayDisposition::NotInternal) => None,
            // A refusal from one of our own channels is definite and will read the same way next
            // time: the channel is disabled, the hop budget is spent, the sender is not authorized.
            Ok(RelayDisposition::Refused(reason)) => Some(ProviderSendOutcome::Terminal {
                class: FailureClass::DestinationUnavailable,
                detail: detail(&reason),
            }),
            // The relay could not decide -- a database read failed. Nothing was ingested, so this
            // is plainly retryable rather than ambiguous.
            Err(error) => Some(ProviderSendOutcome::Retryable {
                class: FailureClass::Internal,
                detail: detail(&error.to_string()),
            }),
        }
    }
}

#[async_trait]
impl TransportSender for EmailSender {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }

    async fn send(&self, delivery: &DeliveryRecord, part: &RenderedPart) -> ProviderSendOutcome {
        let email: OutboundEmailV1 = match part
            .payload
            .decode(TransportKind::Email, OUTBOUND_EMAIL_VERSION)
        {
            Ok(email) => email,
            // A payload this build cannot read will not become readable on the fifth attempt, so
            // it is terminal rather than retryable. The renderer and the decoder are the same
            // version in any single deployment; a mismatch means a rolling deploy queued a shape
            // this process does not know, and an operator has to decide.
            Err(error) => {
                return ProviderSendOutcome::Terminal {
                    class: FailureClass::InvalidPayload,
                    detail: detail(&error.to_string()),
                };
            }
        };
        if let Some(outcome) = self.relay_internally(&email, delivery).await {
            return outcome;
        }
        let message_id = email.message_id.clone();

        match self.transport.send(email.into_mail_message()).await {
            // The relay accepted the message and the `Message-ID` we chose is the provider's key
            // for it: SMTP returns no identifier of its own, and this is the value a recipient's
            // `References:` header will name when they reply.
            Ok(()) => ProviderSendOutcome::Delivered {
                provider_key: provider_key(&message_id, delivery),
            },
            // One `Err` for every SMTP failure is all the transport port offers, and a submission
            // that failed may still have been queued by the relay -- an accepted `DATA` whose
            // final acknowledgement was lost is indistinguishable here from a refused connection.
            // Ambiguity is the honest classification: it is reconciled or dead-lettered, never
            // silently sent twice.
            //
            // Retrying anyway would be the previous shape's behaviour, and the duplicate it
            // produces is the failure this whole queue exists to avoid.
            Err(error) => {
                warn!(
                    delivery_id = %delivery.id,
                    correlation_id = %delivery.correlation_id,
                    %error,
                    "A mail submission failed without a definite verdict"
                );
                ProviderSendOutcome::OutcomeUnknown {
                    class: FailureClass::Network,
                    detail: detail(&error.to_string()),
                }
            }
        }
    }
}

/// The `Message-ID` as a provider key, or nothing when it will not fit the stored bound.
///
/// Losing the key costs reconciliation and outreach reply-matching for this one delivery; refusing
/// the send over it would cost the delivery itself, which is worse.
fn provider_key(message_id: &MessageId, delivery: &DeliveryRecord) -> Option<ExternalMessageKey> {
    match ExternalMessageKey::parse(message_id.as_str()) {
        Ok(key) => Some(key),
        Err(error) => {
            warn!(
                delivery_id = %delivery.id,
                %error,
                "A delivered mail's Message-ID cannot be stored as a provider key"
            );
            None
        }
    }
}

/// A provider's own words, bounded. Truncated rather than refused: a detail that will not fit must
/// not be the reason a failure goes unrecorded.
fn detail(message: &str) -> FailureDetail {
    let mut bounded = message.to_string();
    while FailureDetail::parse(bounded.clone()).is_err() && !bounded.is_empty() {
        bounded.truncate(bounded.len().saturating_sub(bounded.len() / 4 + 1));
    }
    FailureDetail::parse(bounded).unwrap_or_else(|_| {
        FailureDetail::parse("the provider reported an error that could not be recorded")
            .expect("a fixed short string is within the failure-detail bound")
    })
}

#[cfg(test)]
#[path = "egress_tests.rs"]
mod tests;
