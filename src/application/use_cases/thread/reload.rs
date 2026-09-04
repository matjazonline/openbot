//! Rebuilding one inbound message's world from the identifiers a task carries.
//!
//! A durable task holds ids, not entities. Everything the agent run needs is therefore loaded here,
//! with tenant-scoped queries, at the moment the run starts -- so a task that has been queued for
//! an hour answers against the channel's *current* configuration rather than a snapshot taken when
//! the mail arrived.
//!
//! The one thing that cannot be reloaded is what was true of the *delivery* rather than of the
//! message: relay hop count, the channels already traced, whether the sender forwarded someone
//! else's words, and whether the answer was asked to stay in the app. Those come from the payload,
//! which is why [`InboundTaskPayloadV1`] carries exactly them and nothing else.

use std::sync::Arc;

use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        message::{Message, MessageParticipantKind},
        transport::{
            ChannelBindingId, ExternalMessageKey, ExternalThreadKey, InboundSource, TransportKind,
        },
    },
    transport::{
        AddressedIdentity, BoundedVec, CanonicalContent, InboundEnvelope, InboundTaskPayloadV1,
        IngressDirectives, IngressPolicyFacts, MessageDisposition, RecipientRole, ReplyCandidates,
    },
    use_cases::thread::{
        ChannelMatch, InboundIngestResult, PipelineStep, TaskChannelTarget, ThreadUseCases,
    },
};

impl ThreadUseCases {
    /// Reload everything one queued agent run needs.
    ///
    /// Every missing or incoherent row is an error rather than a skipped field: a task whose
    /// channel was deleted, or whose thread belongs to another company, is a task that must fail
    /// visibly instead of running an agent against half a world.
    pub async fn load_inbound_task(
        &self,
        task_id: Uuid,
        payload: &InboundTaskPayloadV1,
    ) -> AppResult<InboundIngestResult> {
        let company = self
            .company_persistence
            .get_by_id(payload.company_id)
            .await?
            .ok_or_else(|| AppError::NotFound("The task's company no longer exists".into()))?;

        let message = self
            .thread_persistence
            .get_thread_message(payload.thread_id, payload.source_message_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound("The message this task answers no longer exists".into())
            })?;
        if message.company_id != company.id {
            return Err(AppError::Internal(
                "The task's message belongs to another company".into(),
            ));
        }

        let targets = self
            .task_persistence
            .list_task_channel_targets(company.id, task_id)
            .await?;
        let total = targets.len();
        let mut channel_matches = Vec::with_capacity(total);
        for (index, target) in targets.into_iter().enumerate() {
            channel_matches.push(
                self.reload_channel_match(
                    &company,
                    &message,
                    target,
                    PipelineStep { index, total },
                )
                .await?,
            );
        }
        let primary = channel_matches
            .iter()
            .position(|matched| matched.channel.id == payload.channel_id)
            .ok_or_else(|| {
                AppError::Internal("The task's own channel is not among its targets".into())
            })?;
        channel_matches.swap(0, primary);

        let envelope = self
            .reload_envelope(payload, &channel_matches[0], &message)
            .await?;

