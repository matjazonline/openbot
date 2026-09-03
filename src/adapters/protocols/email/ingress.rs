//! The email ingress boundary: one arriving mail, validated, into one canonical inbound message.
//!
//! Everything email-only stops in this file. Address syntax (`support+billing.quiet@acme.…`),
//! RFC 5322 headers, MIME bodies and the SPF/DKIM/DMARC verdicts a verifier produced are read
//! here and never again; what leaves is an [`InboundDraft`] plus an [`InboundRouting`], both of
//! which a Slack event could equally produce.
//!
//! Two rules shape the seam:
//!
//! * **Authentication is the boundary's, never the headers'.** [`EmailIngressTrust`] is supplied by
//!   the caller that actually verified the message -- the SMTP session or the webhook signature
//!   check. A route that authenticated a signed-in person instead says so, and gets an envelope
//!   with no [`AuthVerdict`] at all rather than a fabricated `Pass`.
//! * **Bounds are enforced before allocation.** Recipients, attachments, subject, body and reply
//!   candidates are each checked against the limit the application declares, so an oversized
//!   message is refused at the boundary rather than at the `INSERT`.

use tracing::warn;

use crate::{
    adapters::{
        protocols::email::{
            EmailChannelSelectorParser, EmailIdentity, EmailMessageKey, EmailRecipientDestination,
            attachments::store_inbound_attachments,
            parser::{EmailParser, ParsedEmail, RawInboundPayload},
        },
        storage::FileStorage,
    },
    app_error::AppError,
    entities::{
        auth::AuthVerdict,
        email_message::EmailMessageMetadata,
        transport::{
            ExternalMessageKey, ExternalThreadKey, QualifiedIdentity, TransportValueError,
        },
        value_objects::{EmailAddress, MessageId},
    },
    infra::config::AppConfig,
    transport::{
        AddressedIdentity, AddressedRecipient, AddressedTarget, BoundedVec, BoundsError,
        CanonicalContent, EmailIngressFacts, InboundDraft, InboundRouting, IngressDirectives,
        IngressPolicyFacts, MAX_REPLY_CANDIDATES, MessageDisposition, ProtocolExtension,
        RecipientRole, SystemAddress,
    },
    use_cases::thread::{InboundMessage, IngressOrigin, ReplyDelivery, UnusableHint},
};

/// What a verifying mail boundary established about a message.
///
/// Constructed only by a caller that actually ran the checks: the SMTP session, or the webhook
/// handler reading the provider's own verdict fields off an authenticated request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerifiedEmailAuth {
    pub spf: AuthVerdict,
    pub dkim: AuthVerdict,
    pub dmarc: AuthVerdict,
    /// `None` when no scanner ran, which is not the same as a score of zero.
    pub spam_score: Option<f64>,
}

/// How the message that is being ingested was authenticated.
///
/// The two arms are not interchangeable and there is deliberately no default: a caller that has no
/// verdicts to state cannot accidentally state passing ones.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EmailIngressTrust {
    /// Arrived over SMTP or a mail webhook, having crossed a verifier.
    Verified(VerifiedEmailAuth),
    /// Composed through an authenticated application route -- the mailbox, a simulation run. The
    /// signed-in principal is the authentication; there is no transport verdict to invent.
    Application,
}

impl EmailIngressTrust {
    fn policy(self) -> IngressPolicyFacts {
        match self {
            Self::Verified(auth) => IngressPolicyFacts::Email(EmailIngressFacts {
                spf: auth.spf,
                dkim: auth.dkim,
                dmarc: auth.dmarc,
                spam_score: auth.spam_score,
            }),
            Self::Application => IngressPolicyFacts::TrustedApplication,
        }
    }
}

