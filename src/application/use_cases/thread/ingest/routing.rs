//! Phases 2 to 4: turning what an adapter said into the channels, principals and threads a commit
//! needs.
//!
//! Every function here does one round of reads against one port and returns a named outcome. What
//! none of them does is write: nothing an inbound message produces becomes durable until
//! [`super::commit::CommitPlan`] hands the whole set to the committer at once.

use std::collections::{HashMap, HashSet};

use tracing::warn;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        channel::Channel,
        company::Company,
        email_message::EmailMessageMetadata,
        outreach::OutreachReplyMatch,
        participant::{PrincipalAccessContext, ThreadPrincipalRole},
        thread::Thread,
        transport::{
            ChannelBindingId, ChannelSelector, DeliveryPurpose, QualifiedIdentity, TransportKind,
        },
        value_objects::{ChannelSlug, EmailAddress, MessageId},
    },
    transport::{
        CanonicalContent, DeliveryContext, EmailDeliveryContext, EmailThreading, InboundDraft,
        InboundRouting, PipelineStep, RecipientRole, StandaloneDeliveryRequest, SystemAddress,
        ThreadTarget,
    },
    use_cases::{
        channel::find_similar_channel_slugs,
        thread::{
            BounceInfo, ChannelDirectoryEntry, ThreadUseCases,
            ingest::{
                commit::{PreparedChannel, PreparedChannels},
                policy::{
                    IngestRejection, IngressOrigin, UndeliverableKind, UndeliverableReason,
                    UndeliverableSlugs, exceeds_spam_threshold, exceeds_turn_limit,
                    thread_participants,
                },
            },
            support::{
                DirectoryCache, body_mentions_email, body_mentions_slug, strip_quoted_history,
            },
        },
    },
};

/// One channel this message named, and the interface it reached that channel through.
///
/// The binding is resolved here rather than inside the commit because a mail addressed to three
/// channels arrives on three of them, and "the channel's own email interface" is an email rule that
/// must not be re-derived in the transaction that writes the mapping rows.
#[derive(Debug, Clone)]
pub(crate) struct ChannelCandidate {
    pub company: Company,
    pub channel: Channel,
    pub binding_id: ChannelBindingId,
    /// The slug the envelope actually named -- an alias, or the channel's canonical slug.
    pub matched_slug: ChannelSlug,
    /// The handle that produced this match. Pipeline steps of one address share it.
    pub handle: QualifiedIdentity,
    pub role: RecipientRole,
    pub step: PipelineStep,
    /// This sender's standing on the channel, resolved once during authorization.
    pub access: crate::entities::channel::ParticipantAccess,
}

/// Everything phase 2 learned about where the message was addressed.
pub(crate) struct ResolvedAddresses {
    pub company: Company,
    pub candidates: Vec<ChannelCandidate>,
    /// Channels whose inbound message answers an outreach this platform is already awaiting.
    pub outreach_by_channel: HashMap<Uuid, OutreachReplyMatch>,
}

/// Why a single candidate channel did or didn't make it into the match list.
///
/// An outcome enum rather than a bool plus out-parameters, so the loop that drives it is a `match`
/// that pushes into the right accumulator.
enum ChannelVerdict {
    Accept(Box<Option<OutreachReplyMatch>>),
    Unauthorized,
    Reject(IngestRejection),
}

/// How an already-existing thread authorizes -- or refuses -- this sender.
enum ThreadAccess {
    Denied,
    Participant,
    OutreachReply(Box<OutreachReplyMatch>),
}

/// One inbound sender, in both the terms the thread rules need.
///
/// `context` carries the stable actor that decides participation; `handle` is the transport
/// identity outreach correlation and the bounce still speak in. Carrying them together stops a
/// caller passing one thread's principal alongside another's handle.
#[derive(Clone, Copy)]
pub(crate) struct ThreadSender<'a> {
    pub handle: &'a QualifiedIdentity,
    pub context: PrincipalAccessContext,
}

impl<'a> ThreadSender<'a> {
    pub fn new(handle: &'a QualifiedIdentity, context: PrincipalAccessContext) -> Self {
        Self { handle, context }
    }

    pub fn address(self) -> &'a str {
        self.handle.subject().as_str()
    }

    /// Whether this sender is already a party to the thread. A handle that resolved to no
    /// principal is nobody, and is never a party to anything.
    fn is_party_to(self, thread: &Thread) -> bool {
        self.context
            .principal_id
            .is_some_and(|principal_id| thread.contains_principal(principal_id))
    }
}

