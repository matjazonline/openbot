//! The decisions ingest makes without touching the database.
//!
//! Everything here takes already-loaded values and returns a verdict: no `self`, no `async`, no
//! persistence. That is what lets the pipeline in [`super`] read as a list of named phases, and
//! what lets these rules be unit-tested against a table of cases with no mocks at all.

use crate::{
    entities::{
        auth::AuthVerdict,
        channel::ParticipantAccess,
        transport::ExternalMessageKey,
        value_objects::{ChannelSlug, EmailAddress},
    },
    transport::{InboundDraft, IngressPolicyFacts, MAX_INGRESS_HOPS, MessageDisposition},
    use_cases::thread::{BounceInfo, MAX_THREAD_MESSAGES_PER_HOUR},
};

/// Why an inbound message produced no thread.
///
/// An enum rather than a reason string because two very different consumers read it: the SMTP
/// session, which has to answer 250 or 550 *synchronously*, and the connection metric, which has
/// to count a refused message as the thing it was refused for. Both used to match on prose --
/// `src/adapters/smtp/server.rs` still carried the note asking for this type -- and both quietly
/// mis-categorised anything the wording drifted away from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestRejection {
    /// The mail boundary's DMARC verdict was not a pass.
    AuthenticationFailed,
    /// A machine-generated message from outside: answering it starts a loop.
    AutoReply,
    /// The message has been relayed between channels as often as this platform allows.
    HopLimitReached,
    /// A channel already in this message's trace was addressed again, uncorrelated.
    LoopCycle,
    /// An internal message whose stated source channel is not the one that sent it.
    InternalSourceMismatch,
    /// Internal delivery that crossed a tenant boundary.
    CrossCompanyInternal,
    /// One address named channels in two companies.
    CrossCompanyPipeline,
    /// The spam score exceeded the deployment's threshold for an untrusted sender.
    SpamScore,
    /// The addresses named no channel this platform serves.
    UnknownRecipient,
    /// Every channel the message named refused this sender.
    Unauthorized,
    /// One or more named channels do not exist or are switched off; the sender is told which.
    Undeliverable(Box<UndeliverableReason>),
    /// The sender has no standing on the thread they wrote into.
    ThreadInjection(Box<BounceInfo>),
    /// The thread has taken more turns this hour than the loop guard allows.
    ThreadTurnLimit,
    /// The message named only a reserved address, and that address was answered.
    SystemAddressAnswered,
}

/// A bounce, and which of the two undeliverable cases produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndeliverableReason {
    pub kind: UndeliverableKind,
    pub bounce: BounceInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndeliverableKind {
    UnknownChannel,
    DisabledChannel,
}

impl IngestRejection {
    /// The sentence a synchronous transport puts in its refusal, and the mailbox shows.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AuthenticationFailed => "DMARC authentication did not pass",
            Self::AutoReply => "External auto-reply loop detected",
            Self::HopLimitReached => "Max inter-channel hop count reached",
            Self::LoopCycle => "Inter-channel loop cycle detected",
            Self::InternalSourceMismatch => "Internal source channel identity mismatch",
            Self::CrossCompanyInternal => "Cross-company internal channel delivery is not allowed",
            Self::CrossCompanyPipeline => "A channel pipeline cannot span multiple companies",
            Self::SpamScore => "Spam score threshold exceeded",
            Self::UnknownRecipient => "Company or Channel not found",
            Self::Unauthorized => "Sender unauthorized for channel",
            Self::Undeliverable(reason) => match reason.kind {
                UndeliverableKind::UnknownChannel => "Channel address not found or misspelled",
                UndeliverableKind::DisabledChannel => "Channel is disabled",
            },
            Self::ThreadInjection(_) => {
                "Sender is not an authorized participant or delegation target for this thread"
            }
            Self::ThreadTurnLimit => "Thread turn limit exceeded",
            Self::SystemAddressAnswered => "System address answered",
        }
    }

    /// The bounce this rejection owes the sender, if any.
    pub fn bounce(&self) -> Option<&BounceInfo> {
        match self {
            Self::Undeliverable(reason) => Some(&reason.bounce),
            Self::ThreadInjection(bounce) => Some(bounce),
            _ => None,
        }
    }
}

impl std::fmt::Display for IngestRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// How this message reached the platform, as the *caller* -- not the message -- states it.
///
/// A header can claim anything; this cannot, because only the code path that actually performed
/// the authentication can construct the arm that says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngressOrigin {
    /// Delivered by a remote mail server or a signed provider webhook.
    ExternalTransport,
    /// Composed by a signed-in principal through an application route.
    TrustedApplication,
    /// Relayed from another channel of this platform over the in-process transport.
    InternalChannel {
        company_id: uuid::Uuid,
        channel_id: uuid::Uuid,
    },
}

