//! Phase 5: everything one accepted message must make durable, assembled as one request.
//!
//! Nothing here awaits. The plan is built from values the earlier phases already loaded, handed to
//! [`InboundMessageCommitter`](crate::transport::InboundMessageCommitter) in a single call, and
//! read back afterwards. That is the whole reason the phases before it are read-only: a rejection
//! that happens at any point up to here leaves no row behind to clean up.

use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{outreach::OutreachReplyMatch, thread_handoff::HoldDecision},
    transport::{
        BoundedVec, CanonicalContent, InboundCommitRequest, InboundDraft, InboundEnvelope,
        InboundHold, InboundOutreachTransition, InboundTaskRequest, InboundTaskTarget,
        MAX_THREAD_ASSOCIATIONS, MessageDisposition, NewDelivery, ThreadAssociation,
        ThreadPrincipalIntent, ThreadTarget,
    },
    use_cases::thread::ingest::{AGENT_DISPATCH_TASK, ReplyDelivery, routing::ResolvedAddresses},
};

use super::routing::ChannelCandidate;

/// One channel's share of the commit.
pub(crate) struct PreparedChannel {
    pub candidate: ChannelCandidate,
    /// The thread this channel's copy lands in, existing or to be created.
    pub target: ThreadTarget,
    /// Whether this channel's agent runs, or whether the message is only filed on its thread.
    pub answers: bool,
    /// The outreach this message closes, if it closes one. Recording the reply satisfies the
    /// awaiting outreach; the agent must not be re-run for it.
    pub outreach: Option<OutreachReplyMatch>,
    /// The handles this channel's thread gains from the message, under this channel's own policy.
    pub principals: BoundedVec<ThreadPrincipalIntent, { crate::transport::MAX_THREAD_PRINCIPALS }>,
    /// Whether this channel's copy waits for the team, as far as this channel can tell.
    ///
    /// Five of the six eligibility terms are channel-shaped and are decided in `prepare_channels`.
    /// The sixth -- whether the message asked for an answer at all -- is folded message-wide, so
    /// [`holds`] applies it here.
    pub hold: HoldDecision,
}

