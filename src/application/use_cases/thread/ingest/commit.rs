//! Phase 5: everything one accepted message must make durable, assembled as one request.
//!
//! Nothing here awaits. The plan is built from values the earlier phases already loaded, handed to
//! [`InboundMessageCommitter`](crate::transport::InboundMessageCommitter) in a single call, and
//! read back afterwards. That is the whole reason the phases before it are read-only: a rejection
//! that happens at any point up to here leaves no row behind to clean up.

use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::message::CanonicalMessageId,
    transport::{
        BoundedVec, CanonicalContent, InboundAdmissionIntent, InboundCommitRequest, InboundDraft,
        InboundEnvelope, InboundOrigin, MAX_THREAD_ASSOCIATIONS, MessageDisposition, NewDelivery,
        ThreadAssociation, ThreadPrincipalIntent, ThreadTarget,
    },
    use_cases::thread::ingest::{ReplyDelivery, routing::ResolvedAddresses},
};

use super::{policy::IngressOrigin, routing::ChannelCandidate};

/// One channel's share of the commit.
pub(crate) struct PreparedChannel {
    pub candidate: ChannelCandidate,
    /// The thread this channel's copy lands in, existing or to be created.
    pub target: ThreadTarget,
    /// Whether the body names this channel; a fact for binding eligibility, not a decision.
    pub mentioned: bool,
    /// The message this one directly answers in this channel's thread, when resolvable.
    pub reply_to: Option<CanonicalMessageId>,
    /// The handles this channel's thread gains from the message, under this channel's own policy.
    pub principals: BoundedVec<ThreadPrincipalIntent, { crate::transport::MAX_THREAD_PRINCIPALS }>,
}

/// What phases 3 and 4 concluded for every channel the message reached.
pub(crate) struct PreparedChannels {
    pub channels: Vec<PreparedChannel>,
    /// The body with quoted history removed.
    ///
    /// Stripped **once**, against the thread the primary channel continues. One canonical message
    /// has one body: the pre-canonical path stored a separately-stripped copy per channel, which
    /// is why its content hash had to exclude the body to avoid calling ordinary fan-out a
    /// redelivery collision.
    pub body_text: String,
}

/// The complete set of rows one accepted message turns into.
pub(crate) struct CommitPlan {
    company_id: Uuid,
    envelope: InboundEnvelope,
    associations: BoundedVec<ThreadAssociation, MAX_THREAD_ASSOCIATIONS>,
    intents: BoundedVec<InboundAdmissionIntent, MAX_THREAD_ASSOCIATIONS>,
    origin: InboundOrigin,
    deliveries: Vec<NewDelivery>,
    prepared: PreparedChannels,
    disposition: MessageDisposition,
    reply_delivery: ReplyDelivery,
}

impl CommitPlan {
    pub(crate) fn build(
        draft: &InboundDraft,
        resolved: &ResolvedAddresses,
        prepared: PreparedChannels,
        reply_delivery: ReplyDelivery,
        origin: IngressOrigin,
    ) -> AppResult<Self> {
        let primary = prepared
            .channels
            .first()
            .expect("resolution refuses a message with no authorized channel");

        // The interface the primary channel received it on is the message's source. Every other
        // channel gets its own binding-qualified mapping through its association.
        let mut envelope = draft.clone().bind(primary.candidate.binding_id);
        envelope.content = CanonicalContent::parse(draft.content.subject(), &prepared.body_text)?;
        // Only the sender silences a message (p08 D1.3): channel settings no longer decide
        // whether it is answered, the bindings do, after the commit.
        let disposition = draft.directives.disposition;

        let associations: Vec<_> = prepared
            .channels
            .iter()
            .map(|channel| ThreadAssociation {
                channel_id: channel.candidate.channel.id,
                binding_id: channel.candidate.binding_id,
                target: channel.target.clone(),
                role: channel.candidate.role,
                step: channel.candidate.step,
                principals: channel.principals.clone(),
            })
            .collect();
        let intents: Vec<_> = prepared
            .channels
            .iter()
            .map(|channel| InboundAdmissionIntent {
                channel_id: channel.candidate.channel.id,
                role: channel.candidate.role,
                mentioned: channel.mentioned,
                reply_to: channel.reply_to,
                settings_revision: channel.candidate.channel.settings_revision,
            })
            .collect();

        Ok(Self {
            company_id: resolved.company.id,
            envelope,
            associations: BoundedVec::parse("thread associations", associations)?,
            intents: BoundedVec::parse("admission intents", intents)?,
            origin: inbound_origin(origin),
            // Inbound fan-out onto a channel's *other* interfaces has a durable queue now, but
            // nothing to fan out to: email is the only transport a channel speaks, and delivering
            // a message back to the interface it arrived on is an echo. So this is correctly empty
            // rather than unimplemented, and the commit writes whatever it is given.
            deliveries: Vec::new(),
            prepared,
            disposition,
            reply_delivery,
        })
    }

