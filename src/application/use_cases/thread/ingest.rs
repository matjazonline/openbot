//! Inbound ingest pipeline.
//!
//! One inbound message walks these phases in order:
//!
//! 1. [`check_inbound_guards`] — I/O-free rejections (identity, SPF/DKIM/DMARC, hops, auto-reply).
//! 2. [`ThreadUseCases::resolve_channel_candidates`] — turn `To`/`Cc` addresses into authorized
//!    company/channel matches, bouncing unknown slugs.
//! 3. [`ThreadUseCases::materialize_channel_match`] — per match, resolve or create the thread and
//!    persist the inbound message.
//! 4. [`ThreadUseCases::finalize_ingest`] — assemble the result and enqueue the durable task.
//!
//! Each phase returns [`ControlFlow::Break`] carrying the rejection it decided on, so the top
//! level reads as the list of phases rather than as nested conditionals.

use std::{collections::HashMap, ops::ControlFlow};

use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        channel::Channel,
        company::Company,
        message::{Message, MessageDirection, MessageRole},
        message_contract::NormalizedInboundMessage,
        outreach::OutreachReplyMatch,
        thread::Thread,
        value_objects::{ChannelSlug, CompanySlug, EmailAddress, MessageId, ThreadIndex},
    },
    services::email_parser::{ParsedEmail, RawInboundPayload},
    use_cases::channel::{find_similar_channel_slugs, parse_recipient_address_pipeline},
};

use super::{
    BounceInfo, BounceSuggestion, ChannelMatch, EmailIngressAdapter, InboundIngestResult,
    InternalChannelSource, MAX_THREAD_MESSAGES_PER_HOUR, RecipientRole, ThreadUseCases,
    durable_ingest_payload,
    support::{
        DirectoryCache, build_prompt_text, check_inbound_guards, parsed_email_from_normalized,
        reference_ids, strip_quoted_history, thread_index_of, thread_lookup_ids,
    },
};

/// A company/channel pair that passed authorization, before its thread exists.
struct CandidateMatch {
    company: Company,
    channel: Channel,
    role: RecipientRole,
    step_index: usize,
    total_steps: usize,
}

/// Everything phase 2 learned about the addresses on the message.
struct ResolvedCandidates {
    matches: Vec<CandidateMatch>,
    /// Channels whose inbound message answers an outreach this platform is already awaiting.
    outreach_by_channel: HashMap<Uuid, OutreachReplyMatch>,
    /// A `+quiet`-style address suffix asked for context-only ingestion.
    address_context_only: bool,
}

/// Why a single candidate channel did or didn't make it into the match list.
enum ChannelVerdict {
    Accept(Box<Option<OutreachReplyMatch>>),
    Unauthorized,
    Reject(Box<InboundIngestResult>),
}

/// How an already-existing thread authorizes (or refuses) this sender.
enum ThreadAccess {
    Denied,
    Participant,
    OutreachReply(Box<OutreachReplyMatch>),
}

struct MaterializedMatch {
    channel_match: ChannelMatch,
    /// Replies that close an outreach are recorded but must not trigger a fresh agent run.
    marks_context_only: bool,
}

impl ThreadUseCases {
    pub async fn ingest_and_save_inbound_message(
        &self,
        raw_payload: RawInboundPayload,
    ) -> AppResult<InboundIngestResult> {
        let norm = EmailIngressAdapter::parse(raw_payload, &self.config);
        self.ingest_normalized_message(norm).await
    }

    #[instrument(skip(self, norm))]
    pub async fn ingest_normalized_message(
        &self,
        norm: NormalizedInboundMessage,
    ) -> AppResult<InboundIngestResult> {
        self.ingest_normalized_message_with_source(norm, None).await
    }