/// Whether this channel's copy is actually held, once the message-wide disposition is folded in.
///
/// A free function rather than a method so the rule is unit-testable without building a
/// [`CommitPlan`], and so the two independent gates -- an explicit `FileOnly` and a channel hold --
/// meet in exactly one place.
const fn holds(hold: HoldDecision, disposition: MessageDisposition) -> bool {
    matches!(hold, HoldDecision::Hold) && disposition.answers()
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
    tasks: BoundedVec<InboundTaskRequest, MAX_THREAD_ASSOCIATIONS>,
    holds: BoundedVec<InboundHold, MAX_THREAD_ASSOCIATIONS>,
    outreach_transitions: BoundedVec<InboundOutreachTransition, MAX_THREAD_ASSOCIATIONS>,
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
    ) -> AppResult<Self> {
        let primary = prepared
            .channels
            .first()
            .expect("resolution refuses a message with no authorized channel");

        // The interface the primary channel received it on is the message's source. Every other
        // channel gets its own binding-qualified mapping through its association.
        let mut envelope = draft.clone().bind(primary.candidate.binding_id);
        envelope.content = CanonicalContent::parse(draft.content.subject(), &prepared.body_text)?;

        let answers = prepared.channels.iter().any(|channel| channel.answers);
        let all_outreach = prepared
            .channels
            .iter()
            .all(|channel| channel.outreach.is_some());
        let disposition =
            super::policy::fold_disposition(draft.directives.disposition, answers, all_outreach);
        envelope.directives.disposition = disposition;

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

        // A held channel would have answered, but the team asked to be consulted first -- it is
        // dropped from every pipeline below (see `pipelines`) and gains a `thread_handoffs` row
        // here instead.
        let holds = BoundedVec::parse(
            "thread handoffs",
            prepared
                .channels
                .iter()
                .filter(|channel| holds(channel.hold, disposition))
                .map(|channel| InboundHold {
                    channel_id: channel.candidate.channel.id,
                    // Fresh on every held message. An existing row keeps its own id and takes only
                    // the new generation, so a replacement is one `ON CONFLICT` away.
                    handoff_id: Uuid::new_v4(),
                    generation: Uuid::new_v4(),
                })
                .collect(),
        )?;
        let outreach_transitions = BoundedVec::parse(
            "outreach transitions",
            prepared
                .channels
                .iter()
                .filter_map(|channel| {
                    channel
                        .outreach
                        .clone()
                        .map(|matched| InboundOutreachTransition {
                            channel_id: channel.candidate.channel.id,
                            matched,
                        })
                })
                .collect(),
        )?;

        Ok(Self {
            company_id: resolved.company.id,
            envelope,
            associations: BoundedVec::parse("thread associations", associations)?,
            // A held channel is excluded from every pipeline by `pipelines` itself, so a message
            // whose every answering channel is held naturally produces zero tasks -- no separate
            // guard is needed here the way the single-task shape once needed one.
            tasks: BoundedVec::parse(
                "inbound tasks",
                if disposition.answers() {
                    pipelines(&prepared.channels, disposition)
                        .into_iter()
                        .map(|targets| InboundTaskRequest {
                            task_type: AGENT_DISPATCH_TASK.to_string(),
                            targets,
                        })
                        .collect()
                } else {
                    Vec::new()
                },
            )?,
            holds,
            outreach_transitions,
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
            tasks: self.tasks.clone(),
            holds: self.holds.clone(),
            outreach_transitions: self.outreach_transitions.clone(),
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

/// Keep each addressed pipeline independent after routing has dropped repeated/passive channels.
///
/// A held channel is excluded here too, for the same reason the single-task shape excluded it from
/// `targets`: it *would* have answered, but the team asked to be consulted first, and it gains a
/// `thread_handoffs` row instead of a place in any pipeline.
fn pipelines(
    channels: &[PreparedChannel],
    disposition: MessageDisposition,
) -> Vec<Vec<InboundTaskTarget>> {
    let mut groups: Vec<Vec<InboundTaskTarget>> = Vec::new();
    let mut previous_handle = None;
    for channel in channels.iter().filter(|channel| {
        channel.answers && channel.outreach.is_none() && !holds(channel.hold, disposition)
    }) {
        let target = InboundTaskTarget {
            channel_id: channel.candidate.channel.id,
            role: channel.candidate.role,
        };
        let handle = &channel.candidate.handle;
        if previous_handle == Some(handle) {
            groups
                .last_mut()
                .expect("a previous handle has a group")
                .push(target);
        } else {
            groups.push(vec![target]);
        }
        previous_handle = Some(handle);
    }
    groups
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
        transport::{
            IngressDirectives, IngressPolicyFacts, PipelineStep, ProtocolExtension,
            test_support::email_identity,
        },
    };
    use chrono::Utc;

    fn company() -> Company {
        Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme".to_string(),
            slug: "acme".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        }
    }

    fn channel(company_id: Uuid, slug: &str) -> Channel {
        Channel {
            response_trigger: crate::entities::channel::ChannelResponseTrigger::Always,
            owner_agent_id: None,
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
            agent_ids: None,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: CreationProvenance::system(),
            created_at: Utc::now(),
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

    /// One answering channel, with the hold verdict `prepare_channels` would have reached. Each
    /// gets its own address, so it forms a single-channel pipeline of its own.
    fn prepared_channel(company: &Company, slug: &str, hold: HoldDecision) -> PreparedChannel {
        let channel = channel(company.id, slug);
        PreparedChannel {
            candidate: ChannelCandidate {
                company: company.clone(),
                binding_id: ChannelBindingId::new(channel.id),
                matched_slug: channel.slug.clone(),
                handle: identity(&format!("{slug}@acme.example")),
                role: RecipientRole::To,
                step: PipelineStep::only(),
                access: ParticipantAccess {
                    authorized: true,
                    trusted: false,
                },
                channel,
            },
            target: ThreadTarget::Existing(Uuid::new_v4()),
            answers: true,
            outreach: None,
            principals: BoundedVec::empty(),
            hold,
        }
    }

    fn plan_for(company: &Company, channels: Vec<PreparedChannel>) -> InboundCommitRequest {
        plan_with_disposition(company, channels, MessageDisposition::Answer)
    }

    fn plan_with_disposition(
        company: &Company,
        channels: Vec<PreparedChannel>,
        disposition: MessageDisposition,
    ) -> InboundCommitRequest {
        let resolved = ResolvedAddresses {
            company: company.clone(),
            candidates: Vec::new(),
            outreach_by_channel: std::collections::HashMap::new(),
        };
        CommitPlan::build(
            &draft(disposition),
            &resolved,
            PreparedChannels {
                channels,
                body_text: "Any news?".to_string(),
            },
            ReplyDelivery::Send,
        )
        .expect("a hand-made plan is within every bound")
        .request()
    }

    #[test]
    fn a_held_channel_leaves_the_task_to_its_automatic_sibling() {
        let company = company();
        let support = prepared_channel(&company, "support", HoldDecision::Hold);
        let billing = prepared_channel(&company, "billing", HoldDecision::Automatic);
        let support_id = support.candidate.channel.id;
        let billing_id = billing.candidate.channel.id;

        let request = plan_for(&company, vec![support, billing]);

        assert_eq!(request.tasks.len(), 1, "only billing's pipeline answers");
        let task = request.tasks.first().expect("billing still answers");
        assert_eq!(
            task.targets
                .iter()
                .map(|target| target.channel_id)
                .collect::<Vec<_>>(),
            vec![billing_id]
        );
        assert_eq!(
            request
                .holds
                .iter()
                .map(|hold| hold.channel_id)
                .collect::<Vec<_>>(),
            vec![support_id]
        );
    }

    #[test]
    fn holding_every_answering_channel_leaves_no_task_at_all() {
        let company = company();
        let channels = vec![
            prepared_channel(&company, "support", HoldDecision::Hold),
            prepared_channel(&company, "billing", HoldDecision::Hold),
        ];
        let expected: Vec<Uuid> = channels
            .iter()
            .map(|channel| channel.candidate.channel.id)
            .collect();

        let request = plan_for(&company, channels);

        assert!(request.tasks.is_empty(), "no channel is left to answer");
        assert_eq!(
            request
                .holds
                .iter()
                .map(|hold| hold.channel_id)
                .collect::<Vec<_>>(),
            expected
        );
        // Every generation is its own, even within one commit: two threads never share one.
        assert_ne!(request.holds[0].generation, request.holds[1].generation);
        assert_ne!(request.holds[0].handoff_id, request.holds[1].handoff_id);
    }

    /// The two gates are independent and both fire here. A `.quiet` message already produces no
    /// task, and asking the team to act on a message that asked nobody to act would be wrong.
    #[test]
    fn a_file_only_message_is_neither_answered_nor_held() {
        let company = company();
        let request = plan_with_disposition(
            &company,
            vec![prepared_channel(&company, "support", HoldDecision::Hold)],
            MessageDisposition::FileOnly,
        );

        assert!(request.tasks.is_empty());
        assert!(request.holds.is_empty());
        // The hold changes the task, never the message: this is still the customer's own message.
        assert_eq!(
            request.envelope.directives.disposition,
            MessageDisposition::FileOnly
        );
    }

    /// A reply that closes an outreach is `NotEligible` by the time it gets here, and the existing
    /// `outreach.is_none()` term is what keeps it out of the task.
    #[test]
    fn a_channel_that_closes_an_outreach_is_neither_held_nor_targeted() {
        let company = company();
        let mut support = prepared_channel(&company, "support", HoldDecision::NotEligible);
        support.outreach = Some(OutreachReplyMatch {
            outreach_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            target_id: Uuid::new_v4(),
            target_email: "ana@client.com".into(),
        });
        let billing = prepared_channel(&company, "billing", HoldDecision::Automatic);
        let billing_id = billing.candidate.channel.id;

        let request = plan_for(&company, vec![support, billing]);

        assert!(request.holds.is_empty());
        let task = request.tasks.first().expect("billing still answers");
        assert_eq!(
            task.targets
                .iter()
                .map(|target| target.channel_id)
                .collect::<Vec<_>>(),
            vec![billing_id]
        );
    }

    /// A held message keeps the disposition it arrived with. Folding the hold into
    /// `MessageDisposition` would reclassify a paying customer's email as a private internal note.
    #[test]
    fn a_held_message_keeps_the_answer_disposition() {
        let company = company();
        let request = plan_for(
            &company,
            vec![prepared_channel(&company, "support", HoldDecision::Hold)],
        );

        assert_eq!(
            request.envelope.directives.disposition,
            MessageDisposition::Answer
        );
        assert_eq!(request.holds.len(), 1);
        assert!(request.tasks.is_empty());
    }

    fn prepared(handle: &str, step: PipelineStep) -> PreparedChannel {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme".into(),
            slug: "acme".into(),
            channel_defaults: Default::default(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };
        let channel = Channel {
            response_trigger: crate::entities::channel::ChannelResponseTrigger::Always,
            id: Uuid::new_v4(),
            company_id: company.id,
            owner_agent_id: None,
            name: "Support".into(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            agent_ids: None,
            enabled: true,
            add_3rd_party: false,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        };
        PreparedChannel {
            candidate: ChannelCandidate {
                company,
                channel,
                binding_id: ChannelBindingId::random(),
                matched_slug: "support".into(),
                handle: email_identity(handle),
                role: RecipientRole::To,
                step,
                access: ParticipantAccess {
                    authorized: true,
                    trusted: true,
                },
            },
            target: ThreadTarget::Create {
                subject: "Question".into(),
            },
            answers: true,
            outreach: None,
            principals: BoundedVec::parse("principals", Vec::new()).unwrap(),
            hold: HoldDecision::Automatic,
        }
    }

    #[test]
    fn a_pipeline_with_its_first_step_dropped_keeps_its_address_boundary() {
        let first = prepared("support@acme.example", PipelineStep::only());
        let second = prepared(
            "support+sales+legal@acme.example",
            PipelineStep { index: 1, total: 3 },
        );
        let third = prepared(
            "support+sales+legal@acme.example",
            PipelineStep { index: 2, total: 3 },
        );
        let ids = [
            first.candidate.channel.id,
            second.candidate.channel.id,
            third.candidate.channel.id,
        ];
        let groups = pipelines(&[first, second, third], MessageDisposition::Answer);
        assert_eq!(
            groups
                .iter()
                .map(|targets| targets
                    .iter()
                    .map(|target| target.channel_id)
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![vec![ids[0]], vec![ids[1], ids[2]]]
        );
    }
}