/// Why a mail could not become a canonical inbound message.
///
/// Separate from a routing *rejection*: these are messages that never became well-formed enough to
/// route, and the SMTP boundary answers them with a permanent 5xx rather than accepting and losing
/// them.
#[derive(Debug, thiserror::Error)]
pub enum EmailIngressError {
    #[error("unusable {field} address: {source}")]
    Address {
        field: &'static str,
        #[source]
        source: super::EmailIdentityError,
    },
    #[error("unusable provider key: {0}")]
    ProviderKey(#[from] TransportValueError),
    #[error(transparent)]
    Bounds(#[from] BoundsError),
}

/// Malformed input, every arm of it. A permanent rejection rather than something to retry.
impl From<EmailIngressError> for AppError {
    fn from(error: EmailIngressError) -> Self {
        AppError::BadRequest(error.to_string())
    }
}

/// One arriving mail, ready for the canonical ingress use case.
#[derive(Debug, Clone)]
pub struct AcceptedEmail {
    pub draft: InboundDraft,
    pub routing: InboundRouting,
    /// Conversation hints this adapter parsed but could not use.
    ///
    /// Reported rather than logged here: parsing is this adapter's job, and deciding whether a
    /// discarded `Thread-Index` is worth a counter belongs to the layer that owns the metrics.
    pub unusable_hints: Vec<UnusableHint>,
}

impl AcceptedEmail {
    /// Hand this mail to the canonical ingress.
    ///
    /// `origin` is the caller's statement about how the message was authenticated, and only a
    /// caller that performed that authentication can make it -- which is why it is a parameter
    /// here rather than something this adapter infers from the mail it just parsed.
    pub fn into_inbound(
        self,
        origin: IngressOrigin,
        reply_delivery: ReplyDelivery,
    ) -> InboundMessage {
        InboundMessage::arriving(self.draft, self.routing, origin)
            .with_reply_delivery(reply_delivery)
            .with_unusable_hints(self.unusable_hints)
    }
}

/// Parses one mail into the canonical inbound vocabulary.
///
/// Holds the address parser rather than rebuilding it per message: the app domain is deployment
/// configuration read once, and constructing a parser per recipient is how the old path ended up
/// lowercasing the same domain thousands of times a minute.
#[derive(Debug, Clone)]
pub struct EmailIngressAdapter {
    app_domain: String,
    selector: EmailChannelSelectorParser,
}

impl EmailIngressAdapter {
    pub fn new(app_domain: impl AsRef<str>) -> Self {
        Self {
            app_domain: app_domain.as_ref().trim().to_ascii_lowercase(),
            selector: EmailChannelSelectorParser::new(app_domain),
        }
    }

    pub fn for_config(config: &AppConfig) -> Self {
        Self::new(&config.app_domain_name)
    }

    /// Store any attachments, then parse.
    ///
    /// The storing happens here rather than inside [`EmailParser::parse`] because this is the last
    /// point that holds the bytes *and* may await: everything past it carries metadata only. With
    /// no storage configured this is exactly [`EmailIngressAdapter::accept`].
    pub async fn store_and_accept(
        &self,
        mut payload: RawInboundPayload,
        config: &AppConfig,
        storage: Option<&dyn FileStorage>,
        trust: EmailIngressTrust,
    ) -> Result<AcceptedEmail, EmailIngressError> {
        if let (Some(storage), Some(gcs)) = (storage, config.gcs.as_ref())
            && gcs.attachments_bucket.is_some()
            && !payload.attachments_data.is_empty()
        {
            store_inbound_attachments(
                storage,
                &gcs.attachments_folder,
                &mut payload.attachments_data,
            )
            .await;
        }
        self.accept(payload, trust)
    }

    /// Validate one mail and state what it says and where it was addressed.
    pub fn accept(
        &self,
        payload: RawInboundPayload,
        trust: EmailIngressTrust,
    ) -> Result<AcceptedEmail, EmailIngressError> {
        let parsed = EmailParser::parse(payload, &self.app_domain);
        let routing = self.route(&parsed)?;
        let draft = self.draft(&parsed, &routing, trust)?;
        Ok(AcceptedEmail {
            draft,
            routing,
            unusable_hints: parsed
                .thread_index_rejection
                .map(|error| UnusableHint::ThreadIndex(error, parsed.thread_index_raw_bytes))
                .into_iter()
                .collect(),
        })
    }

    /// Classify every `To:` then `Cc:` address into what it addresses.
    ///
    /// The whole of the platform's address grammar is applied here -- company/channel host,
    /// `+`-separated pipelines, `.quiet`/`+noagent` suffixes, reserved `_` local parts -- and none
    /// of it survives the return value.
    fn route(&self, parsed: &ParsedEmail) -> Result<InboundRouting, EmailIngressError> {
        let addresses = parsed
            .recipients_to
            .iter()
            .map(|to| (to, RecipientRole::To))
            .chain(
                parsed
                    .recipients_cc
                    .iter()
                    .map(|cc| (cc, RecipientRole::Cc)),
            );

        let mut recipients = Vec::new();
        for (address, role) in addresses {
            let handle = self.identity("recipient", address)?;
            recipients.push(AddressedRecipient {
                role,
                target: self.classify(address),
                disposition: self.disposition_of(address),
                handle,
            });
        }
        Ok(InboundRouting::parse(recipients)?)
    }

    /// What one address names. A reserved local part is matched before any pipeline or
    /// context-suffix handling, or a future `_msg` would be eaten by suffix stripping.
    fn classify(&self, address: &str) -> AddressedTarget {
        if let Some((company, local_part)) = self.selector.parse_platform_address(address)
            && let Some(system) = SystemAddress::parse(&local_part)
        {
            return AddressedTarget::System {
                company,
                address: system,
            };
        }
        match self.selector.classify(EmailAddress::from(address.trim())) {
            EmailRecipientDestination::Channel(selection) => {
                AddressedTarget::Channels(selection.into_selectors())
            }
            // A platform-domain address that names no channel is nobody: it is not an outsider to
            // copy onto a thread, and it is not a channel to deliver to.
            EmailRecipientDestination::InvalidPlatformAddress => AddressedTarget::Channels(vec![]),
            EmailRecipientDestination::External(_) => AddressedTarget::Outsider,
        }
    }

    fn disposition_of(&self, address: &str) -> MessageDisposition {
        match self.selector.parse(address) {
            Some(selection) if selection.delivery().is_context_only() => {
                MessageDisposition::FileOnly
            }
            _ => MessageDisposition::Answer,
        }
    }

    fn identity(
        &self,
        field: &'static str,
        address: &str,
    ) -> Result<QualifiedIdentity, EmailIngressError> {
        EmailIdentity::parse(EmailAddress::from(address.trim()))
            .map(EmailIdentity::qualify_default)
            .map_err(|source| EmailIngressError::Address { field, source })
    }

    /// The canonical message itself: who said it, what they said, and every key mail offers for
    /// finding the conversation it belongs to.
    fn draft(
        &self,
        parsed: &ParsedEmail,
        routing: &InboundRouting,
        trust: EmailIngressTrust,
    ) -> Result<InboundDraft, EmailIngressError> {
        let metadata = email_metadata(parsed);
        let author = self.identity("sender", &parsed.sender)?;

        let mut addressed = Vec::with_capacity(routing.recipients.len());
        for recipient in &routing.recipients {
            addressed.push(AddressedIdentity::new(
                recipient.role,
                recipient.handle.clone(),
            ));
        }

        let disposition = if parsed.is_context_only || routing.any_files_only() {
            MessageDisposition::FileOnly
        } else {
            MessageDisposition::Answer
        };

        Ok(InboundDraft {
            // Mail has no delivery-event identity of its own: the SMTP transaction is the event,
            // and it is gone by the time this returns. A transport with a durable inbox names its
            // event row here instead.
            event_key: None,
            message_key: message_key(&metadata.rfc_message_id)?,
            thread_key: thread_key(metadata.conversation_root_key())?,
            reply_message_keys: reply_message_keys(metadata.reference_candidates())?,
            reply_thread_keys: BoundedVec::parse(
                "reply thread candidates",
                vec![thread_key(metadata.conversation_root_key())?],
            )?,
            author,
            addressed: BoundedVec::parse("addressed identities", addressed)?,
            content: CanonicalContent::parse(&parsed.subject, &parsed.clean_text_body)?,
            attachments: BoundedVec::parse("attachments", parsed.attachments.clone())?,
            directives: IngressDirectives {
                hop_count: parsed.hop_count,
                trace_channels: parsed.trace_channels.clone(),
                disposition,
                source_channel_id: parsed.channel_id_header,
                is_auto_reply: parsed.is_auto_reply,
                is_forwarded: parsed.is_forwarded,
            },
            policy: trust.policy(),
            correlation_id: parsed.correlation_id,
            extension: ProtocolExtension::email(metadata),
        })
    }
}

/// The headers that survive alongside the canonical message.
fn email_metadata(parsed: &ParsedEmail) -> EmailMessageMetadata {
    EmailMessageMetadata::new(MessageId::from(parsed.message_id.clone()))
        .in_reply_to(parsed.in_reply_to.clone().map(MessageId::from))
        .references(
            parsed
                .references
                .iter()
                .cloned()
                .map(MessageId::from)
                .collect(),
        )
        .thread_index(parsed.thread_index.clone())
        .raw_bodies(parsed.raw_text_body.clone(), parsed.raw_html_body.clone())
}

fn message_key(id: &MessageId) -> Result<ExternalMessageKey, EmailIngressError> {
    Ok(EmailMessageKey::parse(id.clone())?.into_external())
}

fn thread_key(id: &MessageId) -> Result<ExternalThreadKey, EmailIngressError> {
    Ok(ExternalThreadKey::parse(id.as_str().trim())?)
}

/// Convert and bound one candidate list, dropping the entries a sender wrote unusably.
///
/// A malformed `References` entry is the sender's problem and must not cost the message: the
/// remaining candidates still locate the conversation. The *message's own* key is converted
/// separately, where a failure is fatal.
fn reply_message_keys(
    ids: Vec<MessageId>,
) -> Result<BoundedVec<ExternalMessageKey, MAX_REPLY_CANDIDATES>, EmailIngressError> {
    let mut keys = Vec::with_capacity(ids.len().min(MAX_REPLY_CANDIDATES));
    for id in ids.iter().take(MAX_REPLY_CANDIDATES) {
        match message_key(id) {
            Ok(key) => keys.push(key),
            Err(error) => warn!(%error, "Ignoring an unusable reference on an inbound message"),
        }
    }
    Ok(BoundedVec::parse("reply message candidates", keys)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{MAX_ADDRESSED_TARGETS, MAX_BODY_BYTES, MAX_SUBJECT_BYTES};

    fn adapter() -> EmailIngressAdapter {
        EmailIngressAdapter::new("mailagents.com")
    }

    fn verified() -> EmailIngressTrust {
        EmailIngressTrust::Verified(VerifiedEmailAuth {
            spf: AuthVerdict::Pass,
            dkim: AuthVerdict::Pass,
            dmarc: AuthVerdict::Fail,
            spam_score: Some(2.5),
        })
    }

    fn mail(to: &str) -> RawInboundPayload {
        RawInboundPayload {
            to: to.to_string(),
            from: "Someone <Person@Example.COM>".to_string(),
            subject: Some("Quick question".to_string()),
            text: Some("Can you take a look?".to_string()),
            headers: Some("Message-ID: <m1@example.com>".to_string()),
            ..RawInboundPayload::default()
        }
    }

    /// The whole address grammar is applied here and none of it survives: what the application
    /// receives is selectors, a reserved name, or "this is a person".
    #[test]
    fn every_recipient_is_classified_before_the_application_sees_it() {
        let accepted = adapter()
            .accept(
                RawInboundPayload {
                    cc: Some(
                        "_help@acme.mailagents.com, client@external.com, \
                         billing.quiet@acme.mailagents.com"
                            .to_string(),
                    ),
                    ..mail("support+sales@acme.mailagents.com")
                },
                verified(),
            )
            .unwrap();

        let pipelines: Vec<_> = accepted
            .routing
            .channel_pipelines()
            .map(|(recipient, selectors)| {
                (
                    recipient.role,
                    selectors
                        .iter()
                        .map(|selector| selector.channel().to_string())
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            pipelines,
            vec![
                (
                    RecipientRole::To,
                    vec!["support".to_string(), "sales".to_string()]
                ),
                (RecipientRole::Cc, vec!["billing".to_string()]),
            ]
        );
        assert_eq!(
            accepted
                .routing
                .system_addresses()
                .map(|(company, address)| (company.to_string(), address))
                .collect::<Vec<_>>(),
            vec![("acme".to_string(), SystemAddress::Help)]
        );
        assert_eq!(
            accepted
                .routing
                .outsiders()
                .map(|handle| handle.subject().as_str().to_string())
                .collect::<Vec<_>>(),
            vec!["client@external.com".to_string()]
        );
        // One address asked to be filed; the envelope folds that in for the whole message.
        assert!(accepted.routing.any_files_only());
        assert!(!accepted.draft.directives.disposition.answers());
    }

    /// The verdicts belong to the boundary that ran the checks. A route that authenticated a
    /// person instead gets no verdict at all rather than a `Pass` nobody earned.
    #[test]
    fn authentication_comes_from_the_caller_and_never_from_the_message() {
        let verified_facts = adapter()
            .accept(mail("support@acme.mailagents.com"), verified())
            .unwrap()
            .draft
            .policy;
        let facts = verified_facts.email().expect("mail carries verdicts");
        assert_eq!(facts.dmarc, AuthVerdict::Fail);
        assert_eq!(facts.spam_score, Some(2.5));

        // The same message, composed through an authenticated route: no verdict is invented, so
        // there is nothing for a DMARC check to read and nothing to fake.
        let composed = adapter()
            .accept(
                mail("support@acme.mailagents.com"),
                EmailIngressTrust::Application,
            )
            .unwrap()
            .draft
            .policy;
        assert!(composed.email().is_none());
        assert_eq!(composed, IngressPolicyFacts::TrustedApplication);
    }

    /// Provider keys leave this adapter unqualified; binding them is the only way to use them.
    #[test]
    fn the_message_and_its_conversation_are_keyed_by_their_rfc_ids() {
        let accepted = adapter()
            .accept(
                RawInboundPayload {
                    headers: Some(
                        "Message-ID: <reply@example.com>\nIn-Reply-To: <parent@example.com>\n\
                         References: <root@example.com> <parent@example.com>"
                            .to_string(),
                    ),
                    ..mail("support@acme.mailagents.com")
                },
                verified(),
            )
            .unwrap();

        assert_eq!(accepted.draft.message_key.as_str(), "<reply@example.com>");
        // RFC 5322 puts the conversation root first in `References`, so a reply and the root it
        // answers derive the same conversation key.
        assert_eq!(accepted.draft.thread_key.as_str(), "<root@example.com>");
        assert_eq!(
            accepted
                .draft
                .reply_message_keys
                .iter()
                .map(|key| key.as_str())
                .collect::<Vec<_>>(),
            vec!["<parent@example.com>", "<root@example.com>"],
            "the nearest ancestor is offered first"
        );

        let binding_id = crate::entities::transport::ChannelBindingId::random();
        let envelope = accepted.draft.bind(binding_id);
        assert!(
            envelope
                .reply_candidates
                .messages
                .iter()
                .all(|candidate| candidate.binding_id == binding_id),
            "binding is the only way a provider key becomes usable"
        );
    }

    /// Bounds are enforced where the value is built, not advertised and then exceeded.
    #[test]
    fn oversized_input_is_refused_at_the_boundary() {
        let long_subject = adapter().accept(
            RawInboundPayload {
                subject: Some("s".repeat(MAX_SUBJECT_BYTES + 1)),
                ..mail("support@acme.mailagents.com")
            },
            verified(),
        );
        assert!(matches!(long_subject, Err(EmailIngressError::Bounds(_))));

        let long_body = adapter().accept(
            RawInboundPayload {
                text: Some("b".repeat(MAX_BODY_BYTES + 1)),
                ..mail("support@acme.mailagents.com")
            },
            verified(),
        );
        assert!(matches!(long_body, Err(EmailIngressError::Bounds(_))));

        let recipients = (0..=MAX_ADDRESSED_TARGETS)
            .map(|index| format!("person{index}@example.com"))
            .collect::<Vec<_>>()
            .join(", ");
        let too_many = adapter().accept(
            RawInboundPayload {
                cc: Some(recipients),
                ..mail("support@acme.mailagents.com")
            },
            verified(),
        );
        assert!(matches!(too_many, Err(EmailIngressError::Bounds(_))));
    }

    /// A malformed mailbox is the sender's fault and is permanent: it will not parse on a retry.
    #[test]
    fn an_unusable_address_is_a_permanent_rejection_naming_which_field_it_was() {
        let error = adapter()
            .accept(
                RawInboundPayload {
                    from: "not an address".to_string(),
                    ..mail("support@acme.mailagents.com")
                },
                verified(),
            )
            .expect_err("a message from nobody cannot be attributed");
        assert!(matches!(
            error,
            EmailIngressError::Address {
                field: "sender",
                ..
            }
        ));
        assert!(matches!(AppError::from(error), AppError::BadRequest(_)));
    }

    /// Addresses are normalized once, at the boundary, so every later decision names one actor.
    #[test]
    fn identities_are_normalized_where_the_address_syntax_lives() {
        let accepted = adapter()
            .accept(mail("SUPPORT@Acme.MailAgents.com"), verified())
            .unwrap();
        assert_eq!(
            accepted.draft.author.subject().as_str(),
            "person@example.com"
        );
        assert_eq!(
            accepted.draft.addressed[0].identity.subject().as_str(),
            "support@acme.mailagents.com"
        );
    }
}