    pub(crate) fn request(&self) -> InboundCommitRequest {
        InboundCommitRequest {
            company_id: self.company_id,
            envelope: self.envelope.clone(),
            // Mail has no durable inbound event to fence: the SMTP transaction is the claim, and it
            // is held open until this commit returns.
            claimed_event: None,
            associations: self.associations.clone(),
            intents: self.intents.clone(),
            admissible: self.disposition.answers(),
            origin: self.origin,
            deliveries: self.deliveries.clone(),
            reply_delivery: self.reply_delivery,
        }
    }

    pub(crate) fn replace_attachments(
        &mut self,
        attachments: crate::transport::BoundedVec<
            crate::entities::message::AttachmentMetadata,
            { crate::transport::MAX_ATTACHMENTS },
        >,
    ) {
        self.envelope.attachments = attachments;
    }

    pub(crate) const fn envelope(&self) -> &InboundEnvelope {
        &self.envelope
    }

    pub(crate) const fn company_id(&self) -> Uuid {
        self.company_id
    }

    pub(crate) const fn disposition(&self) -> MessageDisposition {
        self.disposition
    }

    pub(crate) const fn reply_delivery(&self) -> ReplyDelivery {
        self.reply_delivery
    }

    pub(crate) fn channels(&self) -> usize {
        self.prepared.channels.len()
    }

    pub(crate) fn into_prepared(self) -> PreparedChannels {
        self.prepared
    }
}