    pub(super) async fn ingest_normalized_message_with_source(
        &self,
        norm: NormalizedInboundMessage,
        internal_source: Option<InternalChannelSource>,
    ) -> AppResult<InboundIngestResult> {
        let mut parsed = parsed_email_from_normalized(&norm);
        info!(
            "Ingesting email Message-ID: {}, From: {}, To: {:?}",
            parsed.message_id, parsed.sender, parsed.recipients_to
        );

        if let Some(rejection) = check_inbound_guards(&parsed, internal_source) {
            return Ok(rejection);
        }

        let mut directory = DirectoryCache::new(self);
        let mut resolved = match self
            .resolve_channel_candidates(&parsed, internal_source, &mut directory)
            .await?
        {
            ControlFlow::Break(rejection) => return Ok(rejection),
            ControlFlow::Continue(resolved) => resolved,
        };

        if let Some(rejection) = self.check_single_company(&resolved.matches) {
            return Ok(rejection);
        }
        if let Some(rejection) = self
            .check_duplicate_message(resolved.matches[0].company.id, &parsed)
            .await?
        {
            return Ok(rejection);
        }

        let mut context_only = resolved.address_context_only;
        let mut channel_matches = Vec::with_capacity(resolved.matches.len());
        for candidate in std::mem::take(&mut resolved.matches) {
            match self
                .materialize_channel_match(
                    candidate,
                    &mut parsed,
                    internal_source,
                    &mut resolved.outreach_by_channel,
                    &mut directory,
                )
                .await?
            {
                ControlFlow::Break(rejection) => return Ok(rejection),
                ControlFlow::Continue(materialized) => {
                    context_only |= materialized.marks_context_only;
                    channel_matches.push(materialized.channel_match);
                }
            }
        }

        self.finalize_ingest(parsed, norm, channel_matches, context_only)
            .await
    }