impl IngressOrigin {
    pub const fn is_internal(self) -> bool {
        matches!(self, Self::InternalChannel { .. })
    }

    pub const fn internal_channel(self) -> Option<uuid::Uuid> {
        match self {
            Self::InternalChannel { channel_id, .. } => Some(channel_id),
            Self::ExternalTransport | Self::TrustedApplication => None,
        }
    }

    pub const fn internal_company(self) -> Option<uuid::Uuid> {
        match self {
            Self::InternalChannel { company_id, .. } => Some(company_id),
            Self::ExternalTransport | Self::TrustedApplication => None,
        }
    }
}

/// Phase 1: the rejections that need no I/O.
///
/// Runs before any persistence work, so a forged, looping or machine-generated message costs one
/// parse and nothing else.
pub fn guard_ingress(draft: &InboundDraft, origin: IngressOrigin) -> Result<(), IngestRejection> {
    if origin.is_internal() && draft.directives.source_channel_id != origin.internal_channel() {
        return Err(IngestRejection::InternalSourceMismatch);
    }

    // Trusted internal transport and signed-in routes are authenticated by who they are, not by
    // SPF/DKIM/DMARC -- which is why a verdict is only *asked for* when the facts carry one.
    if matches!(origin, IngressOrigin::ExternalTransport)
        && let Some(facts) = draft.policy.email()
        && !matches!(facts.dmarc, AuthVerdict::Pass)
    {
        return Err(IngestRejection::AuthenticationFailed);
    }

    if origin.is_internal() && draft.directives.hop_count >= MAX_INGRESS_HOPS {
        return Err(IngestRejection::HopLimitReached);
    }

    if !origin.is_internal() && draft.directives.is_auto_reply {
        return Err(IngestRejection::AutoReply);
    }

    Ok(())
}

/// Whether an untrusted sender's spam score is past the deployment's threshold.
pub fn exceeds_spam_threshold(
    policy: &IngressPolicyFacts,
    access: ParticipantAccess,
    threshold: f64,
) -> bool {
    !access.trusted
        && policy
            .email()
            .and_then(|facts| facts.spam_score)
            .is_some_and(|score| score >= threshold)
}

/// Whether this thread has taken more turns in the last hour than the ping-pong guard allows.
pub const fn exceeds_turn_limit(recent_messages: usize) -> bool {
    recent_messages >= MAX_THREAD_MESSAGES_PER_HOUR
}

/// Whether the agent runs at all for this message, folding every source that can silence it.
///
/// One place, because "quiet" arrives four ways -- an address suffix, a body marker, a header, and
/// an outreach reply that only needs recording -- and a check that missed one would run an agent
/// the sender explicitly asked not to.
pub fn fold_disposition(
    stated: MessageDisposition,
    any_channel_answers: bool,
    all_matches_are_outreach_replies: bool,
) -> MessageDisposition {
    if stated.answers() && any_channel_answers && !all_matches_are_outreach_replies {
        MessageDisposition::Answer
    } else {
        MessageDisposition::FileOnly
    }
}

/// The participants a thread gains from this message.
///
/// Pure so the rule can be read at a glance: the sender joins unless their message merely closes
/// an outreach the channel was waiting on, and third parties join only when the sender is trusted
/// *and* the channel opted in. The flag can narrow who is pulled in, never widen it.
pub fn thread_participants<T: Clone + Eq>(
    existing: &[T],
    sender: &T,
    third_parties: &[T],
    add_sender: bool,
    pull_third_parties: bool,
) -> Vec<T> {
    let mut participants = existing.to_vec();
    let mut push = |candidate: &T| {
        if !participants.contains(candidate) {
            participants.push(candidate.clone());
        }
    };

    if add_sender {
        push(sender);
    }
    if pull_third_parties {
        for third_party in third_parties {
            push(third_party);
        }
    }
    participants
}

/// The slugs on the `To`/`Cc` line that cannot be delivered to, and why.
///
/// Owning both cases in one place keeps the bounce decision -- which reason wins, and what the
/// bounce body lists -- out of the middle of the address loop.
#[derive(Debug, Default)]
pub struct UndeliverableSlugs {
    invalid: Vec<ChannelSlug>,
    disabled: Vec<ChannelSlug>,
    suggestions: Vec<crate::use_cases::thread::BounceSuggestion>,
}