        Ok(InboundIngestResult {
            accepted: true,
            rejection: None,
            thread: Some(channel_matches[0].thread.clone()),
            inbound_message: Some(channel_matches[0].inbound_message.clone()),
            company: Some(channel_matches[0].company.clone()),
            channel: Some(channel_matches[0].channel.clone()),
            envelope: Some(Arc::new(envelope)),
            task_id: None,
            reply_delivery: payload.reply_delivery,
            channel_matches,
        })
    }

    async fn reload_channel_match(
        &self,
        company: &crate::entities::company::Company,
        message: &Message,
        target: TaskChannelTarget,
        step: PipelineStep,
    ) -> AppResult<ChannelMatch> {
        let channel = self
            .channel_persistence
            .get_by_id(target.channel_id)
            .await?
            .ok_or_else(|| AppError::NotFound("A target channel no longer exists".into()))?;
        if channel.company_id != company.id {
            return Err(AppError::Internal(
                "A target channel belongs to another company".into(),
            ));
        }
        let thread = self
            .thread_persistence
            .get_thread_by_id(target.thread_id)
            .await?
            .ok_or_else(|| AppError::NotFound("A target thread no longer exists".into()))?;
        if thread.channel_id != channel.id {
            return Err(AppError::Internal(
                "A target thread belongs to another channel".into(),
            ));
        }
        let inbound_message = self
            .thread_persistence
            .get_thread_message(thread.id, message.canonical_id)
            .await?
            .ok_or_else(|| {
                AppError::Internal(
                    "A target thread does not hold the message its task answers".into(),
                )
            })?;

        Ok(ChannelMatch {
            company: company.clone(),
            channel,
            // The alias the sender wrote to is display-only and is not stored; the channel's own
            // slug is the honest answer rather than a guess at which alias was used.
            matched_slug: None,
            thread,
            inbound_message,
            recipient_role: target.recipient_role,
            step,
        })
    }

    /// The canonical message as its adapter delivered it, rebuilt from what was stored.
    ///
    /// Authentication is deliberately *not* reconstructed. The verdicts were consumed by the
    /// ingress guard before this message existed and nothing downstream reads them, so the
    /// reloaded envelope says [`IngressPolicyFacts::TrustedApplication`] -- "this was admitted by
    /// the platform" -- rather than restating a DMARC pass it cannot verify.
    async fn reload_envelope(
        &self,
        payload: &InboundTaskPayloadV1,
        primary: &ChannelMatch,
        message: &Message,
    ) -> AppResult<InboundEnvelope> {
        let binding_id = self.email_binding_of(primary).await?;
        let extension = self
            .thread_persistence
            .get_message_protocol_extension(message.company_id, message.canonical_id)
            .await?;
        let email = extension.email_metadata();
        let message_key = ExternalMessageKey::parse(
            email
                .as_ref()
                .map(|metadata| metadata.rfc_message_id.as_str().trim().to_string())
                .unwrap_or_else(|| message.canonical_id.to_string()),
        )
        .map_err(|error| AppError::Internal(format!("Unusable stored message key: {error}")))?;
        let thread_key = ExternalThreadKey::parse(
            email
                .as_ref()
                .map(|metadata| metadata.conversation_root_key().as_str().trim().to_string())
                .unwrap_or_else(|| message.canonical_id.to_string()),
        )
        .map_err(|error| AppError::Internal(format!("Unusable stored thread key: {error}")))?;

        let author = message.author.identity.clone().ok_or_else(|| {
            AppError::Internal("The message this task answers has no author handle".into())
        })?;

        let mut addressed = Vec::new();
        for (kind, role) in [
            (MessageParticipantKind::To, RecipientRole::To),
            (MessageParticipantKind::Cc, RecipientRole::Cc),
        ] {
            let mut participants: Vec<_> = message
                .participants
                .iter()
                .filter(|participant| participant.kind == kind)
                .collect();
            participants.sort_by_key(|participant| participant.position);
            addressed.extend(
                participants
                    .into_iter()
                    .map(|participant| AddressedIdentity::new(role, participant.identity.clone())),
            );
        }

        Ok(InboundEnvelope {
            source: InboundSource {
                binding_id,
                event_key: None,
                message_key,
                thread_key,
            },
            author,
            addressed: BoundedVec::parse("addressed identities", addressed)?,
            content: CanonicalContent::parse(&message.subject, &message.clean_text_body)?,
            attachments: BoundedVec::parse(
                "attachments",
                message.attachments.clone().unwrap_or_default(),
            )?,
            // Thread resolution is finished: this run answers a message that already has its
            // threads, so there is nothing left to correlate.
            reply_candidates: ReplyCandidates::default(),
            directives: IngressDirectives {
                hop_count: payload.hop_count,
                trace_channels: payload.trace_channels.clone(),
                // A task exists only for a message that asked for an answer.
                disposition: MessageDisposition::Answer,
                source_channel_id: None,
                target_thread_id: Some(payload.thread_id),
                is_auto_reply: false,
                is_forwarded: payload.is_forwarded,
            },
            policy: IngressPolicyFacts::TrustedApplication,
            correlation_id: payload.correlation_id,
            extension,
        })
    }

    /// The channel's email interface, which is what an outbound reply addresses.
    async fn email_binding_of(&self, primary: &ChannelMatch) -> AppResult<ChannelBindingId> {
        self.binding_persistence
            .active_bindings_for_channel(primary.company.id, primary.channel.id)
            .await?
            .iter()
            .find(|binding| binding.transport == TransportKind::Email)
            .map(|binding| binding.id)
            .ok_or_else(|| {
                AppError::Internal(format!(
                    "Channel '{}' has no active email interface to answer through",
                    primary.channel.slug
                ))
            })
    }
}
