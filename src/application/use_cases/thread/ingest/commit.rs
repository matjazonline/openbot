//! Phase 5: everything one accepted message must make durable, assembled as one request.
//!
//! Nothing here awaits. The plan is built from values the earlier phases already loaded, handed to
//! [`InboundMessageCommitter`](crate::transport::InboundMessageCommitter) in a single call, and
//! read back afterwards. That is the whole reason the phases before it are read-only: a rejection
//! that happens at any point up to here leaves no row behind to clean up.

use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::outreach::OutreachReplyMatch,
    transport::{
        BoundedVec, CanonicalContent, InboundCommitRequest, InboundDraft, InboundEnvelope,
        InboundOutreachTransition, InboundTaskRequest, InboundTaskTarget, MAX_THREAD_ASSOCIATIONS,
        MessageDisposition, NewDelivery, ThreadAssociation, ThreadPrincipalIntent, ThreadTarget,
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
            tasks: BoundedVec::parse(
                "inbound tasks",
                if disposition.answers() {
                    pipelines(&prepared.channels)
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
fn pipelines(channels: &[PreparedChannel]) -> Vec<Vec<InboundTaskTarget>> {
    let mut groups: Vec<Vec<InboundTaskTarget>> = Vec::new();
    let mut previous_handle = None;
    for channel in channels
        .iter()
        .filter(|channel| channel.answers && channel.outreach.is_none())
    {
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
            transport::{ChannelBindingId, RecipientRole},
        },
        transport::{PipelineStep, test_support::email_identity},
    };

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
        let groups = pipelines(&[first, second, third]);
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