impl UndeliverableSlugs {
    pub fn unknown(&mut self, slug: ChannelSlug, suggestions: Vec<ChannelSlug>) {
        self.invalid.push(slug.clone());
        self.suggestions
            .push(crate::use_cases::thread::BounceSuggestion {
                invalid_slug: slug,
                suggestions,
            });
    }

    pub fn disabled(&mut self, slug: ChannelSlug) {
        self.disabled.push(slug);
    }

    /// A misspelling is reported ahead of a disabled channel: the sender can act on a typo, and
    /// the bounce body lists both sets regardless.
    pub fn kind(&self) -> Option<UndeliverableKind> {
        if !self.invalid.is_empty() {
            Some(UndeliverableKind::UnknownChannel)
        } else if !self.disabled.is_empty() {
            Some(UndeliverableKind::DisabledChannel)
        } else {
            None
        }
    }

    pub fn into_bounce(
        self,
        source_message_key: ExternalMessageKey,
        recipient_to: EmailAddress,
        company_slug: Option<crate::entities::value_objects::CompanySlug>,
        available_channels: Vec<crate::use_cases::thread::ChannelDirectoryEntry>,
        original_subject: String,
    ) -> BounceInfo {
        BounceInfo {
            source_message_key,
            recipient_to,
            company_slug,
            invalid_slugs: self.invalid,
            disabled_slugs: self.disabled,
            suggestions: self.suggestions,
            available_channels,
            original_subject,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{
            correlation::CorrelationId,
            transport::{
                ExternalMessageKey, ExternalThreadKey, IdentityNamespace, IdentitySubject,
                QualifiedIdentity, TransportKind,
            },
        },
        transport::{
            BoundedVec, CanonicalContent, EmailIngressFacts, IngressDirectives, ProtocolExtension,
        },
    };
    use uuid::Uuid;

    fn identity(subject: &str) -> QualifiedIdentity {
        QualifiedIdentity::new(
            TransportKind::Email,
            IdentityNamespace::parse("email").unwrap(),
            IdentitySubject::parse(subject).unwrap(),
        )
    }

    fn draft(policy: IngressPolicyFacts, directives: IngressDirectives) -> InboundDraft {
        InboundDraft {
            event_key: None,
            message_key: ExternalMessageKey::parse("<m@example.com>").unwrap(),
            thread_key: ExternalThreadKey::parse("<m@example.com>").unwrap(),
            reply_message_keys: BoundedVec::empty(),
            reply_thread_keys: BoundedVec::empty(),
            author: identity("sender@example.com"),
            addressed: BoundedVec::empty(),
            content: CanonicalContent::parse("subject", "body").unwrap(),
            attachments: BoundedVec::empty(),
            directives,
            policy,
            correlation_id: CorrelationId::new(),
            extension: ProtocolExtension::none(),
        }
    }

    fn email_facts(dmarc: AuthVerdict, spam_score: Option<f64>) -> IngressPolicyFacts {
        IngressPolicyFacts::Email(EmailIngressFacts {
            spf: AuthVerdict::Pass,
            dkim: AuthVerdict::Pass,
            dmarc,
            spam_score,
        })
    }

    #[test]
    fn only_a_dmarc_pass_authorizes_a_message_from_a_remote_mail_server() {
        for verdict in [
            AuthVerdict::Fail,
            AuthVerdict::SoftFail,
            AuthVerdict::Neutral,
            AuthVerdict::TempError,
            AuthVerdict::PermError,
            AuthVerdict::Unavailable,
            AuthVerdict::Unknown,
        ] {
            assert_eq!(
                guard_ingress(
                    &draft(email_facts(verdict, None), IngressDirectives::default()),
                    IngressOrigin::ExternalTransport,
                ),
                Err(IngestRejection::AuthenticationFailed),
                "{verdict:?}"
            );
        }
        assert!(
            guard_ingress(
                &draft(
                    email_facts(AuthVerdict::Pass, None),
                    IngressDirectives::default()
                ),
                IngressOrigin::ExternalTransport,
            )
            .is_ok()
        );
    }

    /// The reason the facts are an enum: a message with no transport verdict is not a message that
    /// failed one, and the guard must not invent a verdict in order to have something to check.
    #[test]
    fn a_signed_in_composer_is_not_asked_for_a_dmarc_verdict() {
        assert!(
            guard_ingress(
                &draft(
                    IngressPolicyFacts::TrustedApplication,
                    IngressDirectives::default()
                ),
                IngressOrigin::TrustedApplication,
            )
            .is_ok()
        );
    }

    #[test]
    fn an_internal_message_must_prove_which_channel_relayed_it() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let origin = IngressOrigin::InternalChannel {
            company_id,
            channel_id,
        };

        let mismatched = draft(
            IngressPolicyFacts::TrustedApplication,
            IngressDirectives {
                source_channel_id: Some(Uuid::new_v4()),
                ..IngressDirectives::default()
            },
        );
        assert_eq!(
            guard_ingress(&mismatched, origin),
            Err(IngestRejection::InternalSourceMismatch)
        );

        let matched = draft(
            IngressPolicyFacts::TrustedApplication,
            IngressDirectives {
                source_channel_id: Some(channel_id),
                ..IngressDirectives::default()
            },
        );
        assert!(guard_ingress(&matched, origin).is_ok());
    }

