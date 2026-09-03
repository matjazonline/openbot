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
    task: Option<InboundTaskRequest>,
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

        // Passive matches have their copy filed on their history; keeping them out of the task is
        // what stops the worker treating a channel that was merely copied as one that must answer.
        let targets: Vec<_> = prepared
            .channels
            .iter()
            .filter(|channel| channel.answers && channel.outreach.is_none())
            .map(|channel| InboundTaskTarget {
                channel_id: channel.candidate.channel.id,
                role: channel.candidate.role,
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
            task: (disposition.answers() && !targets.is_empty()).then(|| InboundTaskRequest {
                task_type: AGENT_DISPATCH_TASK.to_string(),
                targets,
            }),
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
            task: self.task.clone(),
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