    /// Phase 2: walk every `To` then `Cc` address, expanding channel pipelines into authorized
    /// matches. Unknown slugs bounce; unauthorized senders are counted so an all-unauthorized
    /// message reports that rather than "channel not found".
    async fn resolve_channel_candidates(
        &self,
        parsed: &ParsedEmail,
        internal_source: Option<InternalChannelSource>,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<ControlFlow<InboundIngestResult, ResolvedCandidates>> {
        let addresses = parsed
            .recipients_to
            .iter()
            .map(|to| (to.clone(), RecipientRole::To))
            .chain(
                parsed
                    .recipients_cc
                    .iter()
                    .map(|cc| (cc.clone(), RecipientRole::Cc)),
            );

        let mut resolved = ResolvedCandidates {
            matches: Vec::new(),
            outreach_by_channel: HashMap::new(),
            address_context_only: false,
        };
        let mut seen_channels = std::collections::HashSet::new();
        let mut unauthorized_count = 0usize;
        let mut invalid_slugs: Vec<ChannelSlug> = Vec::new();
        let mut bounce_suggestions: Vec<BounceSuggestion> = Vec::new();
        let mut matched_company_slug: Option<CompanySlug> = None;

        for (address, role) in addresses {
            let Some((company_slug, channel_slugs, is_address_context_only)) =
                parse_recipient_address_pipeline(&address, &self.config.app_domain_name)
            else {
                continue;
            };
            resolved.address_context_only |= is_address_context_only;
            matched_company_slug = Some(company_slug.clone());

            let Some(company) = directory.company(&company_slug).await? else {
                continue;
            };
            if let Some(source) = internal_source
                && company.id != source.company_id
            {
                return Ok(ControlFlow::Break(InboundIngestResult::rejected(
                    "Cross-company internal channel delivery is not allowed",
                )));
            }

            let available_channels = directory.channels(company.id).await?;
            let total_steps = channel_slugs.len();

            for (step_index, channel_slug) in channel_slugs.into_iter().enumerate() {
                let Some(channel) = available_channels
                    .iter()
                    .find(|c| c.slug.eq_ignore_ascii_case(&channel_slug))
                    .cloned()
                else {
                    let suggestions = find_similar_channel_slugs(&channel_slug, &available_channels);
                    invalid_slugs.push(channel_slug.clone());
                    bounce_suggestions.push(BounceSuggestion {
                        invalid_slug: channel_slug,
                        suggestions,
                    });
                    continue;
                };

                if seen_channels.contains(&channel.id) {
                    continue;
                }

                match self
                    .evaluate_channel_candidate(&channel, parsed, internal_source, directory)
                    .await?
                {
                    ChannelVerdict::Reject(rejection) => {
                        return Ok(ControlFlow::Break(*rejection));
                    }
                    ChannelVerdict::Unauthorized => {
                        unauthorized_count += 1;
                    }
                    ChannelVerdict::Accept(outreach) => {
                        if let Some(matched) = *outreach {
                            resolved.outreach_by_channel.insert(channel.id, matched);
                        }
                        seen_channels.insert(channel.id);
                        resolved.matches.push(CandidateMatch {
                            company: company.clone(),
                            channel,
                            role,
                            step_index,
                            total_steps,
                        });
                    }
                }
            }
        }

        // A pipeline is all-or-nothing: one misspelled hop bounces the whole message so the sender
        // learns about the typo instead of silently losing a step.
        if !invalid_slugs.is_empty() {
            return Ok(ControlFlow::Break(
                InboundIngestResult::rejected_with_bounce(
                    "Channel address not found or misspelled",
                    BounceInfo {
                        recipient_to: EmailAddress::from(parsed.sender.clone()),
                        company_slug: matched_company_slug,
                        invalid_slugs,
                        suggestions: bounce_suggestions,
                        original_subject: parsed.subject.clone(),
                    },
                ),
            ));
        }

        if resolved.matches.is_empty() {
            if unauthorized_count > 0 {
                return Ok(ControlFlow::Break(InboundIngestResult::rejected(
                    "Sender unauthorized for channel",
                )));
            }
            warn!(
                "No valid company/channel matched for To: {:?}, CC: {:?}",
                parsed.recipients_to, parsed.recipients_cc
            );
            return Ok(ControlFlow::Break(InboundIngestResult::rejected(
                "Company or Channel not found",
            )));
        }

        Ok(ControlFlow::Continue(resolved))
    }

    /// Loop protection, ACL and spam scoring for one candidate channel.
    async fn evaluate_channel_candidate(
        &self,
        channel: &Channel,
        parsed: &ParsedEmail,
        internal_source: Option<InternalChannelSource>,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<ChannelVerdict> {
        let is_inter_channel = internal_source.is_some();
        let sender = parsed.sender.trim();
        let mut outreach_match = None;

        // A channel may appear twice in one trace only when this is the correlated response to an
        // outreach it is already waiting on; anything else is a routing cycle.
        if is_inter_channel && parsed.trace_channels.contains(&channel.id) {
            match self.match_outreach_reply(channel, parsed, sender).await? {
                Some(matched) => outreach_match = Some(matched),
                None => {
                    warn!(
                        "Inter-channel cycle detected for channel '{}' in Message-ID: {}",
                        channel.id, parsed.message_id
                    );
                    return Ok(ChannelVerdict::Reject(Box::new(
                        InboundIngestResult::rejected("Inter-channel loop cycle detected"),
                    )));
                }
            }
        }

        let is_team_member = directory.is_team_member(channel.company_id, sender).await?;
        let access = channel.participant_access(sender, is_team_member);

        // Someone who isn't on the channel ACL may still be a party to an existing thread.
        let mut thread_authorized = false;
        if !access.authorized && !is_inter_channel {
            match self.thread_access_for(channel, parsed, sender).await? {
                ThreadAccess::Denied => {}
                ThreadAccess::Participant => thread_authorized = true,
                ThreadAccess::OutreachReply(matched) => {
                    thread_authorized = true;
                    outreach_match = Some(*matched);
                }
            }
        }

        if !access.authorized && !thread_authorized && !is_inter_channel {
            warn!(
                "Sender '{}' unauthorized for channel '{}'",
                parsed.sender, channel.slug
            );
            return Ok(ChannelVerdict::Unauthorized);
        }

        if !access.trusted
            && let Some(score) = parsed.spam_score
            && score >= self.config.max_spam_score
        {
            warn!(
                "Message-ID '{}' rejected due to high spam score ({:.2} >= {:.2})",
                parsed.message_id, score, self.config.max_spam_score
            );
            return Ok(ChannelVerdict::Reject(Box::new(
                InboundIngestResult::rejected("Spam score threshold exceeded"),
            )));
        }

        Ok(ChannelVerdict::Accept(Box::new(outreach_match)))
    }

    /// Locate the thread this message belongs to: by referenced Message-IDs first, then by the
    /// Outlook-style `Thread-Index` header.
    async fn find_existing_thread(
        &self,
        channel_id: Uuid,
        lookup_ids: &[MessageId],
        thread_index: Option<ThreadIndex>,
    ) -> AppResult<Option<Thread>> {
        if !lookup_ids.is_empty()
            && let Some(thread) = self
                .thread_persistence
                .find_thread_by_message_ids(channel_id, lookup_ids)
                .await?
        {
            return Ok(Some(thread));
        }
        let Some(index) = thread_index else {
            return Ok(None);
        };
        self.thread_persistence
            .find_thread_by_thread_index(channel_id, &index)
            .await
    }

    /// Does this message answer an outreach the channel is awaiting in its referenced thread?
    async fn match_outreach_reply(
        &self,
        channel: &Channel,
        parsed: &ParsedEmail,
        sender: &str,
    ) -> AppResult<Option<OutreachReplyMatch>> {
        let references = reference_ids(parsed);
        if references.is_empty() {
            return Ok(None);
        }
        let Some(thread) = self
            .thread_persistence
            .find_thread_by_message_ids(channel.id, &references)
            .await?
        else {
            return Ok(None);
        };
        self.task_persistence
            .find_correlated_outreach_reply(
                channel.company_id,
                channel.id,
                thread.id,
                sender,
                &references,
            )
            .await
    }

    /// Whether an existing thread lets this sender in, either as a listed participant or as the
    /// target of an outreach awaiting their reply.
    async fn thread_access_for(
        &self,
        channel: &Channel,
        parsed: &ParsedEmail,
        sender: &str,
    ) -> AppResult<ThreadAccess> {
        let references = reference_ids(parsed);
        let Some(thread) = self
            .find_existing_thread(channel.id, &references, thread_index_of(parsed))
            .await?
        else {
            return Ok(ThreadAccess::Denied);
        };
        if thread.channel_id != channel.id {
            return Ok(ThreadAccess::Denied);
        }
        if thread
            .participant_emails
            .iter()
            .any(|p| p.trim().eq_ignore_ascii_case(sender))
        {
            return Ok(ThreadAccess::Participant);
        }
        let matched = self
            .task_persistence
            .find_correlated_outreach_reply(
                channel.company_id,
                channel.id,
                thread.id,
                sender,
                &references,
            )
            .await?;
        Ok(match matched {
            Some(matched) => ThreadAccess::OutreachReply(Box::new(matched)),
            None => ThreadAccess::Denied,
        })
    }

    fn check_single_company(&self, matches: &[CandidateMatch]) -> Option<InboundIngestResult> {
        let first_company_id = matches[0].company.id;
        matches
            .iter()
            .any(|candidate| candidate.company.id != first_company_id)
            .then(|| {
                InboundIngestResult::rejected("A channel pipeline cannot span multiple companies")
            })
    }

    async fn check_duplicate_message(
        &self,
        company_id: Uuid,
        parsed: &ParsedEmail,
    ) -> AppResult<Option<InboundIngestResult>> {
        let existing = self
            .task_persistence
            .get_task_by_source_message_id(company_id, &parsed.message_id)
            .await?;
        if existing.is_none() {
            return Ok(None);
        }
        warn!(
            "Duplicate Message-ID '{}' already processed for company {}",
            parsed.message_id, company_id
        );
        Ok(Some(InboundIngestResult::rejected(
            "Duplicate Message-ID already processed",
        )))
    }

    /// Phase 3: give one authorized candidate a thread and persist its inbound message.
    async fn materialize_channel_match(
        &self,
        candidate: CandidateMatch,
        parsed: &mut ParsedEmail,
        internal_source: Option<InternalChannelSource>,
        outreach_by_channel: &mut HashMap<Uuid, OutreachReplyMatch>,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<ControlFlow<InboundIngestResult, MaterializedMatch>> {
        let CandidateMatch {
            company,
            channel,
            role,
            step_index,
            total_steps,
        } = candidate;
        let sender = parsed.sender.trim().to_string();

        let existing_thread = self
            .find_existing_thread(
                channel.id,
                &thread_lookup_ids(parsed),
                thread_index_of(parsed),
            )
            .await?
            .filter(|thread| thread.channel_id == channel.id);

        if let Some(ref thread) = existing_thread
            && let ControlFlow::Break(rejection) = self
                .authorize_thread_injection(
                    &company,
                    &channel,
                    thread,
                    parsed,
                    &sender,
                    outreach_by_channel,
                )
                .await?
        {
            return Ok(ControlFlow::Break(rejection));
        }

        let is_team_member = directory.is_team_member(channel.company_id, &sender).await?;
        let access = channel.participant_access(&sender, is_team_member);

        let third_party_recipients = if access.trusted {
            self.collect_third_party_recipients(parsed, &sender, directory)
                .await?
        } else {
            Vec::new()
        };

        let thread = match existing_thread {
            Some(thread) => thread,
            None => {
                let mut participants = vec![EmailAddress::from(parsed.sender.clone())];
                for recipient in &third_party_recipients {
                    if !participants.iter().any(|p| p.eq_ignore_ascii_case(recipient)) {
                        participants.push(recipient.clone());
                    }
                }
                self.thread_persistence
                    .create_thread(channel.id, &parsed.subject, &participants)
                    .await?
            }
        };

        let recent_count = self
            .thread_persistence
            .count_recent_messages(thread.id, 3600)
            .await?;
        if recent_count >= MAX_THREAD_MESSAGES_PER_HOUR {
            warn!(
                "Thread turn limit ({}/hr) exceeded for thread_id {}, dropping response to prevent ping-pong loop",
                recent_count, thread.id
            );
            return Ok(ControlFlow::Break(InboundIngestResult::rejected(
                "Thread turn limit exceeded",
            )));
        }

        let thread = self
            .add_thread_participants(
                thread,
                parsed,
                &third_party_recipients,
                access.trusted,
                !outreach_by_channel.contains_key(&channel.id),
            )
            .await?;

        let history = self
            .thread_persistence
            .list_messages_by_thread_id(thread.id)
            .await?;
        let clean_text_body = strip_quoted_history(parsed, &history);
        if clean_text_body != parsed.clean_text_body {
            parsed.clean_text_body = clean_text_body.clone();
            parsed.prompt_text = build_prompt_text(&clean_text_body, &parsed.attachments);
        }

        let inbound_message = self
            .thread_persistence
            .create_message(&build_inbound_message(
                &thread,
                parsed,
                clean_text_body,
                internal_source.is_some(),
            ))
            .await?;

        // Recording the reply satisfies the awaiting outreach; the agent must not be re-run for it.
        let marks_context_only = match outreach_by_channel.get(&channel.id) {
            Some(matched) => {
                self.task_persistence
                    .record_outreach_reply(matched, inbound_message.id)
                    .await?;
                true
            }
            None => false,
        };

        Ok(ControlFlow::Continue(MaterializedMatch {
            channel_match: ChannelMatch {
                company,
                channel,
                thread,
                inbound_message,
                recipient_role: role,
                step_index,
                total_steps,
            },
            marks_context_only,
        }))
    }

    /// Refuse a sender who is neither on the thread nor answering one of its outreaches, and tell
    /// them so with a bounce rather than dropping the mail silently.
    async fn authorize_thread_injection(
        &self,
        company: &Company,
        channel: &Channel,
        thread: &Thread,
        parsed: &ParsedEmail,
        sender: &str,
        outreach_by_channel: &mut HashMap<Uuid, OutreachReplyMatch>,
    ) -> AppResult<ControlFlow<InboundIngestResult>> {
        let is_thread_participant = thread
            .participant_emails
            .iter()
            .any(|p| p.trim().eq_ignore_ascii_case(sender));

        if !is_thread_participant && !outreach_by_channel.contains_key(&channel.id) {
            let references = reference_ids(parsed);
            if let Some(matched) = self
                .task_persistence
                .find_correlated_outreach_reply(
                    channel.company_id,
                    channel.id,
                    thread.id,
                    sender,
                    &references,
                )
                .await?
            {
                outreach_by_channel.insert(channel.id, matched);
            }
        }

        if is_thread_participant || outreach_by_channel.contains_key(&channel.id) {
            return Ok(ControlFlow::Continue(()));
        }

        warn!(
            "Unauthorized thread injection attempt by '{}' for thread '{}'. Triggering bounce notification.",
            sender, thread.id
        );
        Ok(ControlFlow::Break(
            InboundIngestResult::rejected_with_bounce(
                "Sender is not an authorized participant or delegation target for this thread",
                BounceInfo {
                    recipient_to: EmailAddress::from(parsed.sender.clone()),
                    company_slug: Some(company.slug.clone()),
                    invalid_slugs: vec![ChannelSlug::from(format!(
                        "Thread {} (Unauthorized Sender: {})",
                        thread.id, sender
                    ))],
                    suggestions: vec![],
                    original_subject: parsed.subject.clone(),
                },
            ),
        ))
    }

    /// Human recipients on the message who aren't platform channel addresses; a trusted sender can
    /// pull these into the thread so replies reach everyone.
    async fn collect_third_party_recipients(
        &self,
        parsed: &ParsedEmail,
        sender: &str,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<Vec<EmailAddress>> {
        let mut third_party: Vec<EmailAddress> = Vec::new();
        for address in parsed.recipients_to.iter().chain(parsed.recipients_cc.iter()) {
            let address = address.trim();
            if address.is_empty() || address.eq_ignore_ascii_case(sender) {
                continue;
            }
            if self.is_platform_channel_address(address, directory).await? {
                continue;
            }
            if !third_party
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(address))
            {
                third_party.push(EmailAddress::from(address.to_string()));
            }
        }
        Ok(third_party)
    }

    async fn is_platform_channel_address(
        &self,
        address: &str,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<bool> {
        let Some((company_slug, _, _)) =
            parse_recipient_address_pipeline(address, &self.config.app_domain_name)
        else {
            return Ok(false);
        };
        Ok(directory.company(&company_slug).await?.is_some())
    }

    async fn add_thread_participants(
        &self,
        thread: Thread,
        parsed: &ParsedEmail,
        third_party_recipients: &[EmailAddress],
        sender_is_trusted: bool,
        add_sender: bool,
    ) -> AppResult<Thread> {
        let mut participants = thread.participant_emails.clone();
        let mut changed = false;

        if add_sender
            && !participants
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&parsed.sender))
        {
            participants.push(EmailAddress::from(parsed.sender.clone()));
            changed = true;
        }
        if sender_is_trusted {
            for recipient in third_party_recipients {
                if !participants.iter().any(|p| p.eq_ignore_ascii_case(recipient)) {
                    participants.push(recipient.clone());
                    changed = true;
                }
            }
        }

        if !changed {
            return Ok(thread);
        }
        self.thread_persistence
            .update_thread_participants(thread.id, &participants)
            .await
    }

    /// Phase 4: assemble the ingest result and, unless this was context-only, enqueue the durable
    /// task that will run the agent.
    async fn finalize_ingest(
        &self,
        mut parsed: ParsedEmail,
        norm: NormalizedInboundMessage,
        channel_matches: Vec<ChannelMatch>,
        context_only: bool,
    ) -> AppResult<InboundIngestResult> {
        let first = channel_matches[0].clone();
        let context_only = context_only || parsed.is_context_only || norm.is_context_only;
        parsed.is_context_only = context_only;

        let mut norm = norm;
        norm.is_context_only = context_only;

        let mut result = InboundIngestResult {
            accepted: true,
            reason: None,
            thread: Some(first.thread.clone()),
            inbound_message: Some(first.inbound_message.clone()),
            company: Some(first.company.clone()),
            channel: Some(first.channel.clone()),
            parsed_email: Some(parsed),
            normalized_message: Some(norm),
            task_id: None,
            channel_matches,
            bounce_info: None,
        };

        if context_only {
            info!(
                "Ingested message ID '{}' for thread '{}' in context-only / quiet mode. Skipping background task creation and agent execution.",
                first.inbound_message.message_id, first.thread.id
            );
            return Ok(result);
        }

        // Durable task: survives a crash between ingest and agent execution.
        let task = self
            .task_persistence
            .enqueue_task(
                first.company.id,
                first.channel.id,
                Some(first.thread.id),
                "email_agent_dispatch",
                durable_ingest_payload(&result),
            )
            .await?;
        result.task_id = Some(task.id);
        Ok(result)
    }
}

fn build_inbound_message(
    thread: &Thread,
    parsed: &ParsedEmail,
    clean_text_body: String,
    is_inter_channel: bool,
) -> Message {
    Message {
        id: Uuid::new_v4(),
        thread_id: thread.id,
        message_id: MessageId::from(parsed.message_id.clone()),
        in_reply_to: parsed.in_reply_to.clone().map(MessageId::from),
        references_list: parsed
            .references
            .iter()
            .cloned()
            .map(MessageId::from)
            .collect(),
        sender: EmailAddress::from(parsed.sender.clone()),
        recipients_to: parsed
            .recipients_to
            .iter()
            .cloned()
            .map(EmailAddress::from)
            .collect(),
        recipients_cc: parsed
            .recipients_cc
            .iter()
            .cloned()
            .map(EmailAddress::from)
            .collect(),
        subject: parsed.subject.clone(),
        clean_text_body,
        raw_text_body: parsed.raw_text_body.clone(),
        raw_html_body: parsed.raw_html_body.clone(),
        attachments: (!parsed.attachments.is_empty()).then(|| parsed.attachments.clone()),
        direction: MessageDirection::Inbound,
        // An inter-channel message is another agent talking, not a human.
        role: if is_inter_channel {
            MessageRole::Agent
        } else {
            MessageRole::Human
        },
        thread_index: parsed.thread_index.clone().map(ThreadIndex::from),
        created_at: chrono::Utc::now().naive_utc(),
    }
}
