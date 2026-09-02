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

use std::{
    collections::HashMap,
    ops::ControlFlow,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

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
        participant::PrincipalAccessContext,
        task::NewTask,
        thread::Thread,
        transport::PrincipalId,
        value_objects::{
            ChannelSlug, CompanySlug, EmailAddress, MessageId, ThreadIndex, ThreadIndexParseError,
        },
    },
    services::email_parser::{ParsedEmail, RawInboundPayload},
    use_cases::channel::{
        SystemAddress, find_similar_channel_slugs, parse_platform_address,
        parse_recipient_address_pipeline,
    },
};

use super::{
    BounceInfo, BounceSuggestion, ChannelDirectoryEntry, ChannelMatch, EmailIngressAdapter,
    InboundIngestResult, InboundOrigin, InternalChannelSource, MAX_THREAD_MESSAGES_PER_HOUR,
    RecipientRole, ThreadUseCases, durable_ingest_payload, format_help_email_body,
    support::{
        DirectoryCache, body_mentions_email, body_mentions_slug, build_prompt_text,
        check_inbound_guards, parsed_email_from_normalized, reference_ids, strip_quoted_history,
        thread_index_of, thread_lookup_ids,
    },
};

/// The one task type this pipeline produces.
const AGENT_DISPATCH_TASK: &str = "email_agent_dispatch";
const THREAD_INDEX_WARNING_INTERVAL_SECS: u64 = 60;
static LAST_THREAD_INDEX_WARNING_WINDOWS: [AtomicU64; 6] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

fn should_warn_about_thread_index(error: ThreadIndexParseError) -> bool {
    let window = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / THREAD_INDEX_WARNING_INTERVAL_SECS
        + 1;
    LAST_THREAD_INDEX_WARNING_WINDOWS[error.warning_slot()].swap(window, Ordering::Relaxed)
        != window
}

/// Why a message that only named a reserved address produced no thread.
///
/// `SmtpServer::ingest_status` matches this string, so the two must move together -- see the note
/// there about `reason` wanting to be an enum.
pub const SYSTEM_ADDRESS_ANSWERED: &str = "System address answered";

/// Which reserved address, if any, this recipient names.
///
/// Reads the raw local part rather than a parsed channel slug: `strip_context_suffix_from_slug`
/// would swallow a future `_msg` whole, since a bare reserved suffix already parses to an empty
/// slug.
fn system_address_of(address: &str, app_domain: &str) -> Option<(CompanySlug, SystemAddress)> {
    let (company_slug, local_part) = parse_platform_address(address, app_domain)?;
    SystemAddress::parse(&local_part).map(|system| (company_slug, system))
}

/// Whether the agent's reply to this message should actually be sent.
///
/// This is a property of the *message*, not of the run, which is why it has to survive into the
/// durable payload: the worker that eventually answers is a different process from the one that
/// took the message in, and it has no other way to know what was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyDelivery {
    /// Answer for real. Every message that arrived over SMTP or a webhook, and every internal hop.
    Send,
    /// Run the agent but keep the answer in the mailbox — a send the user marked as a test.
    InAppOnly,
}

impl ReplyDelivery {
    fn sends_email(self) -> bool {
        matches!(self, ReplyDelivery::Send)
    }
}

/// A company/channel pair that passed authorization, before its thread exists.
struct CandidateMatch {
    company: Company,
    channel: Channel,
    /// The slug the envelope actually named — an alias, or the channel's canonical slug.
    matched_slug: ChannelSlug,
    /// The complete To/Cc address that produced this match. Pipeline steps share this address.
    delivery_address: EmailAddress,
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

/// The slugs on the To/Cc line that cannot be delivered to, and why.
///
/// Owning both cases in one place keeps the bounce decision — reason string and `BounceInfo` —
/// out of the middle of the address loop.
#[derive(Default)]
struct UndeliverableSlugs {
    /// Slugs that name no channel in the company.
    invalid: Vec<ChannelSlug>,
    /// Slugs that name a real channel whose owner has switched it off.
    disabled: Vec<ChannelSlug>,
    /// Spelling hints, one per entry in `invalid`.
    suggestions: Vec<BounceSuggestion>,
}

impl UndeliverableSlugs {
    fn unknown(&mut self, slug: ChannelSlug, available: &[Channel]) {
        let suggestions = find_similar_channel_slugs(&slug, available);
        self.invalid.push(slug.clone());
        self.suggestions.push(BounceSuggestion {
            invalid_slug: slug,
            suggestions,
        });
    }