    #[test]
    fn the_hop_limit_applies_to_relayed_traffic_and_the_auto_reply_guard_to_outside_traffic() {
        let relayed = |hops| {
            draft(
                IngressPolicyFacts::TrustedApplication,
                IngressDirectives {
                    hop_count: hops,
                    source_channel_id: Some(Uuid::nil()),
                    ..IngressDirectives::default()
                },
            )
        };
        let origin = IngressOrigin::InternalChannel {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::nil(),
        };
        assert!(guard_ingress(&relayed(MAX_INGRESS_HOPS - 1), origin).is_ok());
        assert_eq!(
            guard_ingress(&relayed(MAX_INGRESS_HOPS), origin),
            Err(IngestRejection::HopLimitReached)
        );

        // A relayed message *is* an auto-reply by construction, and must not be dropped as one.
        let auto = draft(
            IngressPolicyFacts::TrustedApplication,
            IngressDirectives {
                is_auto_reply: true,
                source_channel_id: Some(Uuid::nil()),
                ..IngressDirectives::default()
            },
        );
        assert!(guard_ingress(&auto, origin).is_ok());
        assert_eq!(
            guard_ingress(&auto, IngressOrigin::ExternalTransport),
            Err(IngestRejection::AutoReply)
        );
    }

    #[test]
    fn a_trusted_sender_is_never_scored_and_an_absent_score_is_not_a_zero() {
        let untrusted = ParticipantAccess {
            authorized: true,
            trusted: false,
        };
        let trusted = ParticipantAccess {
            authorized: true,
            trusted: true,
        };

        assert!(exceeds_spam_threshold(
            &email_facts(AuthVerdict::Pass, Some(9.0)),
            untrusted,
            5.0
        ));
        assert!(!exceeds_spam_threshold(
            &email_facts(AuthVerdict::Pass, Some(9.0)),
            trusted,
            5.0
        ));
        assert!(!exceeds_spam_threshold(
            &email_facts(AuthVerdict::Pass, None),
            untrusted,
            5.0
        ));
        // A transport with no scanner has no score to compare, rather than a score of zero.
        assert!(!exceeds_spam_threshold(
            &IngressPolicyFacts::TrustedApplication,
            untrusted,
            0.0
        ));
    }

    #[test]
    fn any_source_of_quiet_silences_the_run() {
        assert_eq!(
            fold_disposition(MessageDisposition::Answer, true, false),
            MessageDisposition::Answer
        );
        for (stated, answers, outreach) in [
            (MessageDisposition::FileOnly, true, false),
            (MessageDisposition::Answer, false, false),
            (MessageDisposition::Answer, true, true),
        ] {
            assert_eq!(
                fold_disposition(stated, answers, outreach),
                MessageDisposition::FileOnly
            );
        }
    }

    #[test]
    fn participants_join_once_and_only_when_the_rule_allows() {
        let existing = vec![EmailAddress::from("first@example.com")];
        let sender = EmailAddress::from("first@example.com");
        let third = vec![EmailAddress::from("outsider@example.com")];

        // The adapter has already normalized a qualified identity; an existing one is not added.
        assert_eq!(
            thread_participants(&existing, &sender, &third, true, false),
            existing
        );
        assert_eq!(
            thread_participants(&existing, &sender, &third, true, true),
            vec![
                EmailAddress::from("first@example.com"),
                EmailAddress::from("outsider@example.com"),
            ]
        );
        // An untrusted sender's copied outsiders are not pulled in.
        assert_eq!(
            thread_participants(
                &[],
                &EmailAddress::from("new@example.com"),
                &third,
                true,
                false
            ),
            vec![EmailAddress::from("new@example.com")]
        );
    }
}