impl ThreadUseCases {
    /// Phases 2 and 3: walk every `To` then `Cc` target, expanding channel pipelines into
    /// authorized matches.
    ///
    /// The two phases share one walk because they share the directory cache and the address order;
    /// splitting them would resolve every company and channel list twice. The *decisions* stay
    /// apart: [`Self::authorize_channel`] is the only place a sender is let in or turned away.
    ///
    /// A pipeline is all-or-nothing: one undeliverable hop bounces the whole message, so the sender
    /// learns about the typo -- or the closed channel -- instead of silently losing a step.
    pub(crate) async fn resolve_addresses(
        &self,
        draft: &InboundDraft,
        routing: &InboundRouting,
        origin: IngressOrigin,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<Result<ResolvedAddresses, IngestRejection>> {
        let mut candidates: Vec<ChannelCandidate> = Vec::new();
        let mut outreach_by_channel = HashMap::new();
        let mut seen_channels = HashSet::new();
        let mut unauthorized = 0usize;
        let mut undeliverable = UndeliverableSlugs::default();
        let mut matched_company: Option<Company> = None;

        for (recipient, selectors) in routing.channel_pipelines() {
            let total = selectors.len();
            for (index, selector) in selectors.iter().enumerate() {
                let Some(company) = self.selector_company(selector, origin, directory).await?
                else {
                    continue;
                };
                if let Some(source_company) = origin.internal_company()
                    && company.id != source_company
                {
                    return Ok(Err(IngestRejection::CrossCompanyInternal));
                }
                if matched_company
                    .as_ref()
                    .is_some_and(|first| first.id != company.id)
                {
                    return Ok(Err(IngestRejection::CrossCompanyPipeline));
                }
                matched_company = Some(company.clone());

                let available = directory.channels(company.id).await?;
                let slug = selector.channel().clone();
                let Some(channel) = available
                    .iter()
                    .find(|candidate| candidate.matches_slug(&slug))
                    .cloned()
                else {
                    undeliverable
                        .unknown(slug.clone(), find_similar_channel_slugs(&slug, &available));
                    continue;
                };

                // Disabled is undeliverable for everyone, so it is decided before the ACL, which
                // rules only on *this sender's* access.
                if !channel.enabled {
                    warn!(channel = %channel.slug, "Bouncing a message for a disabled channel");
                    undeliverable.disabled(slug);
                    continue;
                }
                if !seen_channels.insert(channel.id) {
                    continue;
                }

                let binding_id = self.inbound_binding(&company, &channel).await?;
                let context = directory.access_context(company.id, &draft.author).await?;
                match self
                    .authorize_channel(&channel, binding_id, draft, origin, context)
                    .await?
                {
                    ChannelVerdict::Reject(rejection) => return Ok(Err(rejection)),
                    ChannelVerdict::Unauthorized => unauthorized += 1,
                    ChannelVerdict::Accept(outreach) => {
                        if let Some(matched) = *outreach {
                            outreach_by_channel.insert(channel.id, matched);
                        }
                        let access = channel.participant_access(context);
                        candidates.push(ChannelCandidate {
                            company: company.clone(),
                            channel,
                            binding_id,
                            matched_slug: slug,
                            handle: recipient.handle.clone(),
                            role: recipient.role,
                            step: PipelineStep { index, total },
                            access,
                        });
                    }
                }
            }
        }

        if let Some(kind) = undeliverable.kind() {
            return Ok(Err(self
                .undeliverable_rejection(draft, kind, undeliverable, matched_company, directory)
                .await?));
        }
        let Some(company) = matched_company.filter(|_| !candidates.is_empty()) else {
            return Ok(Err(if unauthorized > 0 {
                IngestRejection::Unauthorized
            } else {
                IngestRejection::UnknownRecipient
            }));
        };

        Ok(Ok(ResolvedAddresses {
            company,
            candidates,
            outreach_by_channel,
        }))
    }

    /// The company a selector names: the one it qualified, or -- for an unqualified selector, which
    /// only a trusted internal relay can produce -- the sender's own.
    async fn selector_company(
        &self,
        selector: &ChannelSelector,
        origin: IngressOrigin,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<Option<Company>> {
        match selector.company() {
            Some(slug) => directory.company(slug).await,
            None => match origin.internal_company() {
                Some(company_id) => self.company_persistence.get_by_id(company_id).await,
                None => Ok(None),
            },
        }
    }

    /// The interface an inbound message reached one channel through.
    ///
    /// Email is a deployment transport, so every channel has exactly one email binding from the
    /// moment it is created. A channel with none is a provisioning fault, not a routing decision,
    /// which is why it propagates instead of quietly dropping the recipient.
    async fn inbound_binding(
        &self,
        company: &Company,
        channel: &Channel,
    ) -> AppResult<ChannelBindingId> {
        self.binding_persistence
            .active_bindings_for_channel(company.id, channel.id)
            .await?
            .iter()
            .find(|binding| binding.transport == TransportKind::Email)
            .map(|binding| binding.id)
            .ok_or_else(|| {
                AppError::Internal(format!(
                    "Channel '{}' has no active email interface to receive mail on",
                    channel.slug
                ))
            })
    }

    /// Phase 3: loop protection, ACL and spam scoring for one candidate channel.
    ///
    /// Every fallible call here feeds an authorization decision, so every one of them propagates: a
    /// directory outage must not read as "this sender is an outsider".
    async fn authorize_channel(
        &self,
        channel: &Channel,
        binding_id: ChannelBindingId,
        draft: &InboundDraft,
        origin: IngressOrigin,
        context: PrincipalAccessContext,
    ) -> AppResult<ChannelVerdict> {
        let sender = ThreadSender::new(&draft.author, context);
        let mut outreach = None;

        // A channel may appear twice in one trace only when this is the correlated response to an
        // outreach it is already waiting on; anything else is a routing cycle.
        if origin.is_internal() && draft.directives.trace_channels.contains(&channel.id) {
            match self
                .outreach_reply_in_referenced_thread(channel, binding_id, draft, sender)
                .await?
            {
                Some(matched) => outreach = Some(matched),
                None => {
                    warn!(channel_id = %channel.id, "Refusing an inter-channel routing cycle");
                    return Ok(ChannelVerdict::Reject(IngestRejection::LoopCycle));
                }
            }
        }

        let access = channel.participant_access(context);

        // Someone who isn't on the channel ACL may still be a party to an existing thread.
        let mut thread_authorized = false;
        if !access.authorized && !origin.is_internal() {
            match self
                .thread_access_for(channel, binding_id, draft, sender)
                .await?
            {
                ThreadAccess::Denied => {}
                ThreadAccess::Participant => thread_authorized = true,
                ThreadAccess::OutreachReply(matched) => {
                    thread_authorized = true;
                    outreach = Some(*matched);
                }
            }
        }

        if !access.authorized && !thread_authorized && !origin.is_internal() {
            warn!(channel = %channel.slug, "Sender is not authorized for this channel");
            return Ok(ChannelVerdict::Unauthorized);
        }

        if exceeds_spam_threshold(&draft.policy, access, self.config.max_spam_score) {
            warn!(channel = %channel.slug, "Refusing a message over the spam-score threshold");
            return Ok(ChannelVerdict::Reject(IngestRejection::SpamScore));
        }

        Ok(ChannelVerdict::Accept(Box::new(outreach)))
    }

    /// Whether an existing thread lets this sender in, either as a party to it or as the target of
    /// an outreach awaiting their reply.
    async fn thread_access_for(
        &self,
        channel: &Channel,
        binding_id: ChannelBindingId,
        draft: &InboundDraft,
        sender: ThreadSender<'_>,
    ) -> AppResult<ThreadAccess> {
        let Some(thread) = self
            .find_ancestor_thread(channel, binding_id, draft)
            .await?
        else {
            return Ok(ThreadAccess::Denied);
        };
        if sender.is_party_to(&thread) {
            return Ok(ThreadAccess::Participant);
        }
        Ok(
            match self
                .correlated_outreach_reply(channel, thread.id, draft, sender)
                .await?
            {
                Some(matched) => ThreadAccess::OutreachReply(Box::new(matched)),
                None => ThreadAccess::Denied,
            },
        )
    }

    /// Does this message answer an outreach the channel is awaiting in the thread it references?
    async fn outreach_reply_in_referenced_thread(
        &self,
        channel: &Channel,
        binding_id: ChannelBindingId,
        draft: &InboundDraft,
        sender: ThreadSender<'_>,
    ) -> AppResult<Option<OutreachReplyMatch>> {
        let Some(thread) = self
            .find_ancestor_thread(channel, binding_id, draft)
            .await?
        else {
            return Ok(None);
        };
        self.correlated_outreach_reply(channel, thread.id, draft, sender)
            .await
    }

    async fn correlated_outreach_reply(
        &self,
        channel: &Channel,
        thread_id: Uuid,
        draft: &InboundDraft,
        sender: ThreadSender<'_>,
    ) -> AppResult<Option<OutreachReplyMatch>> {
        let references = reference_ids(draft);
        if references.is_empty() {
            return Ok(None);
        }
        self.task_persistence
            .find_correlated_outreach_reply(
                channel.company_id,
                channel.id,
                thread_id,
                sender.address(),
                &references,
            )
            .await
    }

    /// Phase 4: the conversation this message continues, if any.
    ///
    /// Ordered exactly as the architecture requires: the provider conversation mapping this binding
    /// already holds, then the nearest ancestor message the sender named, and only then the Outlook
    /// `Thread-Index` header -- a heuristic, which must never override a key the platform issued.
    async fn resolve_thread(
        &self,
        candidate: &ChannelCandidate,
        draft: &InboundDraft,
    ) -> AppResult<Option<Thread>> {
        if let Some(target_thread_id) = draft.directives.target_thread_id
            && let Some(thread) = self
                .thread_persistence
                .get_thread_by_id(target_thread_id)
                .await?
            && thread.channel_id == candidate.channel.id
        {
            return Ok(Some(thread));
        }

        if let Some(thread_id) = self
            .correlation_store
            .thread_for_thread_keys(candidate.binding_id, &draft.reply_thread_keys)
            .await?
            && let Some(thread) = self.thread_persistence.get_thread_by_id(thread_id).await?
            && thread.channel_id == candidate.channel.id
        {
            return Ok(Some(thread));
        }

        if let Some(thread) = self
            .find_ancestor_thread(&candidate.channel, candidate.binding_id, draft)
            .await?
        {
            return Ok(Some(thread));
        }

        let Some(index) = email_metadata(draft).and_then(|metadata| metadata.thread_index.clone())
        else {
            return Ok(None);
        };
        Ok(self
            .thread_persistence
            .find_thread_by_thread_index(candidate.channel.id, &index)
            .await?
            .filter(|thread| thread.channel_id == candidate.channel.id))
    }

    /// The thread already holding one of the messages this one names -- including itself, so a
    /// redelivery lands back where its first delivery did -- nearest ancestor first.
    async fn find_ancestor_thread(
        &self,
        channel: &Channel,
        binding_id: ChannelBindingId,
        draft: &InboundDraft,
    ) -> AppResult<Option<Thread>> {
        let mut keys = Vec::with_capacity(draft.reply_message_keys.len() + 1);
        keys.push(draft.message_key.clone());
        keys.extend(draft.reply_message_keys.iter().cloned());

        let Some(thread_id) = self
            .correlation_store
            .thread_for_message_keys(binding_id, &keys)
            .await?
        else {
            return Ok(None);
        };
        Ok(self
            .thread_persistence
            .get_thread_by_id(thread_id)
            .await?
            .filter(|thread| thread.channel_id == channel.id))
    }

    /// Phases 4 and 5: give every authorized channel a thread and decide what it may do.
    pub(crate) async fn prepare_channels(
        &self,
        draft: &InboundDraft,
        routing: &InboundRouting,
        resolved: &ResolvedAddresses,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<Result<PreparedChannels, IngestRejection>> {
        let sender_context = directory
            .access_context(resolved.company.id, &draft.author)
            .await?;
        let sender = ThreadSender::new(&draft.author, sender_context);
        let third_parties = third_party_handles(routing, sender.handle);

        let mut existing_threads = Vec::with_capacity(resolved.candidates.len());
        for candidate in &resolved.candidates {
            existing_threads.push(self.resolve_thread(candidate, draft).await?);
        }

        // One canonical message has one body, so the quoted history is stripped once -- against the
        // thread the *primary* channel continues, which is the conversation the sender was replying
        // in. The pre-canonical path stripped separately per channel and stored a copy each time.
        let body_text = match existing_threads.first().and_then(Option::as_ref) {
            Some(thread) => {
                let history = self
                    .thread_persistence
                    .list_agent_history(thread.id)
                    .await?;
                strip_quoted_history(draft, &history)
            }
            None => draft.content.body_text().to_string(),
        };

        let mut channels = Vec::with_capacity(resolved.candidates.len());
        for (candidate, existing) in resolved.candidates.iter().zip(existing_threads) {
            let outreach = resolved
                .outreach_by_channel
                .get(&candidate.channel.id)
                .cloned();

            let mut outreach = outreach;
            let target = match existing {
                Some(thread) => {
                    match self
                        .authorize_thread_injection(candidate, &thread, draft, sender, &outreach)
                        .await?
                    {
                        Err(rejection) => return Ok(Err(rejection)),
                        // A sender let in *because* they are answering an outreach closes it: the
                        // reply is recorded and the agent is not re-run for it. Dropping the match
                        // here is how the pre-canonical path left an outreach open forever.
                        Ok(matched) => outreach = outreach.or(matched),
                    }
                    let recent = self
                        .thread_persistence
                        .count_recent_messages(thread.id, 3600)
                        .await?;
                    if exceeds_turn_limit(recent) {
                        warn!(
                            thread_id = %thread.id,
                            recent,
                            "Refusing a message that would exceed the thread turn limit"
                        );
                        return Ok(Err(IngestRejection::ThreadTurnLimit));
                    }
                    ThreadTarget::Existing(thread.id)
                }
                None => ThreadTarget::Create {
                    subject: draft.content.subject().to_string(),
                },
            };

            // A third party joins the thread only when the sender is trusted *and* the channel
            // permits it: the flag can narrow who gets pulled in, never widen it.
            let pull_third_parties = candidate.access.trusted && candidate.channel.add_3rd_party;
            let participant_identities = thread_participants(
                &[],
                sender.handle,
                &third_parties,
                // Recording an outreach reply satisfies the outreach; it does not make its sender
                // a party to the channel's conversation.
                outreach.is_none(),
                pull_third_parties,
            );

            let mut principals = Vec::with_capacity(participant_identities.len() + 1);
            for identity in participant_identities {
                if matches!(target, ThreadTarget::Create { .. }) && identity == draft.author {
                    principals.push(crate::transport::ThreadPrincipalIntent::new(
                        identity.clone(),
                        ThreadPrincipalRole::Author,
                    ));
                }
                principals.push(crate::transport::ThreadPrincipalIntent::new(
                    identity,
                    ThreadPrincipalRole::Participant,
                ));
            }

            let answers = candidate.role == RecipientRole::To
                || self
                    .cc_was_mentioned(candidate, &body_text, directory)
                    .await?;

            channels.push(PreparedChannel {
                candidate: candidate.clone(),
                target,
                answers,
                outreach,
                principals: crate::transport::BoundedVec::parse("thread principals", principals)?,
            });
        }

        Ok(Ok(PreparedChannels {
            channels,
            body_text,
        }))
    }

    /// Refuse a sender who is neither on the thread nor answering one of its outreaches, and tell
    /// them so with a bounce rather than dropping the mail silently.
    ///
    /// Returns the outreach that let them in, when one did, so the caller can record the reply
    /// against it: an authorization that is discarded leaves the outreach waiting forever.
    async fn authorize_thread_injection(
        &self,
        candidate: &ChannelCandidate,
        thread: &Thread,
        draft: &InboundDraft,
        sender: ThreadSender<'_>,
        outreach: &Option<OutreachReplyMatch>,
    ) -> AppResult<Result<Option<OutreachReplyMatch>, IngestRejection>> {
        if let Some(matched) = self
            .correlated_outreach_reply(&candidate.channel, thread.id, draft, sender)
            .await?
        {
            return Ok(Ok(Some(matched)));
        }
        if sender.is_party_to(thread)
            || draft.directives.target_thread_id.is_some()
            || outreach.is_some()
        {
            return Ok(Ok(None));
        }

        warn!(
            thread_id = %thread.id,
            "Refusing an unauthorized thread injection and bouncing it back"
        );
        Ok(Err(IngestRejection::ThreadInjection(Box::new(
            BounceInfo {
                source_message_key: draft.message_key.clone(),
                recipient_to: EmailAddress::from(sender.address()),
                company_slug: Some(candidate.company.slug.clone()),
                invalid_slugs: vec![ChannelSlug::from(format!(
                    "Thread {} (Unauthorized Sender: {})",
                    thread.id,
                    sender.address()
                ))],
                disabled_slugs: vec![],
                suggestions: vec![],
                // This bounce fires *because* the sender has no standing on the thread, so it must not
                // hand them the company's channel directory.
                available_channels: vec![],
                original_subject: draft.content.subject().to_string(),
            },
        ))))
    }

    /// Whether a copied channel was actually named in the body.
    ///
    /// A `To` recipient is asked something; a `Cc` is only copied, and a copy runs the agent only
    /// when the writer named the channel, one of its aliases, its address, or one of its agents.
    async fn cc_was_mentioned(
        &self,
        candidate: &ChannelCandidate,
        body: &str,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<bool> {
        let canonical_address = candidate
            .channel
            .inbound_address(&candidate.company.slug, &self.config.app_domain_name);
        if body_mentions_email(body, candidate.handle.subject().as_str())
            || body_mentions_email(body, &canonical_address)
            || body_mentions_slug(body, &candidate.matched_slug)
            || body_mentions_slug(body, &candidate.channel.slug)
            || candidate
                .channel
                .alias_slugs
                .iter()
                .any(|slug| body_mentions_slug(body, slug))
        {
            return Ok(true);
        }

        for agent_id in candidate.channel.agent_ids.iter().flatten() {
            if let Some(agent) = directory.agent(*agent_id).await?
                && body_mentions_slug(body, &agent.slug)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The bounce an undeliverable address earns, with the directory the sender is allowed to see.
    async fn undeliverable_rejection(
        &self,
        draft: &InboundDraft,
        kind: UndeliverableKind,
        undeliverable: UndeliverableSlugs,
        matched_company: Option<Company>,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<IngestRejection> {
        let (company_slug, available_channels) = match matched_company {
            Some(ref company) => (
                Some(company.slug.clone()),
                self.writable_channel_directory(company, &draft.author, directory)
                    .await?,
            ),
            None => (None, Vec::new()),
        };
        Ok(IngestRejection::Undeliverable(Box::new(
            UndeliverableReason {
                kind,
                bounce: undeliverable.into_bounce(
                    draft.message_key.clone(),
                    sender_address(draft),
                    company_slug,
                    available_channels,
                    draft.content.subject().to_string(),
                ),
            },
        )))
    }

    /// The channels this sender could have written to, for the bounce body.
    ///
    /// Empty unless the sender is on the company's team: someone outside it who guesses at a
    /// channel name must not learn the company's directory from the bounce they earn. Eligibility
    /// is decided by [`Channel::participant_access`] -- the same predicate the delivery path
    /// applies -- so the list can never advertise an address that would bounce in turn.
    pub(crate) async fn writable_channel_directory(
        &self,
        company: &Company,
        sender: &QualifiedIdentity,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<Vec<ChannelDirectoryEntry>> {
        let context = directory.access_context(company.id, sender).await?;
        if !context.membership.is_team() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for channel in directory.channels(company.id).await? {
            if !channel.enabled || !channel.participant_access(context).authorized {
                continue;
            }
            let description = match channel.description.clone() {
                Some(own) => Some(own),
                None => {
                    self.responding_agent_description(&channel, directory)
                        .await?
                }
            };
            entries.push(ChannelDirectoryEntry {
                address: channel.inbound_address(&company.slug, &self.config.app_domain_name),
                name: channel.name.clone(),
                description,
            });
        }
        entries.sort_by(|a, b| a.address.cmp(&b.address));
        Ok(entries)
    }

    /// What the channel's responding agent says it is for, when the channel itself says nothing.
    async fn responding_agent_description(
        &self,
        channel: &Channel,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<Option<String>> {
        let Some(agent_id) = channel
            .agent_ids
            .as_ref()
            .and_then(|ids| ids.first().copied())
        else {
            return Ok(None);
        };
        Ok(directory
            .agent(agent_id)
            .await?
            .and_then(|agent| agent.description))
    }

    /// Reply to every reserved address the message named, reporting whether any answer went out.
    ///
    /// A sender the company does not know gets nothing at all -- not an empty directory, not a "you
    /// are not on the team" note -- so `_help` cannot be used to discover which companies exist.
    /// They fall through to the ordinary unknown-address bounce instead.
    pub(crate) async fn answer_system_addresses(
        &self,
        draft: &InboundDraft,
        routing: &InboundRouting,
        origin: IngressOrigin,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<bool> {
        // Inter-channel traffic is excluded outright: an agent has `list_company_agents` for this,
        // and keeping system addresses off the internal path means they can never appear in a trace
        // or be used to bounce a message between two channels.
        if origin.is_internal() {
            return Ok(false);
        }

        let mut answered = false;
        for (company_slug, system) in routing.system_addresses() {
            let Some(company) = directory.company(company_slug).await? else {
                continue;
            };
            let context = directory.access_context(company.id, &draft.author).await?;
            if !context.membership.is_team() {
                warn!(
                    address = system.local_part(),
                    company = %company.slug,
                    "Ignoring a reserved-address request from a non-member"
                );
                continue;
            }
            let entries = self
                .writable_channel_directory(&company, &draft.author, directory)
                .await?;
            self.send_system_reply(draft, &company, system, entries)
                .await?;
            answered = true;
        }
        Ok(answered)
    }

    /// Queue a reserved-address reply through the same durable delivery state machine as every
    /// other notification. It has no canonical message attribution, but it still must survive a
    /// process crash after ingress accepted the request.
    async fn send_system_reply(
        &self,
        draft: &InboundDraft,
        company: &Company,
        system: SystemAddress,
        entries: Vec<ChannelDirectoryEntry>,
    ) -> AppResult<()> {
        let body = match system {
            SystemAddress::Help => crate::use_cases::thread::format_help_email_body(
                &entries,
                &company.slug,
                &self.config.app_domain_name,
            ),
        };
        let subject = match draft.content.subject().trim() {
            "" => "Mail Agents Help".to_string(),
            value
                if value
                    .get(..3)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re:")) =>
            {
                value.to_string()
            }
            value => format!("Re: {value}"),
        };
        let content = CanonicalContent::parse(subject, body)?;
        let source_key = format!(
            "system:{}:{}:{}",
            company.id,
            system.local_part(),
            draft.message_key
        );
        let delivery = self
            .deliveries
            .compose_standalone(StandaloneDeliveryRequest {
                correlation_id: draft.correlation_id,
                purpose: DeliveryPurpose::Notification,
                source_key,
                content: &content,
                context: DeliveryContext::Email(EmailDeliveryContext {
                    from: EmailAddress::from(format!(
                        "{}@{}.{}",
                        system.local_part(),
                        company.slug,
                        self.config.app_domain_name
                    )),
                    from_name: Some("Mail Agents".to_string()),
                    recipient_to: sender_address(draft),
                    recipients_cc: Vec::new(),
                    threading: EmailThreading::received(
                        email_metadata(draft).map(|metadata| metadata.rfc_message_id.clone()),
                        Vec::new(),
                    ),
                    relay: None,
                }),
            })?;
        self.standalone_deliveries
            .enqueue_standalone_delivery(delivery)
            .await?;
        Ok(())
    }
}

/// The sender's mailbox, for rejection bounces and reserved-address replies.
///
/// Same single-transport assumption as `dispatch::sender_address`: the subject is an address only
/// because email is the only ingress. A bounce to a Slack author is not an email at all, so this
/// gains a transport decision when Slack ingress does, not a fallback address before then.
pub(crate) fn sender_address(draft: &InboundDraft) -> EmailAddress {
    EmailAddress::from(draft.author.subject().as_str())
}

/// The handles on the message that name no platform interface, minus the sender's own.
///
/// Pure, because the adapter already decided which addresses are platform interfaces: the
/// pre-canonical path re-parsed every recipient and looked its company up again here.
fn third_party_handles(
    routing: &InboundRouting,
    sender: &QualifiedIdentity,
) -> Vec<QualifiedIdentity> {
    let mut third_parties: Vec<QualifiedIdentity> = Vec::new();
    for handle in routing.outsiders() {
        if handle == sender || third_parties.contains(handle) {
            continue;
        }
        third_parties.push(handle.clone());
    }
    third_parties
}

fn email_metadata(draft: &InboundDraft) -> Option<&EmailMessageMetadata> {
    draft.extension.email_metadata()
}

/// The RFC ids naming *other* messages in this conversation, for the outreach correlation query
/// that still speaks in them.
fn reference_ids(draft: &InboundDraft) -> Vec<MessageId> {
    email_metadata(draft)
        .map(EmailMessageMetadata::reference_candidates)
        .unwrap_or_default()
}