const fn inbound_origin(origin: IngressOrigin) -> InboundOrigin {
    match origin {
        IngressOrigin::ExternalTransport => InboundOrigin::External,
        IngressOrigin::TrustedApplication => InboundOrigin::Application,
        IngressOrigin::InternalChannel { .. } => InboundOrigin::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{
            channel::{Channel, ChannelAccessMode, ParticipantAccess},
            company::Company,
            creation::CreationProvenance,
            transport::{
                ChannelBindingId, ExternalMessageKey, ExternalThreadKey, IdentityNamespace,
                IdentitySubject, QualifiedIdentity, RecipientRole, TransportKind,
            },
        },
        transport::{IngressDirectives, IngressPolicyFacts, PipelineStep, ProtocolExtension},
    };
    use chrono::Utc;

    fn company() -> Company {
        Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }
    }

    fn channel(company_id: Uuid, slug: &str) -> Channel {
        Channel {
            enabled: true,
            add_3rd_party: false,
            id: Uuid::new_v4(),
            company_id,
            name: slug.to_string(),
            description: None,
            slug: slug.into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            created_by: CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }
    }

    fn identity(address: &str) -> QualifiedIdentity {
        QualifiedIdentity::new(
            TransportKind::Email,
            IdentityNamespace::parse("email").unwrap(),
            IdentitySubject::parse(address).unwrap(),
        )
    }

    fn draft(disposition: MessageDisposition) -> InboundDraft {
        InboundDraft {
            direct_parent_message_key: None,
            event_key: None,
            message_key: ExternalMessageKey::parse("<hold@example.com>").unwrap(),
            thread_key: ExternalThreadKey::parse("<hold@example.com>").unwrap(),
            reply_message_keys: BoundedVec::empty(),
            reply_thread_keys: BoundedVec::empty(),
            author: identity("ana@client.com"),
            addressed: BoundedVec::empty(),
            content: CanonicalContent::parse("Invoice 4471", "Any news?").unwrap(),
            attachments: BoundedVec::empty(),
            directives: IngressDirectives {
                disposition,
                ..IngressDirectives::default()
            },
            policy: IngressPolicyFacts::TrustedApplication,
            correlation_id: crate::entities::correlation::CorrelationId::new(),
            extension: ProtocolExtension::none(),
        }
    }

    fn prepared_channel(company: &Company, slug: &str, role: RecipientRole) -> PreparedChannel {
        let channel = channel(company.id, slug);
        PreparedChannel {
            candidate: ChannelCandidate {
                company: company.clone(),
                binding_id: ChannelBindingId::new(channel.id),
                matched_slug: channel.slug.clone(),
                handle: identity(&format!("{slug}@acme.example")),
                role,
                step: PipelineStep::only(),
                access: ParticipantAccess {
                    authorized: true,
                    trusted: false,
                },
                channel,
            },
            target: ThreadTarget::Existing(Uuid::new_v4()),
            mentioned: false,
            reply_to: None,
            principals: BoundedVec::empty(),
        }
    }

    fn plan_with_disposition(
        company: &Company,
        channels: Vec<PreparedChannel>,
        disposition: MessageDisposition,
    ) -> InboundCommitRequest {
        let resolved = ResolvedAddresses {
            company: company.clone(),
            candidates: Vec::new(),
        };
        CommitPlan::build(
            &draft(disposition),
            &resolved,
            PreparedChannels {
                channels,
                body_text: "Any news?".to_string(),
            },
            ReplyDelivery::Send,
            IngressOrigin::ExternalTransport,
        )
        .expect("a hand-made plan is within every bound")
        .request()
    }

    /// Every association gets an intent carrying its own facts, and the settings revision its
    /// channel was routed under; nothing about a channel's settings decides here whether anything
    /// runs (p08 D1.3).
    #[test]
    fn every_association_gets_an_intent_with_its_own_facts() {
        let company = company();
        let mut support = prepared_channel(&company, "support", RecipientRole::To);
        support.mentioned = true;
        let parent = CanonicalMessageId::new(Uuid::new_v4());
        let mut billing = prepared_channel(&company, "billing", RecipientRole::Cc);
        billing.reply_to = Some(parent);
        billing.candidate.channel.settings_revision = 7;
        let ids = [support.candidate.channel.id, billing.candidate.channel.id];
        let request =
            plan_with_disposition(&company, vec![support, billing], MessageDisposition::Answer);
        assert!(request.admissible);
        assert_eq!(request.origin, InboundOrigin::External);
        let intents: Vec<_> = request.intents.iter().copied().collect();
        assert_eq!(
            intents,
            vec![
                InboundAdmissionIntent {
                    channel_id: ids[0],
                    role: RecipientRole::To,
                    mentioned: true,
                    reply_to: None,
                    settings_revision: 0,
                },
                InboundAdmissionIntent {
                    channel_id: ids[1],
                    role: RecipientRole::Cc,
                    mentioned: false,
                    reply_to: Some(parent),
                    settings_revision: 7,
                },
            ]
        );
    }

    /// A quiet message still gets its intents, for reply correlation, but admits no binding, and
    /// keeps the disposition the sender stated.
    #[test]
    fn a_file_only_message_has_intents_that_admit_nothing() {
        let company = company();
        let request = plan_with_disposition(
            &company,
            vec![prepared_channel(&company, "support", RecipientRole::To)],
            MessageDisposition::FileOnly,
        );
        assert!(!request.admissible);
        assert_eq!(request.intents.len(), 1);
        assert_eq!(
            request.envelope.directives.disposition,
            MessageDisposition::FileOnly
        );
    }
}