    fn disabled(&mut self, slug: ChannelSlug) {
        self.disabled.push(slug);
    }

    /// Why the message bounces, or `None` when every address was deliverable.
    ///
    /// A misspelling is reported ahead of a disabled channel: the sender can act on a typo, and
    /// the bounce body lists both sets regardless.
    fn reason(&self) -> Option<&'static str> {
        if !self.invalid.is_empty() {
            Some("Channel address not found or misspelled")
        } else if !self.disabled.is_empty() {
            Some("Channel is disabled")
        } else {
            None
        }
    }

    fn into_bounce(
        self,
        parsed: &ParsedEmail,
        company_slug: Option<CompanySlug>,
        available_channels: Vec<ChannelDirectoryEntry>,
    ) -> BounceInfo {
        BounceInfo {
            recipient_to: EmailAddress::from(parsed.sender.clone()),
            company_slug,
            invalid_slugs: self.invalid,
            disabled_slugs: self.disabled,
            suggestions: self.suggestions,
            available_channels,
            original_subject: parsed.subject.clone(),
        }
    }
}

/// Why a single candidate channel did or didn't make it into the match list.
enum ChannelVerdict {
    Accept(Box<Option<OutreachReplyMatch>>),
    Unauthorized,
    Reject(Box<InboundIngestResult>),
}

/// One inbound sender, in both the terms the thread rules need.
///
/// `principal_id` is the stable actor that decides participation; `address` is the transport
/// handle that outreach correlation and the bounce still speak in. Carrying them together stops a
/// caller passing one thread's principal alongside another's address.
#[derive(Clone, Copy)]
struct ThreadSender<'a> {
    address: &'a str,
    principal_id: Option<PrincipalId>,
}

impl<'a> ThreadSender<'a> {
    fn new(address: &'a str, context: PrincipalAccessContext) -> Self {
        Self {
            address,
            principal_id: context.principal_id,
        }
    }

    /// Whether this sender is already a party to the thread. A handle that resolved to no
    /// principal is nobody, and is never a party to anything.
    fn is_party_to(self, thread: &Thread) -> bool {
        self.principal_id
            .is_some_and(|principal_id| thread.contains_principal(principal_id))
    }
}

/// How an already-existing thread authorizes (or refuses) this sender.
enum ThreadAccess {
    Denied,
    Participant,
    OutreachReply(Box<OutreachReplyMatch>),
}

struct MaterializedMatch {
    channel_match: ChannelMatch,
    executes_agent: bool,
    /// Replies that close an outreach are recorded but must not trigger a fresh agent run.
    marks_context_only: bool,
}

impl ThreadUseCases {
    pub(super) fn canonicalize_thread_index(&self, parsed: &mut ParsedEmail) {
        let raw_length = parsed.thread_index.as_ref().map_or(0, |index| index.len());
        match thread_index_of(parsed) {
            Ok(index) => parsed.thread_index = index,
            Err(error) => {
                parsed.thread_index = None;
                let reason = error.metric_reason();
                if let Some(monitoring) = self.monitoring.as_ref() {
                    monitoring.increment_counter(
                        "thread_index_rejected_total",
                        1,
                        &[("reason", reason)],
                    );
                    monitoring.record_gauge(
                        "thread_index_rejected_encoded_bytes",
                        raw_length as f64,
                        &[("reason", reason)],
                    );
                }
                if should_warn_about_thread_index(error) {
                    warn!(
                        target: "mail_agents::thread_index",
                        reason,
                        encoded_length = raw_length,
                        "Ignoring malformed Thread-Index header"
                    );
                }
            }
        }
    }

