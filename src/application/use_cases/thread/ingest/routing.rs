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
        address::AddressTarget,
        channel::Channel,
        company::Company,
        email_message::EmailMessageMetadata,
        message::CanonicalMessageId,
        participant::{PrincipalAccessContext, ThreadPrincipalRole},
        thread::Thread,
        transport::{
            AddressSelector, ChannelBindingId, DeliveryPurpose, QualifiedIdentity, TransportKind,
        },
        value_objects::{ChannelSlug, EmailAddress},
    },
    transport::{
        AddressedRecipient, CanonicalContent, DeliveryContext, EmailDeliveryContext,
        EmailThreading, InboundDraft, InboundRouting, MessageDisposition, PipelineStep,
        RecipientRole, StandaloneDeliveryRequest, SystemAddress, ThreadTarget,
    },
    use_cases::{
        channel::find_similar_channel_slugs,
        thread::{
            AddressDirectoryEntry, BounceInfo, PrincipalRefusal, ThreadUseCases,
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
    pub inbox: super::super::Inbox,
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
}

/// Why a single candidate channel did or didn't make it into the match list.
///
/// An outcome enum rather than a bool plus out-parameters, so the loop that drives it is a `match`
/// that pushes into the right accumulator.
enum ChannelVerdict {
    Accept,
    Unauthorized,
    Reject(IngestRejection),
}

/// How an already-existing thread authorizes -- or refuses -- this sender.
enum ThreadAccess {
    Denied,
    Participant,
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
    origin: IngressOrigin,
}

impl<'a> ThreadSender<'a> {
    pub fn new(handle: &'a QualifiedIdentity, context: PrincipalAccessContext) -> Self {
        Self {
            handle,
            context,
            origin: IngressOrigin::ExternalTransport,
        }
    }

    pub fn address(self) -> &'a str {
        self.handle.subject().as_str()
    }

    fn principal_intent(
        self,
        identity: QualifiedIdentity,
        role: ThreadPrincipalRole,
    ) -> Option<crate::transport::ThreadPrincipalIntent> {
        if self.origin.is_internal() && identity == *self.handle {
            self.context.principal_id.map(|principal| {
                crate::transport::ThreadPrincipalIntent::principal(principal, role)
            })
        } else {
            Some(crate::transport::ThreadPrincipalIntent::new(identity, role))
        }
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
        let mut seen_channels = HashSet::new();
        let mut unauthorized = 0usize;
        let mut undeliverable = UndeliverableSlugs::default();
        let mut matched_company: Option<Company> = None;
        self.load_addressed_namespace(routing, origin, directory)
            .await?;

        for (recipient, selectors) in routing.platform_pipelines() {
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

                let slug = selector.channel().clone();
                let context = match origin.internal_context() {
                    Some(context) => context,
                    None => directory.access_context(company.id, &draft.author).await?,
                };
                let Some(inbox) = self
                    .addressed_inbox(
                        company.id,
                        &slug,
                        recipient,
                        context,
                        directory,
                        &mut undeliverable,
                    )
                    .await?
                else {
                    continue;
                };
                if !seen_channels.insert(inbox.target()) {
                    continue;
                }
                let Some(binding_id) = self.inbound_binding(&company, &inbox).await? else {
                    let available = directory.channels(company.id).await?;
                    undeliverable
                        .unknown(slug.clone(), find_similar_channel_slugs(&slug, &available));
                    continue;
                };
                match self
                    .authorize_channel(&inbox, binding_id, draft, origin, context)
                    .await?
                {
                    ChannelVerdict::Reject(rejection) => return Ok(Err(rejection)),
                    ChannelVerdict::Unauthorized => unauthorized += 1,
                    ChannelVerdict::Accept => {
                        let access = inbox.participant_access(context);
                        candidates.push(ChannelCandidate {
                            company: company.clone(),
                            inbox,
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

        // An undeliverable slug is only ever recorded after its selector matched a company, so a
        // bounce always has one to belong to.
        if let (Some(kind), Some(company)) = (undeliverable.kind(), matched_company.as_ref()) {
            return Ok(Err(self
                .undeliverable_rejection(draft, kind, undeliverable, company, directory)
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
        }))
    }

    async fn addressed_inbox(
        &self,
        company_id: Uuid,
        slug: &ChannelSlug,
        recipient: &AddressedRecipient,
        context: PrincipalAccessContext,
        directory: &mut DirectoryCache<'_>,
        undeliverable: &mut UndeliverableSlugs,
    ) -> AppResult<Option<super::super::Inbox>> {
        let target = directory.address_target(company_id, slug);
        if let Some(AddressTarget::Principal(id)) = target {
            let principal = self
                .participant_persistence
                .principal_by_id(company_id, id)
                .await?;
            let Some(principal) =
                principal.filter(|principal| principal.direct_access(context).authorized)
            else {
                let available = directory.channels(company_id).await?;
                undeliverable.unknown(slug.clone(), find_similar_channel_slugs(slug, &available));
                return Ok(None);
            };
            if recipient.disposition == MessageDisposition::FileOnly {
                undeliverable.principal(slug.clone(), PrincipalRefusal::ContextSuffix);
                return Ok(None);
            }
            return Ok(Some(super::super::Inbox::Direct {
                principal: Box::new(principal),
                slug: slug.clone(),
            }));
        }
        let available = directory.channels(company_id).await?;
        let outcome = addressed_slug(
            target,
            &available,
            slug,
            recipient,
            context.membership.is_team(),
        );
        Ok(record_undeliverable(undeliverable, slug.clone(), outcome)
            .map(|desk| super::super::Inbox::Desk(Box::new(desk))))
    }

    /// The company a selector names: the one it qualified, or -- for an unqualified selector, which
    /// only a trusted internal relay can produce -- the sender's own.
    /// Read every slug this message names in each company it names, one statement per company,
    /// so the whole message is routed against one snapshot of the address namespace.
    async fn load_addressed_namespace(
        &self,
        routing: &InboundRouting,
        origin: IngressOrigin,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<()> {
        let mut slugs: Vec<(Uuid, Vec<ChannelSlug>)> = Vec::new();
        for (_, selectors) in routing.platform_pipelines() {
            for selector in selectors {
                let Some(company) = self.selector_company(selector, origin, directory).await?
                else {
                    continue;
                };
                let slug = selector.channel().clone();
                match slugs.iter_mut().find(|(id, _)| *id == company.id) {
                    Some((_, named)) => named.push(slug),
                    None => slugs.push((company.id, vec![slug])),
                }
            }
        }
        for (company_id, named) in slugs {
            directory.load_addresses(company_id, &named).await?;
        }
        Ok(())
    }

    async fn selector_company(
        &self,
        selector: &AddressSelector,
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
        inbox: &super::super::Inbox,
    ) -> AppResult<Option<ChannelBindingId>> {
        let binding = self
            .binding_persistence
            .active_bindings_for_target(company.id, inbox.target())
            .await?
            .iter()
            .find(|binding| binding.transport == TransportKind::Email)
            .map(|binding| binding.id);
        if binding.is_none() && inbox.desk().is_some() {
            return Err(AppError::Internal(format!(
                "Channel '{}' has no active email interface to receive mail on",
                inbox.slug()
            )));
        }
        Ok(binding)
    }

    /// Phase 3: loop protection, ACL and spam scoring for one candidate channel.
    ///
    /// Every fallible call here feeds an authorization decision, so every one of them propagates: a
    /// directory outage must not read as "this sender is an outsider".
    async fn authorize_channel(
        &self,
        inbox: &super::super::Inbox,
        binding_id: ChannelBindingId,
        draft: &InboundDraft,
        origin: IngressOrigin,
        context: PrincipalAccessContext,
    ) -> AppResult<ChannelVerdict> {
        let sender = ThreadSender::new(&draft.author, context);

        // A channel never appears twice in one relay trace: that is a routing cycle.
        if origin.is_internal() && draft.directives.trace_targets.contains(&inbox.target()) {
            warn!(target = ?inbox.target(), "Refusing a routing cycle");
            return Ok(ChannelVerdict::Reject(IngestRejection::LoopCycle));
        }

        let access = inbox.participant_access(context);

        // Someone who isn't on the channel ACL may still be a party to an existing thread.
        let mut thread_authorized = false;
        if !access.authorized && !origin.is_internal() && inbox.desk().is_some() {
            match self
                .thread_access_for(
                    inbox.desk().expect("desk branch"),
                    binding_id,
                    draft,
                    sender,
                )
                .await?
            {
                ThreadAccess::Denied => {}
                ThreadAccess::Participant => thread_authorized = true,
            }
        }

        if !access.authorized && !thread_authorized {
            warn!(target = ?inbox.target(), "Sender is not authorized for this inbox");
            return Ok(ChannelVerdict::Unauthorized);
        }

        if exceeds_spam_threshold(&draft.policy, access, self.settings.max_spam_score) {
            warn!(target = ?inbox.target(), "Refusing a message over the spam-score threshold");
            return Ok(ChannelVerdict::Reject(IngestRejection::SpamScore));
        }

        Ok(ChannelVerdict::Accept)
    }

    /// Whether an existing thread lets this sender in as a party to it: a member a workflow's
    /// send addressed is one for the thread that send opened (p08 D1.5.4/D1.5.7).
    async fn thread_access_for(
        &self,
        channel: &Channel,
        binding_id: ChannelBindingId,
        draft: &InboundDraft,
        sender: ThreadSender<'_>,
    ) -> AppResult<ThreadAccess> {
        let Some(thread) = self
            .find_ancestor_thread(
                channel.company_id,
                AddressTarget::Desk(channel.id),
                binding_id,
                draft,
            )
            .await?
        else {
            return Ok(ThreadAccess::Denied);
        };
        Ok(if sender.is_party_to(&thread) {
            ThreadAccess::Participant
        } else {
            ThreadAccess::Denied
        })
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
                .get_thread_by_id(candidate.company.id, target_thread_id)
                .await?
            && thread.belongs_to(candidate.company.id, candidate.inbox.target())
        {
            return Ok(Some(thread));
        }

        if let Some(reply_to_id) = draft.directives.reply_to_message_id
            && let Some(thread) = self
                .thread_persistence
                .find_thread_for_message(
                    candidate.company.id,
                    candidate.inbox.target(),
                    reply_to_id,
                )
                .await?
            && thread.belongs_to(candidate.company.id, candidate.inbox.target())
        {
            return Ok(Some(thread));
        }

        if let Some(thread_id) = self
            .correlation_store
            .thread_for_thread_keys(candidate.binding_id, &draft.reply_thread_keys)
            .await?
            && let Some(thread) = self
                .thread_persistence
                .get_thread_by_id(candidate.company.id, thread_id)
                .await?
            && thread.belongs_to(candidate.company.id, candidate.inbox.target())
        {
            return Ok(Some(thread));
        }

        if let Some(thread) = self
            .find_ancestor_thread(
                candidate.company.id,
                candidate.inbox.target(),
                candidate.binding_id,
                draft,
            )
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
            .find_thread_by_thread_index(candidate.company.id, candidate.inbox.target(), &index)
            .await?
            .filter(|thread| thread.belongs_to(candidate.company.id, candidate.inbox.target())))
    }

    /// The thread already holding one of the messages this one names -- including itself, so a
    /// redelivery lands back where its first delivery did -- nearest ancestor first.
    async fn find_ancestor_thread(
        &self,
        company_id: Uuid,
        target: AddressTarget,
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
            .get_thread_by_id(company_id, thread_id)
            .await?
            .filter(|thread| thread.belongs_to(company_id, target)))
    }

    /// Phases 4 and 5: give every authorized channel a thread and decide what it may do.
    pub(crate) async fn prepare_channels(
        &self,
        draft: &InboundDraft,
        routing: &InboundRouting,
        resolved: &ResolvedAddresses,
        origin: IngressOrigin,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<Result<PreparedChannels, IngestRejection>> {
        let sender_context = match origin.internal_context() {
            Some(context) => context,
            None => {
                directory
                    .access_context(resolved.company.id, &draft.author)
                    .await?
            }
        };
        let sender = ThreadSender {
            origin,
            ..ThreadSender::new(&draft.author, sender_context)
        };
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
            match self
                .prepare_channel(
                    candidate,
                    existing,
                    draft,
                    sender,
                    &third_parties,
                    &body_text,
                )
                .await?
            {
                Ok(channel) => channels.push(channel),
                Err(rejection) => return Ok(Err(rejection)),
            }
        }

        Ok(Ok(PreparedChannels {
            channels,
            body_text,
        }))
    }

    /// One authorized channel's thread, new principals and admission facts (p08 D1.3). Facts, not
    /// decisions: whether anything runs belongs to the channel's bindings, after the commit.
    async fn prepare_channel(
        &self,
        candidate: &ChannelCandidate,
        existing: Option<Thread>,
        draft: &InboundDraft,
        sender: ThreadSender<'_>,
        third_parties: &[QualifiedIdentity],
        body_text: &str,
    ) -> AppResult<Result<PreparedChannel, IngestRejection>> {
        let target = match existing {
            Some(thread) => {
                if let Err(rejection) =
                    self.authorize_thread_injection(candidate, &thread, draft, sender)
                {
                    return Ok(Err(rejection));
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
        let pull_third_parties = candidate.access.trusted
            && candidate
                .inbox
                .desk()
                .is_some_and(|desk| desk.add_3rd_party);
        let participant_identities =
            thread_participants(&[], sender.handle, third_parties, true, pull_third_parties);
        let mut principals = Vec::with_capacity(participant_identities.len() + 1);
        for identity in participant_identities {
            if matches!(target, ThreadTarget::Create { .. }) && identity == draft.author {
                principals
                    .extend(sender.principal_intent(identity.clone(), ThreadPrincipalRole::Author));
            }
            principals.extend(sender.principal_intent(identity, ThreadPrincipalRole::Participant));
        }

        if let AddressTarget::Principal(principal) = candidate.inbox.target() {
            principals.push(crate::transport::ThreadPrincipalIntent::principal(
                principal,
                ThreadPrincipalRole::Participant,
            ));
        }

        let mentioned = self.channel_was_mentioned(candidate, body_text);
        let reply_to = self.direct_parent(candidate, &target, draft).await?;
        Ok(Ok(PreparedChannel {
            candidate: candidate.clone(),
            target,
            mentioned,
            reply_to,
            principals: crate::transport::BoundedVec::parse("thread principals", principals)?,
        }))
    }

    /// Refuse a sender who is not a party to the thread, and tell them so with a bounce rather
    /// than dropping the mail silently. Outreach replies no longer widen this (p08 D1.3); a
    /// member a workflow addressed is a party to the thread its send opened (D1.5.4).
    fn authorize_thread_injection(
        &self,
        candidate: &ChannelCandidate,
        thread: &Thread,
        draft: &InboundDraft,
        sender: ThreadSender<'_>,
    ) -> Result<(), IngestRejection> {
        if sender.is_party_to(thread)
            || draft.directives.target_thread_id.is_some()
            || draft.directives.reply_to_message_id.is_some()
        {
            return Ok(());
        }

        warn!(
            thread_id = %thread.id,
            "Refusing an unauthorized thread injection and bouncing it back"
        );
        Err(IngestRejection::ThreadInjection(Box::new(BounceInfo {
            source_message_key: draft.message_key.clone(),
            recipient_to: EmailAddress::from(sender.address()),
            company_id: candidate.company.id,
            company_slug: candidate.company.slug.clone(),
            invalid_slugs: vec![ChannelSlug::from(format!(
                "Thread {} (Unauthorized Sender: {})",
                thread.id,
                sender.address()
            ))],
            disabled_slugs: vec![],
            principals: vec![],
            suggestions: vec![],
            // This bounce fires *because* the sender has no standing on the thread, so it must not
            // hand them the company's channel directory.
            available_channels: vec![],
            original_subject: draft.content.subject().to_string(),
        })))
    }

    /// The message this one directly answers in the channel's existing thread: the trusted
    /// caller's explicit turn, else the provider-mapped `In-Reply-To`. Never References-only
    /// ancestry, which cannot say which turn was answered (p08 D1.3).
    async fn direct_parent(
        &self,
        candidate: &ChannelCandidate,
        target: &ThreadTarget,
        draft: &InboundDraft,
    ) -> AppResult<Option<CanonicalMessageId>> {
        let ThreadTarget::Existing(thread_id) = target else {
            return Ok(None);
        };
        let parent = match draft.directives.reply_to_message_id {
            Some(id) => Some(id),
            None => match &draft.direct_parent_message_key {
                Some(key) => {
                    self.correlation_store
                        .message_for_external_key(candidate.binding_id, key)
                        .await?
                }
                None => None,
            },
        };
        let Some(parent) = parent else {
            return Ok(None);
        };
        Ok(self
            .thread_persistence
            .get_thread_message(*thread_id, parent)
            .await?
            .map(|_| parent))
    }

    /// Whether the body names the channel, an alias, its address, or the handle it was reached at.
    fn channel_was_mentioned(&self, candidate: &ChannelCandidate, body: &str) -> bool {
        let canonical_address = crate::entities::address::address_for(
            candidate.inbox.slug(),
            &candidate.company.slug,
            &self.settings.mail_domain,
        );
        body_mentions_email(body, candidate.handle.subject().as_str())
            || body_mentions_email(body, &canonical_address)
            || body_mentions_slug(body, &candidate.matched_slug)
            || body_mentions_slug(body, candidate.inbox.slug())
            || candidate.inbox.desk().is_some_and(|desk| {
                desk.alias_slugs
                    .iter()
                    .any(|slug| body_mentions_slug(body, slug))
            })
    }

    /// The bounce an undeliverable address earns, with the directory the sender is allowed to see.
    async fn undeliverable_rejection(
        &self,
        draft: &InboundDraft,
        kind: UndeliverableKind,
        undeliverable: UndeliverableSlugs,
        company: &Company,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<IngestRejection> {
        let available_channels = self
            .writable_address_directory(company, &draft.author, directory)
            .await?;
        Ok(IngestRejection::Undeliverable(Box::new(
            UndeliverableReason {
                kind,
                bounce: undeliverable.into_bounce(
                    draft.message_key.clone(),
                    sender_address(draft),
                    company,
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
    pub(crate) async fn writable_address_directory(
        &self,
        company: &Company,
        sender: &QualifiedIdentity,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<Vec<AddressDirectoryEntry>> {
        let context = directory.access_context(company.id, sender).await?;
        if !context.membership.is_team() {
            return Ok(Vec::new());
        }

        // Grouped by the owning department in tree order (a root before its children), desks no
        // department owns last; by address within each (01-departments 04C.3).
        let departments = directory.departments(company.id).await?;
        let order: HashMap<_, (usize, String)> =
            crate::entities::department::department_tree(&departments)
                .into_iter()
                .enumerate()
                .map(|(rank, (_, department))| (department.id, (rank, department.name.clone())))
                .collect();
        let mut entries = Vec::new();
        for channel in directory.channels(company.id).await? {
            if !channel.enabled || !channel.participant_access(context).authorized {
                continue;
            }
            let owner: Option<(usize, String)> = order
                .get(&channel.department_id)
                .map(|(rank, name)| (*rank, name.clone()));
            entries.push((
                owner.as_ref().map_or(usize::MAX, |(rank, _)| *rank),
                AddressDirectoryEntry {
                    target: crate::entities::address::AddressTarget::Desk(channel.id),
                    address: channel.inbound_address(&company.slug, &self.settings.mail_domain),
                    name: channel.name.clone(),
                    description: channel.description.clone(),
                    department: owner.map(|(_, name)| name),
                },
            ));
        }
        entries.sort_by(|a, b| (a.0, &a.1.address).cmp(&(b.0, &b.1.address)));
        let mut entries: Vec<_> = entries.into_iter().map(|(_, entry)| entry).collect();
        if let Some(sender) = context.principal_id
            && let Some(person) = self
                .participant_persistence
                .principal_by_id(company.id, sender)
                .await?
            && let Some(user) = person.user_id
        {
            for inbox in self
                .participant_persistence
                .direct_inboxes(company.id, user, 201)
                .await?
            {
                if !inbox.principal.direct_access(context).authorized {
                    continue;
                }
                let Some(slug) = inbox.slug else {
                    continue;
                };
                entries.push(AddressDirectoryEntry {
                    target: crate::entities::address::AddressTarget::Principal(inbox.principal.id),
                    address: Channel::address_for(&slug, &company.slug, &self.settings.mail_domain),
                    name: inbox.principal.display_label,
                    description: None,
                    department: None,
                });
            }
        }
        Ok(entries)
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
                    company_id = %company.id,
                    "Ignoring a reserved-address request from a non-member"
                );
                continue;
            }
            let entries = self
                .writable_address_directory(&company, &draft.author, directory)
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
        entries: Vec<AddressDirectoryEntry>,
    ) -> AppResult<()> {
        let body = match system {
            SystemAddress::Help => crate::use_cases::thread::format_help_email_body(
                &entries,
                &company.slug,
                &self.settings.mail_domain,
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
                company_id: company.id,
                correlation_id: draft.correlation_id,
                purpose: DeliveryPurpose::Notification,
                source_key,
                content: &content,
                context: DeliveryContext::Email(EmailDeliveryContext {
                    from: EmailAddress::from(format!(
                        "{}@{}.{}",
                        system.local_part(),
                        company.slug,
                        self.settings.mail_domain
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

/// What one addressed slug reaches, decided from what routing has already read.
enum AddressedSlug {
    /// Boxed: a desk dwarfs the other outcomes.
    Desk(Box<Channel>),
    /// Disabled is undeliverable for everyone, so it is decided before the ACL, which rules only
    /// on *this sender's* access.
    DisabledDesk(Uuid),
    /// Nothing the sender may learn about: a slug that names nothing, a desk this list no longer
    /// holds, or a principal's address mailed by a stranger -- who is told exactly what a slug
    /// that names nothing tells them, so a member's address cannot be confirmed by mailing it.
    Unknown(Vec<ChannelSlug>),
}

fn addressed_slug(
    target: Option<AddressTarget>,
    available: &[Channel],
    slug: &ChannelSlug,
    _recipient: &AddressedRecipient,
    _sender_is_team: bool,
) -> AddressedSlug {
    let desk = match target {
        Some(AddressTarget::Desk(id)) => available.iter().find(|desk| desk.id == id),
        Some(AddressTarget::Principal(_)) | None => None,
    };
    match desk {
        Some(desk) if desk.enabled => AddressedSlug::Desk(Box::new(desk.clone())),
        Some(desk) => AddressedSlug::DisabledDesk(desk.id),
        None => AddressedSlug::Unknown(find_similar_channel_slugs(slug, available)),
    }
}

/// The desk an addressed slug reaches, or `None` once its refusal is recorded for the bounce.
fn record_undeliverable(
    undeliverable: &mut UndeliverableSlugs,
    slug: ChannelSlug,
    outcome: AddressedSlug,
) -> Option<Channel> {
    match outcome {
        AddressedSlug::Desk(channel) => return Some(*channel),
        AddressedSlug::DisabledDesk(id) => {
            warn!(channel_id = %id, "Bouncing a message for a disabled channel");
            undeliverable.disabled(slug);
        }
        AddressedSlug::Unknown(suggestions) => undeliverable.unknown(slug, suggestions),
    }
    None
}
