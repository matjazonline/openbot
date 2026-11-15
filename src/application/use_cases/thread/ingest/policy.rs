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
    transport::{InboundDraft, IngressPolicyFacts, MAX_INGRESS_HOPS},
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
    /// A person's or agent's address, named by one of the company's own people. Only a team
    /// sender learns this: anyone else is told the address does not exist.
    Principal(crate::use_cases::thread::PrincipalRefusal),
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
                UndeliverableKind::Principal(refusal) => refusal.as_str(),
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
    Internal {
        company_id: uuid::Uuid,
        source_target: crate::entities::address::AddressTarget,
        agent_principal: Option<crate::entities::transport::PrincipalId>,
        /// The delivery part the hop is, which the commit proves before storing anything.
        proof: crate::transport::InternalRelayProof,
    },
}

impl IngressOrigin {
    pub const fn is_internal(self) -> bool {
        matches!(self, Self::Internal { .. })
    }

    pub const fn internal_target(self) -> Option<crate::entities::address::AddressTarget> {
        match self {
            Self::Internal { source_target, .. } => Some(source_target),
            Self::ExternalTransport | Self::TrustedApplication => None,
        }
    }

    pub const fn internal_company(self) -> Option<uuid::Uuid> {
        match self {
            Self::Internal { company_id, .. } => Some(company_id),
            Self::ExternalTransport | Self::TrustedApplication => None,
        }
    }

    pub fn internal_context(self) -> Option<crate::entities::participant::PrincipalAccessContext> {
        let Self::Internal {
            agent_principal, ..
        } = self
        else {
            return None;
        };
        Some(crate::entities::participant::PrincipalAccessContext {
            principal_id: agent_principal,
            membership: if agent_principal.is_some() {
                crate::entities::company_member::CompanyMembership::Member
            } else {
                crate::entities::company_member::CompanyMembership::None
            },
            access_level: crate::entities::access_level::AccessLevel::PUBLIC,
            grant_authority: crate::entities::participant::PrincipalGrantAuthority::Available,
        })
    }
}

/// Phase 1: the rejections that need no I/O.
///
/// Runs before any persistence work, so a forged, looping or machine-generated message costs one
/// parse and nothing else.
pub fn guard_ingress(draft: &InboundDraft, origin: IngressOrigin) -> Result<(), IngestRejection> {
    if origin.is_internal() && draft.directives.source_target != origin.internal_target() {
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
    principals: Vec<crate::use_cases::thread::PrincipalBounce>,
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

    /// A principal's address, which only a team sender is told about; routing records anyone
    /// else's as [`Self::unknown`].
    pub fn principal(
        &mut self,
        slug: ChannelSlug,
        refusal: crate::use_cases::thread::PrincipalRefusal,
    ) {
        self.principals
            .push(crate::use_cases::thread::PrincipalBounce { slug, refusal });
    }

    /// A misspelling is reported ahead of a principal's address and that ahead of a disabled
    /// channel: the sender can act on a typo first, and the bounce body lists every set.
    pub fn kind(&self) -> Option<UndeliverableKind> {
        if !self.invalid.is_empty() {
            Some(UndeliverableKind::UnknownChannel)
        } else if let Some(principal) = self.principals.first() {
            Some(UndeliverableKind::Principal(principal.refusal))
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
        company: &crate::entities::company::Company,
        available_channels: Vec<crate::use_cases::thread::AddressDirectoryEntry>,
        original_subject: String,
    ) -> BounceInfo {
        BounceInfo {
            source_message_key,
            recipient_to,
            company_id: company.id,
            company_slug: company.slug.clone(),
            invalid_slugs: self.invalid,
            disabled_slugs: self.disabled,
            principals: self.principals,
            suggestions: self.suggestions,
            available_channels,
            original_subject,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::thread::ProtocolExtension;
    use crate::entities::correlation::CorrelationId;
    use crate::entities::transport::ExternalMessageKey;
    use crate::entities::transport::ExternalThreadKey;
    use crate::entities::transport::IdentityNamespace;
    use crate::entities::transport::IdentitySubject;
    use crate::entities::transport::QualifiedIdentity;
    use crate::entities::transport::TransportKind;
    use crate::transport::BoundedVec;
    use crate::transport::CanonicalContent;
    use crate::transport::EmailIngressFacts;
    use crate::transport::IngressDirectives;
    use crate::transport::test_support::relay_proof;
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
            direct_parent_message_key: None,
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
        let origin = IngressOrigin::Internal {
            company_id,
            source_target: crate::entities::address::AddressTarget::Desk(channel_id),
            agent_principal: None,
            proof: relay_proof(company_id),
        };

        let mismatched = draft(
            IngressPolicyFacts::TrustedApplication,
            IngressDirectives {
                source_target: Some(crate::entities::address::AddressTarget::Desk(Uuid::new_v4())),
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
                source_target: Some(crate::entities::address::AddressTarget::Desk(channel_id)),
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
                    source_target: Some(crate::entities::address::AddressTarget::Desk(Uuid::nil())),
                    ..IngressDirectives::default()
                },
            )
        };
        let company_id = Uuid::new_v4();
        let origin = IngressOrigin::Internal {
            company_id,
            source_target: crate::entities::address::AddressTarget::Desk(Uuid::nil()),
            agent_principal: None,
            proof: relay_proof(company_id),
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
                source_target: Some(crate::entities::address::AddressTarget::Desk(Uuid::nil())),
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