    pub async fn ingest_and_save_inbound_message(
        &self,
        raw_payload: RawInboundPayload,
    ) -> AppResult<InboundIngestResult> {
        let norm =
            EmailIngressAdapter::parse_and_store(raw_payload, &self.config, self.file_storage())
                .await?;
        self.ingest_normalized_message(norm).await
    }

    #[instrument(skip(self, norm))]
    pub async fn ingest_normalized_message(
        &self,
        norm: NormalizedInboundMessage,
    ) -> AppResult<InboundIngestResult> {
        self.ingest_normalized_message_with_source(
            norm,
            None,
            InboundOrigin::ExternalEmail,
            ReplyDelivery::Send,
        )
        .await
    }

    /// Ingest a message composed in the mailbox, which may ask for its reply to stay in-app.
    pub(super) async fn ingest_composed_message(
        &self,
        raw_payload: RawInboundPayload,
        delivery: ReplyDelivery,
    ) -> AppResult<InboundIngestResult> {
        let norm =
            EmailIngressAdapter::parse_and_store(raw_payload, &self.config, self.file_storage())
                .await?;
        self.ingest_normalized_message_with_source(
            norm,
            None,
            InboundOrigin::AuthenticatedApplication,
            delivery,
        )
        .await
    }

    pub(super) async fn ingest_normalized_message_with_source(
        &self,
        norm: NormalizedInboundMessage,
        internal_source: Option<InternalChannelSource>,
        origin: InboundOrigin,
        delivery: ReplyDelivery,
    ) -> AppResult<InboundIngestResult> {
        let mut parsed = parsed_email_from_normalized(&norm);
        self.canonicalize_thread_index(&mut parsed);
        info!(
            "Ingesting email Message-ID: {}, From: {}, To: {:?}",
            parsed.message_id, parsed.sender, parsed.recipients_to
        );

        if let Some(rejection) = check_inbound_guards(&parsed, internal_source, origin) {
            return Ok(rejection);
        }

        let mut directory = DirectoryCache::new(self);

        if let Some(answered) = self
            .answer_system_addresses(&parsed, internal_source, &mut directory)
            .await?
        {
            return Ok(answered);
        }

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
        let mut executable_matches = Vec::new();
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
                    if materialized.executes_agent {
                        executable_matches.push(materialized.channel_match.clone());
                    }
                    channel_matches.push(materialized.channel_match);
                }
            }
        }

        self.finalize_ingest(
            parsed,
            norm,
            channel_matches,
            executable_matches,
            context_only,
            delivery,
        )
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
        let mut undeliverable = UndeliverableSlugs::default();
        let mut matched_company: Option<Company> = None;

        for (address, role) in addresses {
            // Handled by `answer_system_addresses`, whether or not it chose to reply.
            //
            // Skipping is what lets someone CC `_help` on a real message: treated as a channel slug
            // it would be "not found" and bounce the whole thing. It also means an unanswered
            // reserved address drops silently instead of bouncing -- deliberate, because a bounce
            // would confirm the company to a stranger, and `find_similar_channel_slugs("_help", ..)`
            // could offer them a real channel named `help` as a suggestion.
            if system_address_of(&address, &self.config.app_domain_name).is_some() {
                continue;
            }
            let Some((company_slug, channel_slugs, is_address_context_only)) =
                parse_recipient_address_pipeline(&address, &self.config.app_domain_name)
            else {
                continue;
            };
            resolved.address_context_only |= is_address_context_only;

            let Some(company) = directory.company(&company_slug).await? else {
                continue;
            };
            matched_company = Some(company.clone());
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
                    .find(|c| c.matches_slug(&channel_slug))
                    .cloned()
                else {
                    undeliverable.unknown(channel_slug, &available_channels);
                    continue;
                };

                // Disabled is undeliverable for everyone, so it is decided here rather than in
                // `evaluate_channel_candidate`, which only rules on *this sender's* access.
                if !channel.enabled {
                    warn!(
                        "Channel '{}' is disabled; bouncing message {}",
                        channel.slug, parsed.message_id
                    );
                    undeliverable.disabled(channel_slug);
                    continue;
                }

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
                            matched_slug: channel_slug,
                            delivery_address: EmailAddress::from(address.clone()),
                            role,
                            step_index,
                            total_steps,
                        });
                    }
                }
            }
        }

        // A pipeline is all-or-nothing: one undeliverable hop bounces the whole message so the
        // sender learns about the typo (or the closed channel) instead of silently losing a step.
        if let Some(reason) = undeliverable.reason() {
            let (company_slug, available_channels) = match matched_company {
                Some(ref company) => (
                    Some(company.slug.clone()),
                    self.writable_channel_directory(company, parsed.sender.trim(), directory)
                        .await?,
                ),
                None => (None, Vec::new()),
            };
            return Ok(ControlFlow::Break(
                InboundIngestResult::rejected_with_bounce(
                    reason,
                    undeliverable.into_bounce(parsed, company_slug, available_channels),
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

    /// Phase 1.5: answer any reserved `_` address the message was sent to.
    ///
    /// Returns `Some` only when there is nothing left to route -- the message named a system
    /// address and no channel -- so a `_help` CC alongside a real recipient still reaches it.
    ///
    /// Inter-channel traffic is excluded outright: an agent has `list_company_agents` for this, and
    /// keeping system addresses off the internal path means they can never appear in a trace or be
    /// used to bounce a message between two channels.
    async fn answer_system_addresses(
        &self,
        parsed: &ParsedEmail,
        internal_source: Option<InternalChannelSource>,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<Option<InboundIngestResult>> {
        if internal_source.is_some() {
            return Ok(None);
        }

        let app_domain = &self.config.app_domain_name;
        let mut answered_any = false;
        let mut routable_recipients = 0usize;

        for address in parsed
            .recipients_to
            .iter()
            .chain(parsed.recipients_cc.iter())
        {
            match system_address_of(address, app_domain) {
                Some((company_slug, system)) => {
                    if self
                        .answer_one_system_address(parsed, &company_slug, system, directory)
                        .await?
                    {
                        answered_any = true;
                    }
                }
                None => {
                    if parse_recipient_address_pipeline(address, app_domain).is_some() {
                        routable_recipients += 1;
                    }
                }
            }
        }

        if answered_any && routable_recipients == 0 {
            return Ok(Some(InboundIngestResult::rejected(SYSTEM_ADDRESS_ANSWERED)));
        }
        Ok(None)
    }

    /// Reply to one reserved address, reporting whether an answer actually went out.
    ///
    /// A sender the company does not know gets nothing at all -- not an empty directory, not a
    /// "you are not on the team" note -- so `_help` cannot be used to discover which companies
    /// exist. They fall through to the ordinary unknown-address bounce instead.
    async fn answer_one_system_address(
        &self,
        parsed: &ParsedEmail,
        company_slug: &CompanySlug,
        system: SystemAddress,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<bool> {
        let Some(company) = directory.company(company_slug).await? else {
            return Ok(false);
        };

        let sender = parsed.sender.trim();
        let context = directory.access_context(company.id, sender).await?;
        if !context.membership.is_team() {
            warn!(
                "Ignoring {} request from non-member '{}' for company '{}'",
                system.local_part(),
                sender,
                company.slug
            );
            return Ok(false);
        }

        let body = match system {
            SystemAddress::Help => {
                let entries = self
                    .writable_channel_directory(&company, sender, directory)
                    .await?;
                format_help_email_body(&entries, &company.slug, &self.config.app_domain_name)
            }
        };

        // Fire and forget, like `handle_bounce_dispatch`: a relay that is down must not turn a
        // help request into a retried task.
        if let Err(error) = self
            .mail_dispatcher
            .send_system_reply(
                &company.slug,
                system.local_part(),
                &EmailAddress::from(parsed.sender.clone()),
                &parsed.subject,
                Some(MessageId::from(parsed.message_id.clone())),
                &body,
            )
            .await
        {
            warn!(
                "Could not deliver the {} reply to '{}': {}",
                system.local_part(),
                sender,
                error
            );
        }
        Ok(true)
    }

    /// The channels this sender could have written to, for the bounce body.
    ///
    /// Empty unless the sender is on the company's team: someone outside it who guesses at a
    /// channel name must not learn the company's directory from the bounce they earn.
    ///
    /// Eligibility is decided by [`Channel::participant_access`] — the same predicate the delivery
    /// path applies a few lines up — so the list can never advertise an address that would bounce
    /// in turn.
    async fn writable_channel_directory(
        &self,
        company: &Company,
        sender: &str,
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
    ///
    /// The same fallback the `list_company_agents` tool applies, so a channel created before
    /// descriptions existed still explains itself.
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

        let context = directory.access_context(channel.company_id, sender).await?;
        let access = channel.participant_access(context);

        // Someone who isn't on the channel ACL may still be a party to an existing thread.
        let mut thread_authorized = false;
        if !access.authorized && !is_inter_channel {
            match self
                .thread_access_for(channel, parsed, ThreadSender::new(sender, context))
                .await?
            {
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

    /// Whether an existing thread lets this sender in, either as a party to it or as the target
    /// of an outreach awaiting their reply.
    async fn thread_access_for(
        &self,
        channel: &Channel,
        parsed: &ParsedEmail,
        sender: ThreadSender<'_>,
    ) -> AppResult<ThreadAccess> {
        let references = reference_ids(parsed);
        let Some(thread) = self
            .find_existing_thread(channel.id, &references, parsed.thread_index.clone())
            .await?
        else {
            return Ok(ThreadAccess::Denied);
        };
        if thread.channel_id != channel.id {
            return Ok(ThreadAccess::Denied);
        }
        if sender.is_party_to(&thread) {
            return Ok(ThreadAccess::Participant);
        }
        let matched = self
            .task_persistence
            .find_correlated_outreach_reply(
                channel.company_id,
                channel.id,
                thread.id,
                sender.address,
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
            matched_slug,
            delivery_address,
            role,
            step_index,
            total_steps,
        } = candidate;
        let sender = parsed.sender.trim().to_string();
        let context = directory
            .access_context(channel.company_id, &sender)
            .await?;

        let existing_thread = self
            .find_existing_thread(
                channel.id,
                &thread_lookup_ids(parsed),
                parsed.thread_index.clone(),
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
                    ThreadSender::new(&sender, context),
                    outreach_by_channel,
                )
                .await?
        {
            return Ok(ControlFlow::Break(rejection));
        }

        let access = channel.participant_access(context);

        // A third party joins the thread only when the sender is trusted *and* the channel
        // permits it: the flag can narrow who gets pulled in, never widen it.
        let pull_third_parties = access.trusted && channel.add_3rd_party;
        let third_party_recipients = if pull_third_parties {
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
                    if !participants
                        .iter()
                        .any(|p| p.eq_ignore_ascii_case(recipient))
                    {
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
                pull_third_parties,
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

        let executes_agent = role == RecipientRole::To
            || self
                .cc_match_is_mentioned(
                    &channel,
                    &company,
                    &matched_slug,
                    &delivery_address,
                    &clean_text_body,
                    directory,
                )
                .await?;

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
                matched_slug: Some(matched_slug),
                thread,
                inbound_message,
                recipient_role: role,
                step_index,
                total_steps,
            },
            executes_agent,
            marks_context_only,
        }))
    }

    async fn cc_match_is_mentioned(
        &self,
        channel: &Channel,
        company: &Company,
        matched_slug: &ChannelSlug,
        delivery_address: &EmailAddress,
        body: &str,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<bool> {
        if body_mentions_email(body, delivery_address) {
            return Ok(true);
        }

        let canonical_address =
            channel.inbound_address(&company.slug, &self.config.app_domain_name);
        if body_mentions_email(body, &canonical_address)
            || body_mentions_slug(body, matched_slug)
            || body_mentions_slug(body, &channel.slug)
            || channel
                .alias_slugs
                .iter()
                .any(|slug| body_mentions_slug(body, slug))
        {
            return Ok(true);
        }

        for agent_id in channel.agent_ids.iter().flatten() {
            if let Some(agent) = directory.agent(*agent_id).await?
                && body_mentions_slug(body, &agent.slug)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Refuse a sender who is neither on the thread nor answering one of its outreaches, and tell
    /// them so with a bounce rather than dropping the mail silently.
    async fn authorize_thread_injection(
        &self,
        company: &Company,
        channel: &Channel,
        thread: &Thread,
        parsed: &ParsedEmail,
        sender: ThreadSender<'_>,
        outreach_by_channel: &mut HashMap<Uuid, OutreachReplyMatch>,
    ) -> AppResult<ControlFlow<InboundIngestResult>> {
        let is_thread_participant = sender.is_party_to(thread);

        if !is_thread_participant && !outreach_by_channel.contains_key(&channel.id) {
            let references = reference_ids(parsed);
            if let Some(matched) = self
                .task_persistence
                .find_correlated_outreach_reply(
                    channel.company_id,
                    channel.id,
                    thread.id,
                    sender.address,
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
            sender.address, thread.id
        );
        Ok(ControlFlow::Break(
            InboundIngestResult::rejected_with_bounce(
                "Sender is not an authorized participant or delegation target for this thread",
                BounceInfo {
                    recipient_to: EmailAddress::from(parsed.sender.clone()),
                    company_slug: Some(company.slug.clone()),
                    invalid_slugs: vec![ChannelSlug::from(format!(
                        "Thread {} (Unauthorized Sender: {})",
                        thread.id, sender.address
                    ))],
                    disabled_slugs: vec![],
                    suggestions: vec![],
                    // This bounce fires *because* the sender has no standing on the thread, so it
                    // must not hand them the company's channel directory.
                    available_channels: vec![],
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
        for address in parsed
            .recipients_to
            .iter()
            .chain(parsed.recipients_cc.iter())
        {
            let address = address.trim();
            if !self
                .is_third_party_address(address, sender, directory)
                .await?
            {
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

    /// Whether this address belongs to someone outside the platform.
    ///
    /// The single definition of "third party": it decides both who is pulled onto a thread at
    /// ingest and who is dropped from an agent reply's Cc when a channel has `add_3rd_party` off.
    pub(super) async fn is_third_party_address(
        &self,
        address: &str,
        sender: &str,
        directory: &mut DirectoryCache<'_>,
    ) -> AppResult<bool> {
        let address = address.trim();
        if address.is_empty() || address.eq_ignore_ascii_case(sender) {
            return Ok(false);
        }
        Ok(!self.is_platform_channel_address(address, directory).await?)
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
        let mut participants = thread.participant_projection.email_addresses.clone();
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
                if !participants
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(recipient))
                {
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
        executable_matches: Vec<ChannelMatch>,
        context_only: bool,
        delivery: ReplyDelivery,
    ) -> AppResult<InboundIngestResult> {
        let first = channel_matches[0].clone();
        let context_only = context_only
            || parsed.is_context_only
            || norm.is_context_only
            || executable_matches.is_empty();
        parsed.is_context_only = context_only;

        let mut norm = norm;
        norm.is_context_only = context_only;
        let correlation_id = norm.correlation_id;

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
            deliver: delivery.sends_email(),
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

        // Passive CC matches have already been written to their histories. Keep them out of the
        // durable task so neither the worker nor the task activity fan-out treats them as active.
        let primary = executable_matches[0].clone();
        let mut durable_result = result.clone();
        durable_result.company = Some(primary.company.clone());
        durable_result.channel = Some(primary.channel.clone());
        durable_result.thread = Some(primary.thread.clone());
        durable_result.inbound_message = Some(primary.inbound_message.clone());
        durable_result.channel_matches = executable_matches;

        // Durable task: survives a crash between ingest and agent execution.
        let task = self
            .task_persistence
            .enqueue_task(NewTask {
                company_id: primary.company.id,
                channel_id: primary.channel.id,
                thread_id: Some(primary.thread.id),
                task_type: AGENT_DISPATCH_TASK.to_string(),
                payload: durable_ingest_payload(&durable_result),
                // The chain starts at the message, not here: an inter-channel reply carrying a
                // correlation header keeps the chain its sender was already on.
                correlation_id,
            })
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
        thread_index: parsed.thread_index.clone(),
        created_at: chrono::Utc::now(),
    }
}
