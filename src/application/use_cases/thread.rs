use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use chrono::NaiveDateTime;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    adapters::persistence::task::{TASK_LEASE_SECONDS, TaskPersistence},
    adapters::protocols::{
        EgressRegistry,
        email::{EmailEgressAdapter, EmailIngressAdapter},
    },
    app_error::{AppError, AppResult},
    domain::monitoring::MonitoringService,
    entities::{
        channel::{Channel, ChannelType, ParticipantIdentity},
        company::Company,
        message::{Message, MessageDirection, MessageRole},
        message_contract::{NormalizedInboundMessage, NormalizedOutboundMessage},
        outreach::OutreachReplyMatch,
        task::TokenUsage,
        thread::Thread,
        value_objects::{ChannelSlug, CompanySlug, EmailAddress, MessageId, ThreadIndex},
    },
    infra::config::AppConfig,
    services::{
        agent_runner::{
            AgentExecutionDisposition, AgentRunner, ApprovalContext as AgentRunnerApprovalContext,
            ResolvedAgentParams,
        },
        email_parser::{EmailParser, MAX_CHANNEL_HOPS, ParsedEmail, RawInboundPayload},
        outbound_dispatcher::{OutboundDispatcher, OutboundEmail, SentEmailResult},
        outreach_tool::OutreachToolContext,
    },
    use_cases::{
        agent::AgentPersistence,
        approval::ApprovalUseCases,
        channel::{
            ChannelPersistence, find_similar_channel_slugs, parse_recipient_address_pipeline,
        },
        company::CompanyPersistence,
    },
};

pub const MAX_THREAD_MESSAGES_PER_HOUR: usize = 20;

#[derive(Clone, Copy)]
struct InternalChannelSource {
    company_id: Uuid,
    channel_id: Uuid,
}

#[async_trait]
pub trait ThreadPersistence: Send + Sync {
    async fn create_thread(
        &self,
        channel_id: Uuid,
        subject: &str,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread>;

    async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>>;

    async fn list_threads_by_channel_id(
        &self,
        channel_id: Uuid,
        before: Option<(NaiveDateTime, Uuid)>,
        limit: usize,
    ) -> AppResult<Vec<Thread>>;

    async fn update_thread_participants(
        &self,
        id: Uuid,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread>;

    async fn find_thread_by_message_ids(
        &self,
        channel_id: Uuid,
        message_ids: &[MessageId],
    ) -> AppResult<Option<Thread>>;

    async fn find_thread_by_thread_index(
        &self,
        channel_id: Uuid,
        thread_index_prefix: &ThreadIndex,
    ) -> AppResult<Option<Thread>>;

    async fn count_recent_messages(&self, thread_id: Uuid, duration_secs: i64) -> AppResult<usize>;

    async fn create_message(&self, message: &Message) -> AppResult<Message>;

    async fn get_message_by_message_id(
        &self,
        company_id: Uuid,
        message_id: &MessageId,
    ) -> AppResult<Option<Message>>;

    async fn find_outbound_reply(
        &self,
        thread_id: Uuid,
        in_reply_to: &MessageId,
    ) -> AppResult<Option<Message>>;

    async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>>;
}

#[derive(Clone)]
pub struct ThreadUseCases {
    thread_persistence: Arc<dyn ThreadPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    company_persistence: Arc<dyn CompanyPersistence>,
    task_persistence: Arc<dyn TaskPersistence>,
    agent_persistence: Option<Arc<dyn AgentPersistence>>,
    approval_use_cases: Option<Arc<ApprovalUseCases>>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    egress_registry: Arc<EgressRegistry>,
    config: Arc<AppConfig>,
}

impl ThreadUseCases {
    pub fn new(
        thread_persistence: Arc<dyn ThreadPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        company_persistence: Arc<dyn CompanyPersistence>,
        task_persistence: Arc<dyn TaskPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        let egress_registry = Arc::new(
            EgressRegistry::new().register(Arc::new(EmailEgressAdapter::new(config.clone()))),
        );

        Self {
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            agent_persistence: None,
            approval_use_cases: None,
            monitoring: None,
            egress_registry,
            config,
        }
    }

    pub fn with_egress_registry(mut self, egress_registry: Arc<EgressRegistry>) -> Self {
        self.egress_registry = egress_registry;
        self
    }

    pub fn with_agent_persistence(mut self, agent_persistence: Arc<dyn AgentPersistence>) -> Self {
        self.agent_persistence = Some(agent_persistence);
        self
    }

    pub fn with_approval_use_cases(mut self, approval_use_cases: Arc<ApprovalUseCases>) -> Self {
        self.approval_use_cases = Some(approval_use_cases);
        self
    }

    pub fn get_approval_use_cases(&self) -> Option<Arc<ApprovalUseCases>> {
        self.approval_use_cases.clone()
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    pub fn channel_persistence(&self) -> &Arc<dyn ChannelPersistence> {
        &self.channel_persistence
    }

    pub fn company_persistence(&self) -> &Arc<dyn CompanyPersistence> {
        &self.company_persistence
    }

    pub fn config(&self) -> &Arc<AppConfig> {
        &self.config
    }

    async fn resolve_internal_destination(
        &self,
        source_channel_id: Uuid,
        recipient: &str,
    ) -> AppResult<Option<(Channel, Channel, Company)>> {
        let recipient_domain = recipient
            .trim()
            .rsplit_once('@')
            .map(|(_, domain)| domain.trim().to_lowercase())
            .unwrap_or_default();
        let app_domain = self.config.app_domain_name.trim().to_lowercase();
        if recipient_domain != app_domain && !recipient_domain.ends_with(&format!(".{app_domain}"))
        {
            return Ok(None);
        }

        let (company_slug, channel_slugs, is_context_only) =
            parse_recipient_address_pipeline(recipient, &self.config.app_domain_name).ok_or_else(
                || AppError::Internal(format!("Invalid platform channel address: {recipient}")),
            )?;
        if is_context_only || channel_slugs.len() != 1 {
            return Err(AppError::Internal(
                "Internal channel delivery requires one direct channel address".into(),
            ));
        }

        let source = self
            .channel_persistence
            .get_by_id(source_channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Internal source channel was not found".into()))?;
        let target = self
            .channel_persistence
            .get_by_company_slug_and_channel_slug(&company_slug, &channel_slugs[0])
            .await?
            .ok_or_else(|| {
                AppError::Internal(format!("Platform channel does not exist: {recipient}"))
            })?;
        if source.company_id != target.company_id {
            return Err(AppError::Internal(
                "Cross-company internal channel delivery is not allowed".into(),
            ));
        }
        if source.id == target.id {
            return Err(AppError::Internal(
                "A channel cannot deliver an internal message to itself".into(),
            ));
        }
        if target.agent_ids.as_ref().is_none_or(Vec::is_empty) {
            return Err(AppError::Internal(format!(
                "Target channel '{}' has no configured agent",
                target.slug
            )));
        }
        let company = self
            .company_persistence
            .get_by_id(source.company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Internal source company was not found".into()))?;
        if !company.slug.eq_ignore_ascii_case(&company_slug) {
            return Err(AppError::Internal(
                "Internal recipient company does not match the source company".into(),
            ));
        }
        Ok(Some((source, target, company)))
    }

    pub async fn prepare_internal_channel_delivery(
        &self,
        email: OutboundEmail,
        idempotency_key: Option<&str>,
    ) -> AppResult<Option<SentEmailResult>> {
        if self
            .resolve_internal_destination(email.channel_id, &email.recipient_to)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        let prepared = match idempotency_key {
            Some(key) => OutboundDispatcher::prepare_idempotent(&self.config, email, key)?,
            None => OutboundDispatcher::prepare(&self.config, email)?,
        };
        Ok(Some(prepared))
    }

    pub async fn ingest_prepared_internal_message(
        &self,
        sent: &SentEmailResult,
    ) -> AppResult<InboundIngestResult> {
        let source_channel_id = sent.source_channel_id.ok_or_else(|| {
            AppError::Internal("Internal delivery has no source channel identity".into())
        })?;
        let recipient = sent
            .recipients_to
            .first()
            .ok_or_else(|| AppError::Internal("Internal delivery has no recipient".into()))?;
        let (source, _, company) = self
            .resolve_internal_destination(source_channel_id, recipient)
            .await?
            .ok_or_else(|| AppError::Internal("Recipient is not an internal channel".into()))?;
        let expected_sender = format!(
            "{}@{}.{}",
            source.slug, company.slug, self.config.app_domain_name
        );
        if !sent.from_address.eq_ignore_ascii_case(&expected_sender) {
            return Err(AppError::Internal(
                "Internal sender address does not match its source channel".into(),
            ));
        }

        let norm = NormalizedInboundMessage {
            message_id: sent.outbound_message_id.clone(),
            thread_ref: Some(sent.in_reply_to.clone()),
            references: sent.references.clone(),
            thread_index: None,
            sender: ParticipantIdentity::email(sent.from_address.clone()),
            recipients_to: sent
                .recipients_to
                .iter()
                .cloned()
                .map(ParticipantIdentity::email)
                .collect(),
            recipients_cc: sent
                .recipients_cc
                .iter()
                .cloned()
                .map(ParticipantIdentity::email)
                .collect(),
            subject: sent.subject.clone(),
            clean_text: sent.body_text.clone(),
            raw_text: Some(sent.body_text.clone()),
            raw_html: None,
            attachments: Vec::new(),
            is_auto_reply: true,
            is_forwarded: false,
            channel_id_header: Some(source_channel_id),
            hop_count: sent.hop_count,
            trace_channels: sent.trace_channels.clone(),
            protocol: ChannelType::Email,
            spf_status: None,
            dkim_status: None,
            dmarc_status: None,
            spam_score: None,
            is_context_only: false,
        };
        self.ingest_normalized_message_with_source(
            norm,
            Some(InternalChannelSource {
                company_id: company.id,
                channel_id: source_channel_id,
            }),
        )
        .await
    }

    pub async fn record_outreach_outbound_message(
        &self,
        outbox_id: Uuid,
        sent: &crate::services::outbound_dispatcher::SentEmailResult,
    ) -> AppResult<()> {
        let Some(thread_id) = self
            .task_persistence
            .get_outreach_thread_for_outbox(outbox_id)
            .await?
        else {
            return Ok(());
        };
        self.thread_persistence
            .create_message(&Message {
                id: Uuid::new_v4(),
                thread_id,
                message_id: MessageId::from(sent.outbound_message_id.clone()),
                in_reply_to: Some(MessageId::from(sent.in_reply_to.clone())),
                references_list: sent.references.iter().cloned().map(MessageId::from).collect(),
                sender: EmailAddress::from(sent.from_address.clone()),
                recipients_to: sent.recipients_to.iter().cloned().map(EmailAddress::from).collect(),
                recipients_cc: sent.recipients_cc.iter().cloned().map(EmailAddress::from).collect(),
                subject: sent.subject.clone(),
                clean_text_body: sent.body_text.clone(),
                raw_text_body: None,
                raw_html_body: None,
                attachments: None,
                direction: MessageDirection::Outbound,
                role: MessageRole::Agent,
                thread_index: None,
                created_at: chrono::Utc::now().naive_utc(),
            })
            .await?;
        Ok(())
    }

    #[instrument(skip(self, raw_payload))]
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

    async fn ingest_normalized_message_with_source(
        &self,
        norm: NormalizedInboundMessage,
        internal_source: Option<InternalChannelSource>,
    ) -> AppResult<InboundIngestResult> {
        let mut parsed = ParsedEmail {
            message_id: norm.message_id.clone().into_string(),
            in_reply_to: norm.thread_ref.clone().map(MessageId::into_string),
            references: norm
                .references
                .iter()
                .cloned()
                .map(MessageId::into_string)
                .collect(),
            thread_index: norm.thread_index.clone().map(ThreadIndex::into_string),
            sender: norm.sender.identity.clone(),
            recipients_to: norm
                .recipients_to
                .iter()
                .map(|p| p.identity.clone())
                .collect(),
            recipients_cc: norm
                .recipients_cc
                .iter()
                .map(|p| p.identity.clone())
                .collect(),
            subject: norm.subject.clone(),
            clean_text_body: norm.clean_text.clone(),
            raw_text_body: norm.raw_text.clone(),
            raw_html_body: norm.raw_html.clone(),
            attachments: norm.attachments.clone(),
            prompt_text: norm.clean_text.clone(),
            is_auto_reply: norm.is_auto_reply,
            is_forwarded: norm.is_forwarded,
            channel_id_header: norm.channel_id_header,
            hop_count: norm.hop_count,
            trace_channels: norm.trace_channels.clone(),
            spf_status: norm.spf_status.clone(),
            dkim_status: norm.dkim_status.clone(),
            dmarc_status: norm.dmarc_status.clone(),
            spam_score: norm.spam_score,
            is_context_only: norm.is_context_only,
        };

        info!(
            "Ingesting email Message-ID: {}, From: {}, To: {:?}",
            parsed.message_id, parsed.sender, parsed.recipients_to
        );

        let is_inter_channel = internal_source.is_some();
        if let Some(source) = internal_source
            && parsed.channel_id_header != Some(source.channel_id)
        {
            return Ok(InboundIngestResult::rejected(
                "Internal source channel identity mismatch",
            ));
        }

        // Global SPF / DKIM / DMARC authentication verification check
        if !is_inter_channel {
            if let Some(ref spf) = parsed.spf_status {
                if spf.eq_ignore_ascii_case("fail") {
                    warn!("SPF authentication failed for sender '{}'", parsed.sender);
                    return Ok(InboundIngestResult::rejected("SPF authentication failed"));
                }
            }
            if let Some(ref dkim) = parsed.dkim_status {
                if dkim.eq_ignore_ascii_case("fail") {
                    warn!("DKIM authentication failed for sender '{}'", parsed.sender);
                    return Ok(InboundIngestResult::rejected("DKIM authentication failed"));
                }
            }
            if let Some(ref dmarc) = parsed.dmarc_status {
                if dmarc.eq_ignore_ascii_case("fail") {
                    warn!("DMARC authentication failed for sender '{}'", parsed.sender);
                    return Ok(InboundIngestResult::rejected("DMARC authentication failed"));
                }
            }
        }

        // Inter-channel hop limit check
        if is_inter_channel && parsed.hop_count >= MAX_CHANNEL_HOPS {
            warn!(
                "Max inter-channel hop count ({}) reached for Message-ID: {}",
                parsed.hop_count, parsed.message_id
            );
            return Ok(InboundIngestResult::rejected(
                "Max inter-channel hop count reached",
            ));
        }

        if !is_inter_channel && parsed.is_auto_reply {
            warn!(
                "External auto-reply loop detected for Message-ID: {}, dropping message",
                parsed.message_id
            );
            return Ok(InboundIngestResult::rejected(
                "External auto-reply loop detected",
            ));
        }

        // Collect all target recipient candidates (To first, then CC)
        let mut candidates = Vec::new();
        for to in &parsed.recipients_to {
            candidates.push((to.clone(), RecipientRole::To));
        }
        for cc in &parsed.recipients_cc {
            candidates.push((cc.clone(), RecipientRole::Cc));
        }

        let mut candidate_matches: Vec<(Company, Channel, RecipientRole, usize, usize)> =
            Vec::new();
        let mut seen_channels = std::collections::HashSet::new();
        let mut company_cache: HashMap<String, Option<Company>> = HashMap::new();
        let mut channel_cache: HashMap<Uuid, Vec<Channel>> = HashMap::new();
        let mut membership_cache: HashMap<(Uuid, String), bool> = HashMap::new();
        let mut unauthorized_count = 0;

        let mut invalid_slugs = Vec::new();
        let mut bounce_suggestions = Vec::new();
        let mut matched_company_slug: Option<CompanySlug> = None;
        let mut is_address_context_only = false;
        let mut matched_outreach_by_channel: HashMap<Uuid, OutreachReplyMatch> = HashMap::new();

        for (addr, role) in candidates {
            if let Some((company_slug, channel_slugs, is_addr_context)) =
                parse_recipient_address_pipeline(&addr, &self.config.app_domain_name)
            {
                if is_addr_context {
                    is_address_context_only = true;
                }
                matched_company_slug = Some(company_slug.clone());
                let company_key = company_slug.to_lowercase();
                let company_opt = if let Some(cached) = company_cache.get(&company_key) {
                    cached.clone()
                } else {
                    let loaded = self
                        .company_persistence
                        .get_by_slug(&company_slug)
                        .await
                        .ok()
                        .flatten();
                    company_cache.insert(company_key, loaded.clone());
                    loaded
                };

                if let Some(company) = company_opt {
                    if let Some(source) = internal_source
                        && company.id != source.company_id
                    {
                        return Ok(InboundIngestResult::rejected(
                            "Cross-company internal channel delivery is not allowed",
                        ));
                    }
                    let available_channels = if let Some(cached) = channel_cache.get(&company.id) {
                        cached.clone()
                    } else {
                        let loaded = self
                            .channel_persistence
                            .list_by_company_id(company.id)
                            .await
                            .unwrap_or_default();
                        channel_cache.insert(company.id, loaded.clone());
                        loaded
                    };
                    let total_steps = channel_slugs.len();

                    for (step_idx, channel_slug) in channel_slugs.into_iter().enumerate() {
                        let matched_wf = available_channels
                            .iter()
                            .find(|w| w.slug.eq_ignore_ascii_case(&channel_slug))
                            .cloned();

                        if let Some(channel) = matched_wf {
                            if seen_channels.contains(&channel.id) {
                                continue;
                            }

                            // A channel may appear again only when this is the correlated
                            // response to an outreach that is already waiting in that thread.
                            if is_inter_channel && parsed.trace_channels.contains(&channel.id) {
                                let mut reply_references: Vec<MessageId> = Vec::new();
                                if let Some(ref reply_id) = parsed.in_reply_to {
                                    reply_references.push(MessageId::from(reply_id.clone()));
                                }
                                reply_references.extend(
                                    parsed.references.iter().cloned().map(MessageId::from),
                                );
                                let existing_thread = if reply_references.is_empty() {
                                    None
                                } else {
                                    self.thread_persistence
                                        .find_thread_by_message_ids(channel.id, &reply_references)
                                        .await?
                                };
                                let correlated = if let Some(thread) = existing_thread {
                                    self.task_persistence
                                        .find_correlated_outreach_reply(
                                            channel.company_id,
                                            channel.id,
                                            thread.id,
                                            parsed.sender.trim(),
                                            &reply_references,
                                        )
                                        .await?
                                } else {
                                    None
                                };
                                if let Some(matched) = correlated {
                                    matched_outreach_by_channel.insert(channel.id, matched);
                                } else {
                                    warn!(
                                        "Inter-channel cycle detected for channel '{}' in Message-ID: {}",
                                        channel.id, parsed.message_id
                                    );
                                    return Ok(InboundIngestResult::rejected(
                                        "Inter-channel loop cycle detected",
                                    ));
                                }
                            }

                            // ACL & Participant Restriction Check
                            let sender_clean = parsed.sender.trim();
                            let member_key = (channel.company_id, sender_clean.to_lowercase());
                            let is_team_member =
                                if let Some(cached) = membership_cache.get(&member_key) {
                                    *cached
                                } else {
                                    let loaded = self
                                        .company_persistence
                                        .is_company_team_member(channel.company_id, sender_clean)
                                        .await
                                        .unwrap_or(false);
                                    membership_cache.insert(member_key, loaded);
                                    loaded
                                };

                            let (is_authorized, is_trusted_participant) =
                                match &channel.participant_emails {
                                    Some(allowed) if !allowed.is_empty() => {
                                        let is_public = allowed
                                            .iter()
                                            .any(|e| e.trim().eq_ignore_ascii_case("@public"));
                                        let explicitly_listed = allowed.iter().any(|e| {
                                            !e.trim().eq_ignore_ascii_case("@public")
                                                && e.eq_ignore_ascii_case(sender_clean)
                                        });
                                        let authorized = is_public || explicitly_listed;
                                        let trusted = explicitly_listed || is_team_member;
                                        (authorized, trusted)
                                    }
                                    _ => {
                                        // None or empty: default to company team members
                                        (is_team_member, is_team_member)
                                    }
                                };

                            let mut is_thread_authorized = false;
                            if !is_authorized && !is_inter_channel {
                                let mut lookup_ids: Vec<MessageId> = Vec::new();
                                if let Some(ref reply_id) = parsed.in_reply_to {
                                    lookup_ids.push(MessageId::from(reply_id.clone()));
                                }
                                lookup_ids
                                    .extend(parsed.references.iter().cloned().map(MessageId::from));

                                let mut ext_t = if !lookup_ids.is_empty() {
                                    self.thread_persistence
                                        .find_thread_by_message_ids(channel.id, &lookup_ids)
                                        .await
                                        .ok()
                                        .flatten()
                                } else {
                                    None
                                };

                                if ext_t.is_none() {
                                    if let Some(ref idx) = parsed.thread_index {
                                        ext_t = self
                                            .thread_persistence
                                            .find_thread_by_thread_index(
                                                channel.id,
                                                &ThreadIndex::from(idx.clone()),
                                            )
                                            .await
                                            .ok()
                                            .flatten();
                                    }
                                }

                                if let Some(ref t) = ext_t {
                                    if t.channel_id == channel.id {
                                        if t.participant_emails
                                            .iter()
                                            .any(|p| p.trim().eq_ignore_ascii_case(sender_clean))
                                        {
                                            is_thread_authorized = true;
                                        } else if let Ok(Some(matched)) = self
                                            .task_persistence
                                            .find_correlated_outreach_reply(
                                                channel.company_id,
                                                channel.id,
                                                t.id,
                                                sender_clean,
                                                &lookup_ids,
                                            )
                                            .await
                                        {
                                            is_thread_authorized = true;
                                            matched_outreach_by_channel.insert(channel.id, matched);
                                        }
                                    }
                                }
                            }

                            if !is_authorized && !is_thread_authorized && !is_inter_channel {
                                warn!(
                                    "Sender '{}' unauthorized for channel '{}'",
                                    parsed.sender, channel.slug
                                );
                                unauthorized_count += 1;
                                continue;
                            }

                            // Spam score check (skipped for trusted participants)
                            let is_participant = is_trusted_participant;
                            if !is_participant {
                                if let Some(score) = parsed.spam_score {
                                    if score >= self.config.max_spam_score {
                                        warn!(
                                            "Message-ID '{}' rejected due to high spam score ({:.2} >= {:.2})",
                                            parsed.message_id, score, self.config.max_spam_score
                                        );
                                        return Ok(InboundIngestResult::rejected(
                                            "Spam score threshold exceeded",
                                        ));
                                    }
                                }
                            }

                            seen_channels.insert(channel.id);
                            candidate_matches.push((
                                company.clone(),
                                channel,
                                role,
                                step_idx,
                                total_steps,
                            ));
                        } else {
                            // Misspelled / Invalid channel slug found
                            let suggestions =
                                find_similar_channel_slugs(&channel_slug, &available_channels);
                            invalid_slugs.push(channel_slug.clone());
                            bounce_suggestions.push(BounceSuggestion {
                                invalid_slug: channel_slug,
                                suggestions,
                            });
                        }
                    }
                }
            }
        }

        // Strict pipeline & address validation: If any invalid channel slug was encountered
        if !invalid_slugs.is_empty() {
            let bounce = BounceInfo {
                recipient_to: EmailAddress::from(parsed.sender.clone()),
                company_slug: matched_company_slug,
                invalid_slugs,
                suggestions: bounce_suggestions,
                original_subject: parsed.subject.clone(),
            };
            return Ok(InboundIngestResult::rejected_with_bounce(
                "Channel address not found or misspelled",
                bounce,
            ));
        }

        if candidate_matches.is_empty() {
            if unauthorized_count > 0 {
                return Ok(InboundIngestResult::rejected(
                    "Sender unauthorized for channel",
                ));
            }
            warn!(
                "No valid company/channel matched for To: {:?}, CC: {:?}",
                parsed.recipients_to, parsed.recipients_cc
            );
            return Ok(InboundIngestResult::rejected(
                "Company or Channel not found",
            ));
        }

        let task_company_id = candidate_matches[0].0.id;
        if candidate_matches
            .iter()
            .any(|(company, _, _, _, _)| company.id != task_company_id)
        {
            return Ok(InboundIngestResult::rejected(
                "A channel pipeline cannot span multiple companies",
            ));
        }

        if let Ok(Some(_)) = self
            .task_persistence
            .get_task_by_source_message_id(task_company_id, &parsed.message_id)
            .await
        {
            warn!(
                "Duplicate Message-ID '{}' already processed for company {}",
                parsed.message_id, task_company_id
            );
            return Ok(InboundIngestResult::rejected(
                "Duplicate Message-ID already processed",
            ));
        }

        let mut channel_matches = Vec::new();

        for (company, channel, role, step_idx, total_steps) in candidate_matches {
            // Thread Resolution
            let mut lookup_ids = vec![MessageId::from(parsed.message_id.clone())];
            if let Some(ref reply_id) = parsed.in_reply_to {
                lookup_ids.push(MessageId::from(reply_id.clone()));
            }
            lookup_ids.extend(parsed.references.iter().cloned().map(MessageId::from));

            let mut existing_thread = self
                .thread_persistence
                .find_thread_by_message_ids(channel.id, &lookup_ids)
                .await?;

            if existing_thread.is_none() {
                if let Some(ref idx) = parsed.thread_index {
                    existing_thread = self
                        .thread_persistence
                        .find_thread_by_thread_index(channel.id, &ThreadIndex::from(idx.clone()))
                        .await?;
                }
            }

            if let Some(ref ext_t) = existing_thread {
                if ext_t.channel_id == channel.id {
                    let sender_clean = parsed.sender.trim();
                    let is_thread_participant = ext_t
                        .participant_emails
                        .iter()
                        .any(|p| p.trim().eq_ignore_ascii_case(sender_clean));

                    if !is_thread_participant
                        && !matched_outreach_by_channel.contains_key(&channel.id)
                    {
                        let mut reply_references: Vec<MessageId> = Vec::new();
                        if let Some(ref reply_id) = parsed.in_reply_to {
                            reply_references.push(MessageId::from(reply_id.clone()));
                        }
                        reply_references.extend(
                            parsed.references.iter().cloned().map(MessageId::from),
                        );
                        if let Some(matched) = self
                            .task_persistence
                            .find_correlated_outreach_reply(
                                channel.company_id,
                                channel.id,
                                ext_t.id,
                                sender_clean,
                                &reply_references,
                            )
                            .await?
                        {
                            matched_outreach_by_channel.insert(channel.id, matched);
                        }
                    }
                    let is_outreach_target = matched_outreach_by_channel.contains_key(&channel.id);

                    if !is_thread_participant && !is_outreach_target {
                        warn!(
                            "Unauthorized thread injection attempt by '{}' for thread '{}'. Triggering bounce notification.",
                            sender_clean, ext_t.id
                        );

                        let bounce = BounceInfo {
                            recipient_to: EmailAddress::from(parsed.sender.clone()),
                            company_slug: Some(company.slug.clone()),
                            invalid_slugs: vec![ChannelSlug::from(format!(
                                "Thread {} (Unauthorized Sender: {})",
                                ext_t.id, sender_clean
                            ))],
                            suggestions: vec![],
                            original_subject: parsed.subject.clone(),
                        };

                        return Ok(InboundIngestResult::rejected_with_bounce(
                            "Sender is not an authorized participant or delegation target for this thread",
                            bounce,
                        ));
                    }
                }
            }

            // Extract third-party recipients if sender is a workflow participant or team member
            let sender_clean = parsed.sender.trim();
            let member_key = (channel.company_id, sender_clean.to_lowercase());
            let is_team_member = if let Some(cached) = membership_cache.get(&member_key) {
                *cached
            } else {
                let loaded = self
                    .company_persistence
                    .is_company_team_member(channel.company_id, sender_clean)
                    .await
                    .unwrap_or(false);
                membership_cache.insert(member_key, loaded);
                loaded
            };

            let is_trusted_participant = match &channel.participant_emails {
                Some(allowed) if !allowed.is_empty() => {
                    let explicitly_listed = allowed.iter().any(|e| {
                        !e.trim().eq_ignore_ascii_case("@public")
                            && e.eq_ignore_ascii_case(sender_clean)
                    });
                    explicitly_listed || is_team_member
                }
                _ => is_team_member,
            };

            let mut third_party_recipients = Vec::new();
            if is_trusted_participant {
                let mut candidate_recipients = Vec::new();
                candidate_recipients.extend(parsed.recipients_to.clone());
                candidate_recipients.extend(parsed.recipients_cc.clone());

                for addr in candidate_recipients {
                    let addr_clean = addr.trim();
                    if addr_clean.is_empty() {
                        continue;
                    }
                    if addr_clean.eq_ignore_ascii_case(sender_clean) {
                        continue;
                    }
                    // Check if addr is an actual channel email address in our system
                    let is_channel_address = if let Some((comp_slug, _, _)) =
                        parse_recipient_address_pipeline(addr_clean, &self.config.app_domain_name)
                    {
                        let key = comp_slug.to_lowercase();
                        let company = if let Some(cached) = company_cache.get(&key) {
                            cached.clone()
                        } else {
                            let loaded = self
                                .company_persistence
                                .get_by_slug(&comp_slug)
                                .await
                                .ok()
                                .flatten();
                            company_cache.insert(key, loaded.clone());
                            loaded
                        };
                        company.is_some()
                    } else {
                        false
                    };

                    if is_channel_address {
                        continue;
                    }

                    if !third_party_recipients
                        .iter()
                        .any(|r: &EmailAddress| r.eq_ignore_ascii_case(addr_clean))
                    {
                        third_party_recipients.push(EmailAddress::from(addr_clean.to_string()));
                    }
                }
            }

            let thread = match existing_thread {
                Some(t) if t.channel_id == channel.id => t,
                _ => {
                    let mut participants = vec![EmailAddress::from(parsed.sender.clone())];
                    for tp in &third_party_recipients {
                        if !participants.iter().any(|p| p.eq_ignore_ascii_case(tp)) {
                            participants.push(tp.clone());
                        }
                    }

                    self.thread_persistence
                        .create_thread(channel.id, &parsed.subject, &participants)
                        .await?
                }
            };

            // Thread Turn Limit Check
            let recent_count = self
                .thread_persistence
                .count_recent_messages(thread.id, 3600)
                .await?;
            if recent_count >= MAX_THREAD_MESSAGES_PER_HOUR {
                warn!(
                    "Thread turn limit ({}/hr) exceeded for thread_id {}, dropping response to prevent ping-pong loop",
                    recent_count, thread.id
                );
                return Ok(InboundIngestResult::rejected("Thread turn limit exceeded"));
            }

            // Update thread participants
            let mut current_participants = thread.participant_emails.clone();
            let mut participant_added = false;
            if !matched_outreach_by_channel.contains_key(&channel.id)
                && !current_participants
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(&parsed.sender))
            {
                current_participants.push(EmailAddress::from(parsed.sender.clone()));
                participant_added = true;
            }
            if is_trusted_participant {
                for tp in &third_party_recipients {
                    if !current_participants
                        .iter()
                        .any(|p| p.eq_ignore_ascii_case(tp))
                    {
                        current_participants.push(tp.clone());
                        participant_added = true;
                    }
                }
            }
            let thread = if participant_added {
                self.thread_persistence
                    .update_thread_participants(thread.id, &current_participants)
                    .await?
            } else {
                thread
            };

            // Fetch thread history for quote stripping fallback
            let history_messages = self
                .thread_persistence
                .list_messages_by_thread_id(thread.id)
                .await?;
            let history_clean_bodies: Vec<String> = history_messages
                .iter()
                .map(|m| m.clean_text_body.clone())
                .collect();

            let clean_text_body = if parsed.is_forwarded || history_messages.is_empty() {
                parsed.clean_text_body.clone()
            } else {
                let heuristic_clean = EmailParser::strip_quotes_heuristic(&parsed.clean_text_body);
                EmailParser::strip_historical_quotes_fallback(
                    &heuristic_clean,
                    &history_clean_bodies,
                )
            };

            if clean_text_body != parsed.clean_text_body {
                parsed.clean_text_body = clean_text_body.clone();
                let attachment_prompts: Vec<String> = parsed
                    .attachments
                    .iter()
                    .filter(|att| {
                        !(att.content_type.to_lowercase().starts_with("image/")
                            && att.size_bytes < 10_000)
                    })
                    .map(|meta| {
                        format!(
                            "[Attachment: {}, Type: {}, SHA256: {}, Size: {} bytes]",
                            meta.filename, meta.content_type, meta.sha256_hash, meta.size_bytes
                        )
                    })
                    .collect();

                parsed.prompt_text = if attachment_prompts.is_empty() {
                    parsed.clean_text_body.clone()
                } else {
                    format!(
                        "{}\n\n{}",
                        parsed.clean_text_body,
                        attachment_prompts.join("\n")
                    )
                };
            }

            let inbound_role = if is_inter_channel {
                MessageRole::Agent
            } else {
                MessageRole::Human
            };

            let inbound_msg_id = Uuid::new_v4();
            let inbound_message = Message {
                id: inbound_msg_id,
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
                attachments: if parsed.attachments.is_empty() {
                    None
                } else {
                    Some(parsed.attachments.clone())
                },
                direction: MessageDirection::Inbound,
                role: inbound_role,
                thread_index: parsed.thread_index.clone().map(ThreadIndex::from),
                created_at: chrono::Utc::now().naive_utc(),
            };

            let saved_inbound = self
                .thread_persistence
                .create_message(&inbound_message)
                .await?;

            if let Some(matched) = matched_outreach_by_channel.get(&channel.id) {
                self.task_persistence
                    .record_outreach_reply(matched, saved_inbound.id)
                    .await?;
                is_address_context_only = true;
            }

            channel_matches.push(ChannelMatch {
                company,
                channel,
                thread,
                inbound_message: saved_inbound,
                recipient_role: role,
                step_index: step_idx,
                total_steps,
            });
        }

        let first = channel_matches[0].clone();

        let is_context_only_effective =
            parsed.is_context_only || norm.is_context_only || is_address_context_only;
        parsed.is_context_only = is_context_only_effective;
        let mut norm_updated = norm.clone();
        norm_updated.is_context_only = is_context_only_effective;

        let mut ingest_result = InboundIngestResult {
            accepted: true,
            reason: None,
            thread: Some(first.thread.clone()),
            inbound_message: Some(first.inbound_message.clone()),
            company: Some(first.company.clone()),
            channel: Some(first.channel.clone()),
            parsed_email: Some(parsed.clone()),
            normalized_message: Some(norm_updated),
            task_id: None,
            channel_matches,
            bounce_info: None,
        };

        if is_context_only_effective {
            info!(
                "Ingested message ID '{}' for thread '{}' in context-only / quiet mode. Skipping background task creation and agent execution.",
                parsed.message_id, first.thread.id
            );
            return Ok(ingest_result);
        }

        // Enqueue background task for durable processing & crash recovery
        let payload_json = durable_ingest_payload(&ingest_result);
        let task = self
            .task_persistence
            .enqueue_task(
                first.company.id,
                first.channel.id,
                Some(first.thread.id),
                "email_agent_dispatch",
                payload_json,
            )
            .await?;
        ingest_result.task_id = Some(task.id);

        Ok(ingest_result)
    }

    pub async fn execute_agent_and_dispatch(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
    ) -> AppResult<Option<AgentExecutionResult>> {
        self.execute_agent_and_dispatch_inner(ingest, send_email, false, None)
            .await
    }

    pub async fn execute_claimed_agent_task_and_dispatch(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
        worker_id: Uuid,
    ) -> AppResult<Option<AgentExecutionResult>> {
        self.execute_agent_and_dispatch_inner(ingest, send_email, true, Some(worker_id))
            .await
    }

    async fn execute_agent_and_dispatch_inner(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
        task_already_claimed: bool,
        claimed_worker_id: Option<Uuid>,
    ) -> AppResult<Option<AgentExecutionResult>> {
        if let Some(ref p) = ingest.parsed_email {
            if p.is_context_only {
                info!(
                    "Skipping agent execution for context-only message ID {}",
                    p.message_id
                );
                return Ok(None);
            }
        }
        if let Some(ref n) = ingest.normalized_message {
            if n.is_context_only {
                info!(
                    "Skipping agent execution for context-only message ID {}",
                    n.message_id
                );
                return Ok(None);
            }
        }

        let mut direct_worker_id = None;
        if !task_already_claimed && let Some(task_id) = ingest.task_id {
            let worker_id = Uuid::new_v4();
            let lock_expires_at =
                chrono::Utc::now().naive_utc() + chrono::Duration::seconds(TASK_LEASE_SECONDS);
            match self
                .task_persistence
                .claim_task(task_id, worker_id, lock_expires_at)
                .await
            {
                Ok(true) => {
                    info!("Successfully claimed task {} for execution", task_id);
                    direct_worker_id = Some(worker_id);
                }
                Ok(false) => {
                    info!(
                        "Task {} already claimed or completed by another worker, skipping duplicate dispatch",
                        task_id
                    );
                    return Ok(None);
                }
                Err(err) => {
                    return Err(err);
                }
            }
        }
        let effective_worker_id = claimed_worker_id.or(direct_worker_id);

        let parsed = match &ingest.parsed_email {
            Some(p) => p,
            None => return Ok(None),
        };

        let matches: Vec<ChannelMatch> = if !ingest.channel_matches.is_empty() {
            ingest.channel_matches.clone()
        } else if let (Some(c), Some(w), Some(t), Some(m)) = (
            &ingest.company,
            &ingest.channel,
            &ingest.thread,
            &ingest.inbound_message,
        ) {
            vec![ChannelMatch {
                company: c.clone(),
                channel: w.clone(),
                thread: t.clone(),
                inbound_message: m.clone(),
                recipient_role: RecipientRole::To,
                step_index: 0,
                total_steps: 1,
            }]
        } else {
            return Ok(None);
        };

        let mut agent_outputs: Vec<(
            &ChannelMatch,
            String,
            Option<String>,
            Option<serde_json::Value>,
        )> = Vec::new();
        let mut total_prompt_tokens = 0usize;
        let mut total_completion_tokens = 0usize;
        let mut primary_execution_error = None;
        let mut primary_resolved_params = None;
        let mut primary_agent = None;
        let mut membership_cache: HashMap<(Uuid, String), bool> = HashMap::new();
        let mut agent_cache: HashMap<Uuid, Option<crate::entities::agent::Agent>> = HashMap::new();

        for (idx, m) in matches.iter().enumerate() {
            let history_messages = self
                .thread_persistence
                .list_messages_by_thread_id(m.thread.id)
                .await?;

            let team_approver = self
                .company_persistence
                .list_company_team_emails(m.company.id)
                .await
                .unwrap_or_default()
                .into_iter()
                .next();
            let approver_email = m
                .channel
                .participant_emails
                .as_ref()
                .and_then(|participants| {
                    participants
                        .iter()
                        .find(|email| !email.eq_ignore_ascii_case("@public"))
                        .cloned()
                })
                .or(team_approver.map(EmailAddress::from))
                .unwrap_or_default();

            let approval_ctx = AgentRunnerApprovalContext {
                company_id: m.company.id,
                channel_id: m.channel.id,
                channel_name: m.channel.name.clone(),
                channel_slug: m.channel.slug.clone(),
                company_slug: m.company.slug.clone(),
                thread_id: Some(m.thread.id),
                task_id: ingest.task_id,
                approver_email,
            };

            let first_agent = if let Some(ref ap) = self.agent_persistence {
                if let Some(&agent_id) = m.channel.agent_ids.as_ref().and_then(|ids| ids.first()) {
                    if let Some(cached) = agent_cache.get(&agent_id) {
                        cached.clone()
                    } else {
                        let loaded = ap.get_by_id(agent_id).await.ok().flatten();
                        agent_cache.insert(agent_id, loaded.clone());
                        loaded
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let resolved_params_res =
                ResolvedAgentParams::new(Some(&m.company), Some(&m.channel), first_agent.as_ref());
            if idx == 0 {
                primary_resolved_params = resolved_params_res.as_ref().ok().cloned();
                primary_agent = first_agent.clone();
            }

            let sender_clean = parsed.sender.trim();
            let member_key = (m.company.id, sender_clean.to_lowercase());
            let is_team_member = if let Some(cached) = membership_cache.get(&member_key) {
                *cached
            } else {
                let loaded = self
                    .company_persistence
                    .is_company_team_member(m.company.id, sender_clean)
                    .await
                    .unwrap_or(false);
                membership_cache.insert(member_key, loaded);
                loaded
            };

            let is_participant = match &m.channel.participant_emails {
                Some(allowed) if !allowed.is_empty() => {
                    let explicitly_listed = allowed.iter().any(|e| {
                        !e.trim().eq_ignore_ascii_case("@public")
                            && e.eq_ignore_ascii_case(sender_clean)
                    });
                    explicitly_listed || is_team_member
                }
                _ => is_team_member,
            };

            let mut upstream_context = String::new();
            if !agent_outputs.is_empty() {
                for (prev_m, prev_content, _, _) in &agent_outputs {
                    upstream_context.push_str(&format!(
                        "--- Step {step_num}: {name} ({slug}) ---\n{content}\n\n",
                        step_num = prev_m.step_index + 1,
                        name = prev_m.channel.name,
                        slug = prev_m.channel.slug,
                        content = prev_content
                    ));
                }
            }

            if let Some(task_id) = ingest.task_id
                && let Some(outreach_context) =
                    self.task_persistence.get_outreach_context(task_id).await?
            {
                upstream_context.push_str("--- Outreach Progress ---\n");
                upstream_context.push_str(&outreach_context);
                upstream_context.push_str("\n\n");
            }
            let upstream_ctx_opt = if !upstream_context.is_empty() {
                Some(upstream_context)
            } else {
                None
            };

            let runner_res = match &resolved_params_res {
                Ok(resolved_params) => {
                    let mut runner = AgentRunner::new(&parsed.prompt_text, resolved_params)
                        .history(&history_messages)
                        .approval_use_cases(self.approval_use_cases.clone())
                        .approval_context(Some(approval_ctx))
                        .monitoring(self.monitoring.clone())
                        .config(Some(self.config.clone()))
                        .company(Some(m.company.clone()))
                        .skip_spam_guardrail(is_participant)
                        .recipient_role(Some(m.recipient_role))
                        .upstream_pipeline_context(upstream_ctx_opt)
                        .ids(
                            Some(m.company.id),
                            Some(m.channel.id),
                            first_agent.as_ref().map(|a| a.id),
                        );
                    if let (Some(task_id), Some(worker_id)) = (ingest.task_id, effective_worker_id)
                    {
                        let mut thread_references = parsed.references.clone();
                        if let Some(ref in_reply_to) = parsed.in_reply_to
                            && !thread_references.contains(in_reply_to)
                        {
                            thread_references.push(in_reply_to.clone());
                        }
                        runner = runner.outreach_tool(
                            self.task_persistence.clone(),
                            self.channel_persistence.clone(),
                            OutreachToolContext {
                                task_id,
                                worker_id,
                                company_id: m.company.id,
                                channel_id: m.channel.id,
                                channel_name: m.channel.name.clone(),
                                channel_slug: m.channel.slug.clone(),
                                company_slug: m.company.slug.clone(),
                                trigger_message_id: parsed.message_id.clone().into(),
                                thread_references: thread_references
                                    .into_iter()
                                    .map(MessageId::from)
                                    .collect(),
                                hop_count: parsed.hop_count,
                                trace_channels: parsed.trace_channels.clone(),
                                app_domain_name: self.config.app_domain_name.clone(),
                            },
                        );
                    }
                    runner.execute().await
                }
                Err(err) => Err(anyhow::anyhow!("{err}")),
            };

            match runner_res {
                Ok(output) => {
                    if output.disposition == AgentExecutionDisposition::Suspended {
                        info!("Agent execution suspended for task approval or outreach");
                        return Ok(None);
                    }
                    total_prompt_tokens += output.token_usage.prompt_tokens;
                    total_completion_tokens += output.token_usage.completion_tokens;
                    agent_outputs.push((m, output.content, None::<String>, output.metadata));
                }
                Err(err) => {
                    let err_msg = format!("Agent execution failed: {err}");
                    if idx == 0 {
                        primary_execution_error = Some(err_msg.clone());
                    }
                    agent_outputs.push((m, err_msg.clone(), Some(err_msg), None));
                }
            }
        }

        // Concatenate Responses
        let combined_agent_response = if agent_outputs.len() == 1 {
            agent_outputs[0].1.clone()
        } else {
            let mut pieces = Vec::new();
            for (m, resp, _, _) in &agent_outputs {
                let role_label = match m.recipient_role {
                    RecipientRole::To => "TO",
                    RecipientRole::Cc => "CC",
                };
                let header = format!(
                    "[{name} ({slug}@{comp}.{domain}) - {role_label}]",
                    name = m.channel.name,
                    slug = m.channel.slug,
                    comp = m.company.slug,
                    domain = self.config.app_domain_name,
                    role_label = role_label
                );
                pieces.push(format!("{}\n{}", header, resp));
            }
            pieces.join("\n\n--------------------------------------------------\n\n")
        };

        let combined_metadata = if agent_outputs.len() == 1 {
            agent_outputs[0].3.clone()
        } else {
            let mut map = serde_json::Map::new();
            for (i, (m, _, _, meta)) in agent_outputs.iter().enumerate() {
                if let Some(meta_val) = meta {
                    map.insert(
                        format!("step_{}_{}", i + 1, m.channel.slug),
                        meta_val.clone(),
                    );
                }
            }
            if map.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(map))
            }
        };

        let primary_match = &matches[0];
        let primary_channel = &primary_match.channel;
        let primary_company = &primary_match.company;

        let (
            sent_message_id,
            in_reply_to,
            references,
            from_address,
            recipients_to,
            recipients_cc,
            subject,
            email_sent,
        ) = if send_email {
            // Construct Outbound Email
            let mut references_for_outbound = parsed.references.clone();
            if let Some(ref reply_to) = parsed.in_reply_to {
                if !references_for_outbound.contains(reply_to) {
                    references_for_outbound.push(reply_to.clone());
                }
            }

            let mut outbound_cc = parsed.recipients_cc.clone();
            if let Some(ref wf_participants) = primary_channel.participant_emails {
                for p in wf_participants {
                    if !p.eq_ignore_ascii_case("@public")
                        && !p.eq_ignore_ascii_case(&parsed.sender)
                        && !outbound_cc
                            .iter()
                            .any(|recipient| recipient.eq_ignore_ascii_case(p))
                    {
                        outbound_cc.push(p.to_string());
                    }
                }
            }

            let norm_outbound = NormalizedOutboundMessage {
                thread_id: matches[0].thread.id,
                in_reply_to_ref: Some(MessageId::from(parsed.message_id.clone())),
                references: references_for_outbound
                    .iter()
                    .cloned()
                    .map(MessageId::from)
                    .collect(),
                recipients_to: vec![ParticipantIdentity::email(&parsed.sender)],
                recipients_cc: outbound_cc.iter().map(ParticipantIdentity::email).collect(),
                subject: parsed.subject.clone(),
                content: combined_agent_response.clone(),
                attachments: vec![],
                protocol: ChannelType::Email,
                channel_id: primary_channel.id,
                hop_count: parsed.hop_count,
                trace_channels: parsed.trace_channels.clone(),
            };

            let outbound_email = OutboundEmail {
                channel_id: primary_channel.id,
                channel_name: primary_channel.name.clone(),
                channel_slug: primary_channel.slug.clone(),
                company_slug: primary_company.slug.clone(),
                trigger_message_id: parsed.message_id.clone().into(),
                thread_references: references_for_outbound
                    .into_iter()
                    .map(MessageId::from)
                    .collect(),
                recipient_to: parsed.sender.clone().into(),
                recipients_cc: outbound_cc.into_iter().map(EmailAddress::from).collect(),
                subject: parsed.subject.clone(),
                body_text: combined_agent_response.clone(),
                hop_count: parsed.hop_count,
                trace_channels: parsed.trace_channels.clone(),
            };

            if let (Some(task_id), Some(worker_id)) = (ingest.task_id, effective_worker_id) {
                let renewed = self
                    .task_persistence
                    .renew_task_lease(
                        task_id,
                        worker_id,
                        chrono::Utc::now().naive_utc()
                            + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                    )
                    .await?;
                if !renewed {
                    return Err(crate::app_error::AppError::Internal(
                        "Task lease was lost before outbound dispatch".into(),
                    ));
                }
            }
            let idempotency_key = ingest
                .task_id
                .map(|task_id| format!("task:{task_id}:agent-reply"));
            let sent_result = if let Some(prepared) = self
                .prepare_internal_channel_delivery(
                    outbound_email.clone(),
                    idempotency_key.as_deref(),
                )
                .await?
            {
                let internal_ingest = self.ingest_prepared_internal_message(&prepared).await?;
                if !internal_ingest.accepted
                    && internal_ingest.reason.as_deref()
                        != Some("Duplicate Message-ID already processed")
                {
                    return Err(AppError::Internal(internal_ingest.reason.unwrap_or_else(
                        || "Internal channel delivery was rejected".into(),
                    )));
                }
                info!(
                    "Delivered agent response {} through trusted internal channel transport",
                    prepared.outbound_message_id
                );
                prepared
            } else if let Some(ref key) = idempotency_key {
                OutboundDispatcher::send_idempotent(&self.config, outbound_email, key).await?
            } else {
                OutboundDispatcher::send(&self.config, outbound_email).await?
            };
            if let Some(adapter) = self.egress_registry.get(&norm_outbound.protocol) {
                let _ = adapter; // Registered egress adapter present
            }
            (
                sent_result.outbound_message_id.into_string(),
                sent_result.in_reply_to.into_string(),
                sent_result
                    .references
                    .into_iter()
                    .map(MessageId::into_string)
                    .collect(),
                sent_result.from_address.into_string(),
                sent_result
                    .recipients_to
                    .into_iter()
                    .map(EmailAddress::into_string)
                    .collect(),
                sent_result
                    .recipients_cc
                    .into_iter()
                    .map(EmailAddress::into_string)
                    .collect(),
                sent_result.subject,
                true,
            )
        } else {
            let outbound_uuid = Uuid::new_v4();
            let simulated_msg_id = format!(
                "<simulated-test-{}@{}>",
                outbound_uuid, self.config.app_domain_name
            );
            let from_email = format!(
                "{}@{}.{}",
                primary_channel.slug, primary_company.slug, self.config.app_domain_name
            );
            info!(
                "Simulation test mode (Run_Test): Skipped SMTP email dispatch for Message-ID {}",
                simulated_msg_id
            );
            (
                simulated_msg_id,
                parsed.message_id.clone(),
                parsed.references.clone(),
                from_email,
                vec![parsed.sender.clone()],
                parsed.recipients_cc.clone(),
                if parsed.subject.to_lowercase().starts_with("re:") {
                    parsed.subject.clone()
                } else {
                    format!("Re: {}", parsed.subject)
                },
                false,
            )
        };

        // Save Outbound Agent Message into each matched thread
        for m in &matches {
            let outbound_msg_id = Uuid::new_v4();
            let outbound_message = Message {
                id: outbound_msg_id,
                thread_id: m.thread.id,
                message_id: MessageId::from(sent_message_id.clone()),
                in_reply_to: Some(MessageId::from(in_reply_to.clone())),
                references_list: references.iter().cloned().map(MessageId::from).collect(),
                sender: EmailAddress::from(from_address.clone()),
                recipients_to: recipients_to.iter().cloned().map(EmailAddress::from).collect(),
                recipients_cc: recipients_cc.iter().cloned().map(EmailAddress::from).collect(),
                subject: subject.clone(),
                clean_text_body: combined_agent_response.clone(),
                raw_text_body: None,
                raw_html_body: None,
                attachments: None,
                direction: MessageDirection::Outbound,
                role: MessageRole::Agent,
                thread_index: None,
                created_at: chrono::Utc::now().naive_utc(),
            };

            let _ = self
                .thread_persistence
                .create_message(&outbound_message)
                .await?;
        }

        if let Some(task_id) = ingest.task_id {
            let exec_params = match &primary_resolved_params {
                Some(p) => {
                    let mut cfg = p.config().clone();
                    scrub_json_secrets(Some(&mut cfg));
                    serde_json::json!({
                        "provider": p.provider(),
                        "model": p.model(),
                        "agent_id": primary_agent.as_ref().map(|a| a.id),
                        "agent_name": primary_agent.as_ref().map(|a| a.name.as_str()),
                        "prompt": parsed.prompt_text,
                        "config": cfg,
                        "executed_at": chrono::Utc::now().naive_utc().to_string(),
                    })
                }
                None => serde_json::json!({
                    "prompt": parsed.prompt_text,
                    "executed_at": chrono::Utc::now().naive_utc().to_string(),
                }),
            };

            let mut updated_payload = durable_ingest_payload(ingest);
            if let Some(obj) = updated_payload.as_object_mut() {
                obj.insert("execution_parameters".to_string(), exec_params);
                let mut exec_res = serde_json::json!({
                    "response": combined_agent_response.clone(),
                    "email_sent": email_sent,
                    "outbound_message_id": sent_message_id.clone(),
                    "error": primary_execution_error.clone(),
                    "token_usage": TokenUsage::new(total_prompt_tokens, total_completion_tokens),
                });
                if let Some(ref meta) = combined_metadata {
                    exec_res
                        .as_object_mut()
                        .unwrap()
                        .insert("metadata".to_string(), meta.clone());
                }
                obj.insert("execution_result".to_string(), exec_res);
            }
            if let Some(worker_id) = effective_worker_id {
                let _ = self
                    .task_persistence
                    .update_claimed_task_payload(task_id, worker_id, updated_payload)
                    .await;
            } else {
                let _ = self
                    .task_persistence
                    .update_task_payload(task_id, updated_payload)
                    .await;
            }
        }

        if let Some(ref err_msg) = primary_execution_error {
            if let (Some(task_id), Some(worker_id)) = (ingest.task_id, direct_worker_id) {
                let next_run = chrono::Utc::now().naive_utc();
                let _ = self
                    .task_persistence
                    .mark_task_failed(task_id, worker_id, err_msg, next_run, true)
                    .await;
            }
            return Err(crate::app_error::AppError::Internal(err_msg.clone()));
        }

        if let Some(task_id) = ingest.task_id {
            self.task_persistence.complete_outreach(task_id).await?;
        }
        if let (Some(task_id), Some(worker_id)) = (ingest.task_id, direct_worker_id) {
            let _ = self
                .task_persistence
                .mark_task_completed(task_id, worker_id)
                .await;
        }

        Ok(Some(AgentExecutionResult {
            outbound_message_id: Some(sent_message_id),
            agent_response: combined_agent_response,
            email_sent,
            token_usage: Some(TokenUsage::new(
                total_prompt_tokens,
                total_completion_tokens,
            )),
            metadata: combined_metadata,
        }))
    }

    pub async fn execute_simulation(
        &self,
        raw_payload: RawInboundPayload,
        mode: SimulationMode,
    ) -> AppResult<SimulationExecutionResult> {
        let ingest = self.ingest_and_save_inbound_message(raw_payload).await?;

        if !ingest.accepted {
            return Ok(SimulationExecutionResult {
                ingest_result: ingest,
                agent_execution: None,
                simulation_mode: mode,
            });
        }

        let send_email = mode == SimulationMode::Run;
        let agent_execution = match self.execute_agent_and_dispatch(&ingest, send_email).await {
            Ok(exec) => exec,
            Err(err) => Some(AgentExecutionResult {
                outbound_message_id: None,
                agent_response: format!("{err}"),
                email_sent: false,
                token_usage: None,
                metadata: None,
            }),
        };

        Ok(SimulationExecutionResult {
            ingest_result: ingest,
            agent_execution,
            simulation_mode: mode,
        })
    }

    pub async fn handle_bounce_dispatch(&self, ingest: &InboundIngestResult) {
        if let Some(ref bounce) = ingest.bounce_info {
            if let Some(adapter) = self.egress_registry.get(&ChannelType::Email) {
                let _ = adapter.dispatch_bounce(bounce).await;
            } else {
                let bounce_body = format_bounce_email_body(bounce, &self.config.app_domain_name);
                let _ = OutboundDispatcher::send_bounce(
                    &self.config,
                    &bounce.recipient_to,
                    &bounce.original_subject,
                    &bounce_body,
                )
                .await;
            }
        }
    }

    pub async fn process_and_dispatch_email(
        &self,
        raw_payload: RawInboundPayload,
    ) -> AppResult<ProcessEmailResult> {
        let ingest = self.ingest_and_save_inbound_message(raw_payload).await?;
        if !ingest.accepted {
            self.handle_bounce_dispatch(&ingest).await;
            return Ok(ProcessEmailResult {
                processed: false,
                reason: ingest.reason,
                thread_id: None,
                inbound_message_id: None,
                outbound_message_id: None,
            });
        }

        let thread_id = ingest.thread.as_ref().map(|t| t.id);
        let inbound_message_id = ingest
            .inbound_message
            .as_ref()
            .map(|m| m.message_id.to_string());

        let agent_res = self.execute_agent_and_dispatch(&ingest, true).await?;
        let outbound_msg_id = agent_res.and_then(|r| r.outbound_message_id);

        Ok(ProcessEmailResult {
            processed: true,
            reason: None,
            thread_id,
            inbound_message_id,
            outbound_message_id: outbound_msg_id,
        })
    }

    pub async fn get_thread(&self, thread_id: Uuid) -> AppResult<Option<Thread>> {
        self.thread_persistence.get_thread_by_id(thread_id).await
    }

    pub async fn list_channel_threads(
        &self,
        channel_id: Uuid,
        before: Option<(NaiveDateTime, Uuid)>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        self.thread_persistence
            .list_threads_by_channel_id(channel_id, before, limit)
            .await
    }

    pub async fn get_thread_history(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
        self.thread_persistence
            .list_messages_by_thread_id(thread_id)
            .await
    }

    pub async fn find_outbound_reply(
        &self,
        thread_id: Uuid,
        in_reply_to: &MessageId,
    ) -> AppResult<Option<Message>> {
        self.thread_persistence
            .find_outbound_reply(thread_id, in_reply_to)
            .await
    }

    pub async fn save_message(&self, message: &Message) -> AppResult<Message> {
        self.thread_persistence.create_message(message).await
    }

    pub async fn hydrate_ingest_configuration(
        &self,
        ingest: &mut InboundIngestResult,
    ) -> AppResult<()> {
        if let Some(company) = ingest.company.as_mut() {
            *company = self
                .company_persistence
                .get_by_id(company.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal("Task company no longer exists".into())
                })?;
        }
        if let Some(channel) = ingest.channel.as_mut() {
            let current = self
                .channel_persistence
                .get_by_id(channel.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal("Task channel no longer exists".into())
                })?;
            if ingest
                .company
                .as_ref()
                .is_some_and(|company| company.id != current.company_id)
            {
                return Err(crate::app_error::AppError::Internal(
                    "Task channel does not belong to its company".into(),
                ));
            }
            *channel = current;
        }
        for channel_match in &mut ingest.channel_matches {
            let company = self
                .company_persistence
                .get_by_id(channel_match.company.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal(
                        "Task target company no longer exists".into(),
                    )
                })?;
            let channel = self
                .channel_persistence
                .get_by_id(channel_match.channel.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal(
                        "Task target channel no longer exists".into(),
                    )
                })?;
            if channel.company_id != company.id {
                return Err(crate::app_error::AppError::Internal(
                    "Task target channel does not belong to its company".into(),
                ));
            }
            let thread = self
                .thread_persistence
                .get_thread_by_id(channel_match.thread.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal(
                        "Task target thread no longer exists".into(),
                    )
                })?;
            if thread.channel_id != channel.id {
                return Err(crate::app_error::AppError::Internal(
                    "Task target thread does not belong to its channel".into(),
                ));
            }
            channel_match.company = company;
            channel_match.channel = channel;
            channel_match.thread = thread;
        }
        Ok(())
    }

    pub async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<crate::entities::task::TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
        self.task_persistence
            .list_company_tasks(company_id, channel_id, status, sort_asc)
            .await
    }

    pub async fn list_company_tasks_page(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<crate::entities::task::TaskStatus>,
        sort_asc: bool,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
        self.task_persistence
            .list_company_tasks_page(company_id, channel_id, status, sort_asc, offset, limit)
            .await
    }

    pub async fn get_task_persistence(&self) -> Arc<dyn TaskPersistence> {
        self.task_persistence.clone()
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecipientRole {
    To,
    Cc,
}

impl RecipientRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecipientRole::To => "to",
            RecipientRole::Cc => "cc",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMatch {
    pub company: Company,
    pub channel: Channel,
    pub thread: Thread,
    pub inbound_message: Message,
    pub recipient_role: RecipientRole,
    #[serde(default)]
    pub step_index: usize,
    #[serde(default)]
    pub total_steps: usize,
}

fn durable_ingest_payload(ingest: &InboundIngestResult) -> serde_json::Value {
    let mut durable = ingest.clone();
    if let Some(company) = durable.company.as_mut() {
        company.api_key = None;
    }
    if let Some(channel) = durable.channel.as_mut() {
        channel.api_key = None;
        scrub_json_secrets(channel.channel_config.as_mut());
    }
    for channel_match in &mut durable.channel_matches {
        channel_match.company.api_key = None;
        channel_match.channel.api_key = None;
        scrub_json_secrets(channel_match.channel.channel_config.as_mut());
    }
    serde_json::to_value(durable).unwrap_or_default()
}

fn scrub_json_secrets(value: Option<&mut serde_json::Value>) {
    let Some(value) = value else {
        return;
    };
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let normalized = key.to_ascii_lowercase();
                if matches!(
                    normalized.as_str(),
                    "api_key"
                        | "apikey"
                        | "access_token"
                        | "token"
                        | "secret"
                        | "secret_key"
                        | "private_key"
                        | "app_key"
                        | "app_secret"
                        | "auth"
                        | "authorization"
                        | "password"
                        | "bearer"
                ) {
                    *value = serde_json::Value::Null;
                } else {
                    scrub_json_secrets(Some(value));
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                scrub_json_secrets(Some(value));
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BounceSuggestion {
    pub invalid_slug: ChannelSlug,
    pub suggestions: Vec<ChannelSlug>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BounceInfo {
    pub recipient_to: EmailAddress,
    pub company_slug: Option<CompanySlug>,
    pub invalid_slugs: Vec<ChannelSlug>,
    pub suggestions: Vec<BounceSuggestion>,
    pub original_subject: String,
}

pub fn format_bounce_email_body(bounce: &BounceInfo, app_domain: &str) -> String {
    let mut body = String::new();
    body.push_str("Undeliverable Mail Notification\n\n");
    body.push_str("Your email could not be delivered because one or more channel addresses were not found or misspelled.\n\n");

    if let Some(ref comp) = bounce.company_slug {
        body.push_str(&format!("Company Domain: {comp}.{app_domain}\n\n"));
    }

    body.push_str("Details:\n");
    for s in &bounce.suggestions {
        let domain_part = bounce
            .company_slug
            .as_deref()
            .map(|c| format!("{c}.{app_domain}"))
            .unwrap_or_else(|| app_domain.to_string());

        body.push_str(&format!(
            "  - Invalid Channel Slug: '{}@{}'\n",
            s.invalid_slug, domain_part
        ));
        if !s.suggestions.is_empty() {
            body.push_str("    Did you mean:\n");
            for sug in &s.suggestions {
                body.push_str(&format!("      * {sug}@{domain_part}\n"));
            }
        } else {
            body.push_str("    No similar channel suggestions found.\n");
        }
        body.push('\n');
    }

    body.push_str("Please check the channel address and try sending your email again.\n");
    body
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundIngestResult {
    pub accepted: bool,
    pub reason: Option<String>,
    pub thread: Option<Thread>,
    pub inbound_message: Option<Message>,
    pub company: Option<Company>,
    pub channel: Option<Channel>,
    pub parsed_email: Option<ParsedEmail>,
    pub normalized_message: Option<NormalizedInboundMessage>,
    pub task_id: Option<Uuid>,
    #[serde(default)]
    pub channel_matches: Vec<ChannelMatch>,
    #[serde(default)]
    pub bounce_info: Option<BounceInfo>,
}

impl InboundIngestResult {
    pub fn rejected(reason: &str) -> Self {
        Self {
            accepted: false,
            reason: Some(reason.to_string()),
            thread: None,
            inbound_message: None,
            company: None,
            channel: None,
            parsed_email: None,
            normalized_message: None,
            task_id: None,
            channel_matches: Vec::new(),
            bounce_info: None,
        }
    }

    pub fn rejected_with_bounce(reason: &str, bounce: BounceInfo) -> Self {
        Self {
            accepted: false,
            reason: Some(reason.to_string()),
            thread: None,
            inbound_message: None,
            company: None,
            channel: None,
            parsed_email: None,
            normalized_message: None,
            task_id: None,
            channel_matches: Vec::new(),
            bounce_info: Some(bounce),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulationMode {
    Verify,
    RunTest,
    Run,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutionResult {
    pub outbound_message_id: Option<String>,
    pub agent_response: String,
    pub email_sent: bool,
    pub token_usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationExecutionResult {
    pub ingest_result: InboundIngestResult,
    pub agent_execution: Option<AgentExecutionResult>,
    pub simulation_mode: SimulationMode,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProcessEmailResult {
    pub processed: bool,
    pub reason: Option<String>,
    pub thread_id: Option<Uuid>,
    pub inbound_message_id: Option<String>,
    pub outbound_message_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::channel::Channel;
    use crate::use_cases::channel::ChannelPersistence;
    use chrono::Utc;
    use std::sync::Mutex;

    fn internal_test_config() -> Arc<AppConfig> {
        Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "smtp.invalid".to_string(),
            smtp_port: 2525,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        })
    }

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
        team_members: Mutex<Vec<(Uuid, String)>>,
    }

    impl MockCompanyPersistence {
        fn new(companies: Vec<Company>) -> Self {
            Self {
                companies: Mutex::new(companies),
                team_members: Mutex::new(Vec::new()),
            }
        }

        fn with_team_members(companies: Vec<Company>, members: Vec<(Uuid, String)>) -> Self {
            Self {
                companies: Mutex::new(companies),
                team_members: Mutex::new(members),
            }
        }
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(
            &self,
            _user_id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
            unimplemented!()
        }
        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|company| company.id == id)
                .cloned())
        }
        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.slug == slug)
                .cloned())
        }
        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
            unimplemented!()
        }
        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
        async fn is_company_team_member(&self, company_id: Uuid, email: &str) -> AppResult<bool> {
            let members = self.team_members.lock().unwrap();
            let clean = email.trim().to_lowercase();
            if members.is_empty() {
                if clean.contains("spammer")
                    || clean.contains("unauthorized")
                    || clean.contains("evil")
                    || clean.contains("notallowed")
                {
                    return Ok(false);
                }
                return Ok(true);
            }
            Ok(members
                .iter()
                .any(|(cid, e)| *cid == company_id && e.eq_ignore_ascii_case(&clean)))
        }
        async fn list_company_team_emails(&self, company_id: Uuid) -> AppResult<Vec<String>> {
            let members = self.team_members.lock().unwrap();
            Ok(members
                .iter()
                .filter(|(cid, _)| *cid == company_id)
                .map(|(_, e)| e.clone())
                .collect())
        }
    }

    struct MockChannelPersistence {
        channels: Mutex<Vec<Channel>>,
    }

    #[async_trait]
    impl ChannelPersistence for MockChannelPersistence {
        async fn create(
            &self,
            _company_id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _participant_emails: Option<Vec<String>>,
            _agent_ids: Option<Vec<Uuid>>,
            _channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|channel| channel.id == id)
                .cloned())
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &CompanySlug,
            channel_slug: &ChannelSlug,
        ) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|w| &w.slug == channel_slug)
                .cloned())
        }
        async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .filter(|w| w.company_id == company_id)
                .cloned()
                .collect())
        }
        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _participant_emails: Option<Vec<String>>,
            _agent_ids: Option<Vec<Uuid>>,
            _channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockThreadPersistence {
        threads: Mutex<Vec<Thread>>,
        messages: Mutex<Vec<Message>>,
    }

    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(
            &self,
            channel_id: Uuid,
            subject: &str,
            participant_emails: &[EmailAddress],
        ) -> AppResult<Thread> {
            let thread = Thread {
                id: Uuid::new_v4(),
                channel_id,
                subject: subject.to_string(),
                participant_emails: participant_emails.to_vec(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
            };
            self.threads.lock().unwrap().push(thread.clone());
            Ok(thread)
        }

        async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>> {
            Ok(self
                .threads
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned())
        }

        async fn list_threads_by_channel_id(
            &self,
            channel_id: Uuid,
            before: Option<(chrono::NaiveDateTime, Uuid)>,
            limit: usize,
        ) -> AppResult<Vec<Thread>> {
            let mut threads: Vec<_> = self
                .threads
                .lock()
                .unwrap()
                .iter()
                .filter(|thread| thread.channel_id == channel_id)
                .filter(|thread| {
                    before.is_none_or(|cursor| (thread.updated_at, thread.id) < cursor)
                })
                .cloned()
                .collect();
            threads.sort_by_key(|thread| std::cmp::Reverse((thread.updated_at, thread.id)));
            threads.truncate(limit);
            Ok(threads)
        }

        async fn update_thread_participants(
            &self,
            id: Uuid,
            participant_emails: &[EmailAddress],
        ) -> AppResult<Thread> {
            let mut list = self.threads.lock().unwrap();
            let thread = list.iter_mut().find(|t| t.id == id).unwrap();
            thread.participant_emails = participant_emails.to_vec();
            Ok(thread.clone())
        }

        async fn find_thread_by_message_ids(
            &self,
            channel_id: Uuid,
            message_ids: &[MessageId],
        ) -> AppResult<Option<Thread>> {
            let thread_id = {
                let msgs = self.messages.lock().unwrap();
                msgs.iter()
                    .find(|m| message_ids.contains(&m.message_id))
                    .map(|m| m.thread_id)
            };
            if let Some(tid) = thread_id {
                return Ok(self
                    .get_thread_by_id(tid)
                    .await?
                    .filter(|thread| thread.channel_id == channel_id));
            }
            Ok(None)
        }

        async fn find_thread_by_thread_index(
            &self,
            channel_id: Uuid,
            thread_index_prefix: &ThreadIndex,
        ) -> AppResult<Option<Thread>> {
            let thread_id = {
                let msgs = self.messages.lock().unwrap();
                msgs.iter()
                    .find(|m| {
                        m.thread_index
                            .as_deref()
                            .unwrap_or_default()
                            .starts_with(thread_index_prefix.as_str())
                    })
                    .map(|m| m.thread_id)
            };
            if let Some(tid) = thread_id {
                return Ok(self
                    .get_thread_by_id(tid)
                    .await?
                    .filter(|thread| thread.channel_id == channel_id));
            }
            Ok(None)
        }

        async fn count_recent_messages(
            &self,
            thread_id: Uuid,
            _duration_secs: i64,
        ) -> AppResult<usize> {
            let msgs = self.messages.lock().unwrap();
            Ok(msgs.iter().filter(|m| m.thread_id == thread_id).count())
        }

        async fn create_message(&self, message: &Message) -> AppResult<Message> {
            self.messages.lock().unwrap().push(message.clone());
            Ok(message.clone())
        }

        async fn get_message_by_message_id(
            &self,
            _company_id: Uuid,
            message_id: &MessageId,
        ) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|m| &m.message_id == message_id)
                .cloned())
        }

        async fn find_outbound_reply(
            &self,
            thread_id: Uuid,
            in_reply_to: &MessageId,
        ) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|message| {
                    message.thread_id == thread_id
                        && message.direction == MessageDirection::Outbound
                        && message.in_reply_to.as_ref() == Some(in_reply_to)
                })
                .cloned())
        }

        async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.thread_id == thread_id)
                .cloned()
                .collect())
        }
    }

    struct MockTaskPersistence {
        tasks: Mutex<Vec<crate::entities::task::BackgroundTask>>,
    }

    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
        async fn find_correlated_outreach_reply(
            &self,
            company_id: Uuid,
            channel_id: Uuid,
            thread_id: Uuid,
            sender: &str,
            references: &[MessageId],
        ) -> AppResult<Option<crate::entities::outreach::OutreachReplyMatch>> {
            let tasks = self.tasks.lock().unwrap();
            Ok(tasks.iter().find_map(|task| {
                let outreach = task.payload.get("test_outreach")?;
                let target = outreach.get("target_email")?.as_str()?;
                let outbound_message_id = outreach.get("outbound_message_id")?.as_str()?;
                (task.company_id == company_id
                    && task.channel_id == channel_id
                    && task.thread_id == Some(thread_id)
                    && target.eq_ignore_ascii_case(sender)
                    && references.iter().any(|value| value == outbound_message_id))
                .then(|| crate::entities::outreach::OutreachReplyMatch {
                    outreach_id: outreach
                        .get("outreach_id")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|value| Uuid::parse_str(value).ok())
                        .unwrap(),
                    task_id: task.id,
                    target_email: target.into(),
                })
            }))
        }

        async fn record_outreach_reply(
            &self,
            matched: &crate::entities::outreach::OutreachReplyMatch,
            _response_message_id: Uuid,
        ) -> AppResult<crate::entities::outreach::OutreachProgress> {
            let mut tasks = self.tasks.lock().unwrap();
            let task = tasks
                .iter_mut()
                .find(|task| task.id == matched.task_id)
                .unwrap();
            task.status = crate::entities::task::TaskStatus::Pending;
            Ok(crate::entities::outreach::OutreachProgress {
                id: matched.outreach_id,
                task_id: matched.task_id,
                status: crate::entities::outreach::OutreachStatus::ThresholdMet,
                required_threshold_percent: 100.0,
                target_count: 1,
                response_count: 1,
                required_response_count: 1,
                expires_at: Utc::now().naive_utc() + chrono::Duration::hours(1),
                suspended: false,
            })
        }

        async fn enqueue_task(
            &self,
            company_id: Uuid,
            channel_id: Uuid,
            thread_id: Option<Uuid>,
            task_type: &str,
            payload: serde_json::Value,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            let task = crate::entities::task::BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id,
                task_type: task_type.to_string(),
                status: crate::entities::task::TaskStatus::Pending,
                payload,
                retry_count: 0,
                max_retries: 3,
                last_error: None,
                worker_id: None,
                locked_at: None,
                lock_expires_at: None,
                run_at: Utc::now().naive_utc(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
            };
            self.tasks.lock().unwrap().push(task.clone());
            Ok(task)
        }

        async fn get_task_by_id(
            &self,
            id: Uuid,
        ) -> AppResult<Option<crate::entities::task::BackgroundTask>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned())
        }

        async fn update_task_payload(&self, id: Uuid, payload: serde_json::Value) -> AppResult<()> {
            let mut list = self.tasks.lock().unwrap();
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.payload = payload;
            }
            Ok(())
        }

        async fn claim_pending_tasks(
            &self,
            worker_id: Uuid,
            lock_expires_at: chrono::NaiveDateTime,
            limit: i64,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            let now = Utc::now().naive_utc();
            let mut tasks = self.tasks.lock().unwrap();
            let mut claimed = Vec::new();
            for task in tasks
                .iter_mut()
                .filter(|task| {
                    (task.status == crate::entities::task::TaskStatus::Pending
                        && task.run_at <= now)
                        || (task.status == crate::entities::task::TaskStatus::Processing
                            && task.lock_expires_at.is_none_or(|expires| expires <= now))
                })
                .take(limit as usize)
            {
                task.status = crate::entities::task::TaskStatus::Processing;
                task.worker_id = Some(worker_id);
                task.locked_at = Some(now);
                task.lock_expires_at = Some(lock_expires_at);
                claimed.push(task.clone());
            }
            Ok(claimed)
        }

        async fn claim_task(
            &self,
            id: Uuid,
            worker_id: Uuid,
            lock_expires_at: chrono::NaiveDateTime,
        ) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now().naive_utc();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == id
                    && t.status == crate::entities::task::TaskStatus::Pending
                    && t.run_at <= now
            }) {
                t.status = crate::entities::task::TaskStatus::Processing;
                t.worker_id = Some(worker_id);
                t.locked_at = Some(now);
                t.lock_expires_at = Some(lock_expires_at);
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn mark_task_completed(&self, id: Uuid, worker_id: Uuid) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now().naive_utc();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == id
                    && t.status == crate::entities::task::TaskStatus::Processing
                    && t.worker_id == Some(worker_id)
                    && t.lock_expires_at.is_some_and(|expires| expires > now)
            }) {
                t.status = crate::entities::task::TaskStatus::Completed;
                t.worker_id = None;
                t.locked_at = None;
                t.lock_expires_at = None;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn mark_task_failed(
            &self,
            id: Uuid,
            worker_id: Uuid,
            error_msg: &str,
            next_run_at: chrono::NaiveDateTime,
            is_dead_letter: bool,
        ) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now().naive_utc();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == id
                    && t.status == crate::entities::task::TaskStatus::Processing
                    && t.worker_id == Some(worker_id)
                    && t.lock_expires_at.is_some_and(|expires| expires > now)
            }) {
                t.last_error = Some(error_msg.to_string());
                t.retry_count += 1;
                t.run_at = next_run_at;
                t.status = if is_dead_letter {
                    crate::entities::task::TaskStatus::DeadLetter
                } else {
                    crate::entities::task::TaskStatus::Pending
                };
                t.worker_id = None;
                t.locked_at = None;
                t.lock_expires_at = None;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn stop_task(&self, id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list
                .iter_mut()
                .find(|t| {
                    t.id == id
                        && matches!(
                            t.status,
                            crate::entities::task::TaskStatus::Pending
                                | crate::entities::task::TaskStatus::Processing
                                | crate::entities::task::TaskStatus::PendingApproval
                                | crate::entities::task::TaskStatus::WaitingForThirdPartyReply
                                | crate::entities::task::TaskStatus::Failed
                        )
                })
                .unwrap();
            t.status = crate::entities::task::TaskStatus::Stopped;
            t.worker_id = None;
            t.locked_at = None;
            t.lock_expires_at = None;
            Ok(t.clone())
        }

        async fn resume_task(&self, id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list
                .iter_mut()
                .find(|t| {
                    t.id == id
                        && matches!(
                            t.status,
                            crate::entities::task::TaskStatus::Stopped
                                | crate::entities::task::TaskStatus::PendingApproval
                                | crate::entities::task::TaskStatus::WaitingForThirdPartyReply
                                | crate::entities::task::TaskStatus::Failed
                        )
                })
                .unwrap();
            t.status = crate::entities::task::TaskStatus::Pending;
            t.run_at = Utc::now().naive_utc();
            t.worker_id = None;
            t.locked_at = None;
            t.lock_expires_at = None;
            Ok(t.clone())
        }

        async fn update_task_status(
            &self,
            id: Uuid,
            status: crate::entities::task::TaskStatus,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list
                .iter_mut()
                .find(|t| {
                    t.id == id
                        && match status {
                            crate::entities::task::TaskStatus::PendingApproval => matches!(
                                t.status,
                                crate::entities::task::TaskStatus::Processing
                                    | crate::entities::task::TaskStatus::WaitingForThirdPartyReply
                            ),
                            crate::entities::task::TaskStatus::WaitingForThirdPartyReply => {
                                matches!(
                                    t.status,
                                    crate::entities::task::TaskStatus::Processing
                                        | crate::entities::task::TaskStatus::PendingApproval
                                )
                            }
                            _ => false,
                        }
                })
                .unwrap();
            t.status = status;
            t.worker_id = None;
            t.locked_at = None;
            t.lock_expires_at = None;
            Ok(t.clone())
        }

        async fn list_company_tasks(
            &self,
            company_id: Uuid,
            channel_id: Option<Uuid>,
            status: Option<crate::entities::task::TaskStatus>,
            _sort_asc: bool,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.company_id == company_id)
                .filter(|t| channel_id.map_or(true, |w_id| t.channel_id == w_id))
                .filter(|t| status.as_ref().map_or(true, |s| t.status == *s))
                .cloned()
                .collect())
        }
    }

    #[tokio::test]
    async fn test_inter_channel_hop_limit_rejection() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let source_channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![
                Channel {
                    id: channel_id,
                    company_id,
                    name: "Inbound Flow".to_string(),
                    slug: "inbound".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: Some(vec![Uuid::new_v4()]),
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: source_channel_id,
                    company_id,
                    name: "Source Flow".to_string(),
                    slug: "source".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: Some(vec![Uuid::new_v4()]),
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
            ]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let prepared = thread_use_cases
            .prepare_internal_channel_delivery(
                OutboundEmail {
                    channel_id: source_channel_id,
                    channel_name: "Source Flow".to_string(),
                    channel_slug: "source".into(),
                    company_slug: "acme".into(),
                    trigger_message_id: "<source@example.com>".into(),
                    thread_references: Vec::new(),
                    recipient_to: "inbound@acme.mailagents.com".into(),
                    recipients_cc: Vec::new(),
                    subject: "Test Inter Channel".to_string(),
                    body_text: "Hello".to_string(),
                    hop_count: MAX_CHANNEL_HOPS - 1,
                    trace_channels: Vec::new(),
                },
                Some("hop-limit-test"),
            )
            .await
            .unwrap()
            .unwrap();
        let result = thread_use_cases
            .ingest_prepared_internal_message(&prepared)
            .await
            .unwrap();
        assert!(!result.accepted);
        assert_eq!(
            result.reason.as_deref(),
            Some("Max inter-channel hop count reached")
        );
    }

    #[tokio::test]
    async fn test_spf_authentication_failure_rejection() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Inbound Flow".to_string(),
                slug: "inbound".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: Some(vec!["@public".into()]),
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let raw_payload = RawInboundPayload {
            to: "restricted@acme.mailagents.com".to_string(),
            from: "forged@acme.mailagents.com".to_string(),
            subject: Some("Spoofed email".to_string()),
            text: Some("Hello".to_string()),
            headers: Some(format!("X-MailAgents-Channel-ID: {channel_id}\n")),
            spf: Some("fail".to_string()),
            ..Default::default()
        };

        let result = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();
        assert!(!result.accepted);
        assert_eq!(result.reason.as_deref(), Some("SPF authentication failed"));
    }

    #[tokio::test]
    async fn test_high_spam_score_rejection() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Inbound Flow".to_string(),
                slug: "inbound".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: Some(vec!["@public".into()]),
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let raw_payload = RawInboundPayload {
            to: "inbound@acme.mailagents.com".to_string(),
            from: "spammer@external.com".to_string(),
            subject: Some("Buy Cheap Rolex".to_string()),
            text: Some("Spam message body".to_string()),
            spam_score: Some(8.5),
            ..Default::default()
        };

        let result = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();
        assert!(!result.accepted);
        assert_eq!(
            result.reason.as_deref(),
            Some("Spam score threshold exceeded")
        );
    }

    #[tokio::test]
    async fn test_dmarc_authentication_failure_rejection() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Inbound Flow".to_string(),
                slug: "inbound".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let raw_payload = RawInboundPayload {
            to: "inbound@acme.mailagents.com".to_string(),
            from: "spoofed@external.com".to_string(),
            subject: Some("Spoofed email".to_string()),
            text: Some("Hello".to_string()),
            dmarc: Some("fail".to_string()),
            ..Default::default()
        };

        let result = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();
        assert!(!result.accepted);
        assert_eq!(
            result.reason.as_deref(),
            Some("DMARC authentication failed")
        );
    }

    #[tokio::test]
    async fn test_unauthorized_sender_blocked_before_spam_checks() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Restricted Flow".to_string(),
                slug: "restricted".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: Some(vec!["alice@example.com".into()]),
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let raw_payload = RawInboundPayload {
            to: "restricted@acme.mailagents.com".to_string(),
            from: "unauthorized@evil.com".to_string(),
            subject: Some("Hello".to_string()),
            text: Some("Test message".to_string()),
            spam_score: Some(10.0), // high spam score
            ..Default::default()
        };

        let result = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();

        assert!(!result.accepted);
        assert_eq!(
            result.reason.as_deref(),
            Some("Sender unauthorized for channel")
        );
    }

    #[tokio::test]
    async fn test_participant_sender_bypasses_spam_checks() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Restricted Flow".to_string(),
                slug: "restricted".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: Some(vec!["alice@example.com".into()]),
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        // High spam score payload from a registered participant
        let raw_payload = RawInboundPayload {
            to: "restricted@acme.mailagents.com".to_string(),
            from: "alice@example.com".to_string(),
            subject: Some("Urgent update".to_string()),
            text: Some("Meeting notes".to_string()),
            spam_score: Some(99.0), // extreme spam score that would normally be rejected
            ..Default::default()
        };

        let result = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();

        assert!(result.accepted);
        assert!(result.thread.is_some());
    }

    #[tokio::test]
    async fn test_channel_in_cc_resolves_properly() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Support Flow".to_string(),
                slug: "support".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let raw_payload = RawInboundPayload {
            to: "user@example.com".to_string(),
            cc: Some("support@acme.mailagents.com".to_string()),
            from: "customer@client.com".to_string(),
            subject: Some("Need help".to_string()),
            text: Some("Can someone assist?".to_string()),
            ..Default::default()
        };

        let result = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();

        assert!(result.accepted);
        assert_eq!(result.channel_matches.len(), 1);
        assert_eq!(result.channel_matches[0].recipient_role, RecipientRole::Cc);
    }

    #[tokio::test]
    async fn test_multi_channel_to_and_cc_execution() {
        let company_id = Uuid::new_v4();
        let wf1_id = Uuid::new_v4();
        let wf2_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![
                Channel {
                    id: wf1_id,
                    company_id,
                    name: "Support".to_string(),
                    slug: "support".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: wf2_id,
                    company_id,
                    name: "Billing".to_string(),
                    slug: "billing".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
            ]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let raw_payload = RawInboundPayload {
            to: "support@acme.mailagents.com".to_string(),
            cc: Some("billing@acme.mailagents.com".to_string()),
            from: "customer@client.com".to_string(),
            subject: Some("Invoice and account question".to_string()),
            text: Some("Please help with my account and invoice.".to_string()),
            ..Default::default()
        };

        let ingest = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();

        assert!(ingest.accepted);
        assert_eq!(ingest.channel_matches.len(), 2);
        assert_eq!(ingest.channel_matches[0].channel.slug, "support");
        assert_eq!(ingest.channel_matches[0].recipient_role, RecipientRole::To);
        assert_eq!(ingest.channel_matches[1].channel.slug, "billing");
        assert_eq!(ingest.channel_matches[1].recipient_role, RecipientRole::Cc);

        // Verify thread creation for both channels
        let threads = thread_persistence.threads.lock().unwrap();
        assert_eq!(threads.len(), 2);
    }

    #[tokio::test]
    async fn test_pipeline_address_chaining_execution() {
        let company_id = Uuid::new_v4();
        let wf1_id = Uuid::new_v4();
        let wf2_id = Uuid::new_v4();
        let wf3_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![
                Channel {
                    id: wf1_id,
                    company_id,
                    name: "Support".to_string(),
                    slug: "support".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: wf2_id,
                    company_id,
                    name: "Billing".to_string(),
                    slug: "billing".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: wf3_id,
                    company_id,
                    name: "Legal".to_string(),
                    slug: "legal".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
            ]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let raw_payload = RawInboundPayload {
            to: "support+billing+legal@acme.mailagents.com".to_string(),
            from: "customer@client.com".to_string(),
            subject: Some("Pipeline Request".to_string()),
            text: Some("Please process via support, billing, and legal.".to_string()),
            ..Default::default()
        };

        let ingest = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();

        assert!(ingest.accepted);
        assert_eq!(ingest.channel_matches.len(), 3);
        assert_eq!(ingest.channel_matches[0].channel.slug, "support");
        assert_eq!(ingest.channel_matches[0].step_index, 0);
        assert_eq!(ingest.channel_matches[0].total_steps, 3);
        assert_eq!(ingest.channel_matches[1].channel.slug, "billing");
        assert_eq!(ingest.channel_matches[1].step_index, 1);
        assert_eq!(ingest.channel_matches[2].channel.slug, "legal");
        assert_eq!(ingest.channel_matches[2].step_index, 2);

        // Verify threads were created for all 3 step channels
        let threads = thread_persistence.threads.lock().unwrap();
        assert_eq!(threads.len(), 3);
    }

    #[tokio::test]
    async fn test_misspelled_channel_bounce_and_strict_pipeline_validation() {
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![
                Channel {
                    id: Uuid::new_v4(),
                    company_id,
                    name: "Support".to_string(),
                    slug: "support".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: Uuid::new_v4(),
                    company_id,
                    name: "Billing".to_string(),
                    slug: "billing".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
            ]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "".to_string(), // skip external SMTP in test
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        // 1. Single misspelled address
        let raw_payload_single = RawInboundPayload {
            to: "suppport@acme.mailagents.com".to_string(),
            from: "customer@client.com".to_string(),
            subject: Some("Help".to_string()),
            text: Some("Help please".to_string()),
            ..Default::default()
        };

        let ingest_single = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload_single)
            .await
            .unwrap();

        assert!(!ingest_single.accepted);
        assert_eq!(
            ingest_single.reason.as_deref(),
            Some("Channel address not found or misspelled")
        );
        let bounce_single = ingest_single.bounce_info.unwrap();
        assert_eq!(bounce_single.invalid_slugs, vec!["suppport"]);
        assert_eq!(bounce_single.suggestions[0].suggestions, vec!["support"]);

        // Verify bounce email body formatting
        let bounce_body = format_bounce_email_body(&bounce_single, "mailagents.com");
        assert!(bounce_body.contains("suppport@acme.mailagents.com"));
        assert!(bounce_body.contains("support@acme.mailagents.com"));

        // 2. Strict pipeline validation with misspelled step 'biling'
        let raw_payload_pipeline = RawInboundPayload {
            to: "support+biling@acme.mailagents.com".to_string(),
            from: "customer@client.com".to_string(),
            subject: Some("Pipeline Help".to_string()),
            text: Some("Help please".to_string()),
            ..Default::default()
        };

        let ingest_pipeline = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload_pipeline)
            .await
            .unwrap();

        assert!(!ingest_pipeline.accepted);
        let bounce_pipeline = ingest_pipeline.bounce_info.unwrap();
        assert_eq!(bounce_pipeline.invalid_slugs, vec!["biling"]);
        assert_eq!(bounce_pipeline.suggestions[0].suggestions, vec!["billing"]);
    }

    #[tokio::test]
    async fn test_quote_stripping_rules_for_first_in_thread_and_forwarded_emails() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Support".to_string(),
                slug: "support".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        // 1. Initial email in a NEW thread containing blockquotes/header markers.
        // Quotes MUST NOT be stripped because it is the first email in a new thread.
        let msg1_text = "Please check this user issue:\n\nOn Mon, Aug 10, 2026 at 10:00 AM User wrote:\n> Blockquoted text here";
        let raw_first_email = RawInboundPayload {
            to: "support@acme.mailagents.com".to_string(),
            from: "customer@client.com".to_string(),
            subject: Some("New Ticket with Quotes".to_string()),
            text: Some(msg1_text.to_string()),
            headers: Some("Message-ID: <msg1@client.com>\n".to_string()),
            ..Default::default()
        };

        let res1 = thread_use_cases
            .ingest_and_save_inbound_message(raw_first_email)
            .await
            .unwrap();

        assert!(res1.accepted);
        let msg1 = res1.inbound_message.unwrap();
        assert_eq!(msg1.clean_text_body, msg1_text); // Preserved full text!

        // 2. Reply in the EXISTING thread containing repeated quotes from msg1.
        // Quotes MUST be stripped because this is a subsequent message in an existing thread.
        let msg2_text = "Thanks for looking into this.\n\nPlease check this user issue:\n\nOn Mon, Aug 10, 2026 at 10:00 AM User wrote:\n> Blockquoted text here";
        let raw_reply_email = RawInboundPayload {
            to: "support@acme.mailagents.com".to_string(),
            from: "customer@client.com".to_string(),
            subject: Some("Re: New Ticket with Quotes".to_string()),
            text: Some(msg2_text.to_string()),
            headers: Some(
                "Message-ID: <msg2@client.com>\nIn-Reply-To: <msg1@client.com>\n".to_string(),
            ),
            ..Default::default()
        };

        let res2 = thread_use_cases
            .ingest_and_save_inbound_message(raw_reply_email)
            .await
            .unwrap();

        assert!(res2.accepted);
        let msg2 = res2.inbound_message.unwrap();
        assert_eq!(msg2.clean_text_body, "Thanks for looking into this."); // Stripped quotes!

        // 3. Forwarded email into existing or new thread.
        // Quotes MUST NOT be stripped because it is a forwarded email.
        let fwd_text = "FYI forwarding this:\n\n---------- Forwarded message ---------\nFrom: Alice <alice@other.com>\nDate: Mon, Aug 10, 2026\nSubject: Forwarded Issue\n\nOriginal forwarded body";
        let raw_fwd_email = RawInboundPayload {
            to: "support@acme.mailagents.com".to_string(),
            from: "manager@client.com".to_string(),
            subject: Some("Fwd: External Report".to_string()),
            text: Some(fwd_text.to_string()),
            headers: Some("Message-ID: <msg3@client.com>\n".to_string()),
            ..Default::default()
        };

        let res3 = thread_use_cases
            .ingest_and_save_inbound_message(raw_fwd_email)
            .await
            .unwrap();

        assert!(res3.accepted);
        let msg3 = res3.inbound_message.unwrap();
        assert_eq!(msg3.clean_text_body, fwd_text); // Preserved full forwarded text!
    }

    #[tokio::test]
    async fn test_participant_modes_company_team_public_and_explicit() {
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
            vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }],
            vec![(company_id, "team_member@acme.com".to_string())],
        ));

        let flow_team_only = Uuid::new_v4();
        let flow_public = Uuid::new_v4();
        let flow_explicit = Uuid::new_v4();

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![
                Channel {
                    id: flow_team_only,
                    company_id,
                    name: "Team Only".to_string(),
                    slug: "team-only".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None, // Default = Company Team
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: flow_public,
                    company_id,
                    name: "Public Flow".to_string(),
                    slug: "public-flow".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: Some(vec!["@public".into()]), // Public
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: flow_explicit,
                    company_id,
                    name: "Explicit Flow".to_string(),
                    slug: "explicit-flow".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: Some(vec!["allowed@external.com".into()]), // Explicit list
                    agent_ids: None,
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
            ]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        // 1. Team-only flow: Team member accepted
        let res1 = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                to: "team-only@acme.mailagents.com".to_string(),
                from: "team_member@acme.com".to_string(),
                subject: Some("Team msg".to_string()),
                text: Some("Hello team".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(res1.accepted);

        // 2. Team-only flow: External sender rejected
        let res2 = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                to: "team-only@acme.mailagents.com".to_string(),
                from: "external@other.com".to_string(),
                subject: Some("External msg".to_string()),
                text: Some("Hello team".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(!res2.accepted);
        assert_eq!(
            res2.reason.as_deref(),
            Some("Sender unauthorized for channel")
        );

        // 3. Public flow: External sender accepted
        let res3 = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                to: "public-flow@acme.mailagents.com".to_string(),
                from: "external@other.com".to_string(),
                subject: Some("Public msg".to_string()),
                text: Some("Hello public".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(res3.accepted);

        // 4. Explicit list flow: Allowed sender accepted, non-allowed rejected
        let res4 = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                to: "explicit-flow@acme.mailagents.com".to_string(),
                from: "allowed@external.com".to_string(),
                subject: Some("Explicit allowed".to_string()),
                text: Some("Hello allowed".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(res4.accepted);

        let res5 = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                to: "explicit-flow@acme.mailagents.com".to_string(),
                from: "notallowed@external.com".to_string(),
                subject: Some("Explicit blocked".to_string()),
                text: Some("Hello blocked".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(!res5.accepted);
        assert_eq!(
            res5.reason.as_deref(),
            Some("Sender unauthorized for channel")
        );
    }

    #[tokio::test]
    async fn test_sender_verification_and_delegation_target_check() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Support".to_string(),
                slug: "support".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: Some(vec!["@public".into()]),
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence.clone(),
            config,
        );

        // 1. Initial email from client creates thread
        let res1 = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some("Message-ID: <msg-client-1@external.com>\n".to_string()),
                to: "support@acme.mailagents.com".to_string(),
                from: "client@external.com".to_string(),
                subject: Some("Need quote".to_string()),
                text: Some("Can I get a quote?".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(res1.accepted);
        let thread_id = res1.thread.unwrap().id;

        // 2. Enqueue a waiting outreach with one target and a sent outbound Message-ID.
        let outreach_id = Uuid::new_v4();
        let delegation_payload = serde_json::json!({
            "test_outreach": {
                "outreach_id": outreach_id,
                "target_email": "vendor@supplier.com",
                "outbound_message_id": "<outreach-vendor@mailagents.com>"
            }
        });
        let task = task_persistence
            .enqueue_task(
                company_id,
                channel_id,
                Some(thread_id),
                "email_agent_dispatch",
                delegation_payload,
            )
            .await
            .unwrap();

        let mut list = task_persistence.tasks.lock().unwrap();
        if let Some(t) = list.iter_mut().find(|t| t.id == task.id) {
            t.status = crate::entities::task::TaskStatus::WaitingForThirdPartyReply;
        }
        drop(list);

        // 3. Unauthorized third-party attacker tries to inject message into thread using In-Reply-To
        let res_attacker = thread_use_cases.ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some("Message-ID: <msg-attacker-1@evil.com>\nIn-Reply-To: <msg-client-1@external.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            from: "attacker@evil.com".to_string(),
            subject: Some("Re: Need quote".to_string()),
            text: Some("Fake reply".to_string()),
            ..Default::default()
        }).await.unwrap();
        assert!(!res_attacker.accepted);
        assert!(res_attacker.bounce_info.is_some());
        assert_eq!(
            res_attacker.reason.as_deref(),
            Some("Sender is not an authorized participant or delegation target for this thread")
        );

        // 4. Authorized vendor replies to their exact outreach Message-ID.
        let res_vendor = thread_use_cases.ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some("Message-ID: <msg-vendor-1@supplier.com>\nIn-Reply-To: <outreach-vendor@mailagents.com>\nReferences: <msg-client-1@external.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            from: "vendor@supplier.com".to_string(),
            subject: Some("Re: Need quote".to_string()),
            text: Some("Quote is $500".to_string()),
            ..Default::default()
        }).await.unwrap();
        assert!(res_vendor.accepted);

        // Verify task was resumed to Pending
        let resumed_task = task_persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            resumed_task.status,
            crate::entities::task::TaskStatus::Pending
        );

        // Outreach authorization is scoped to the correlated outbound message and does not
        // permanently promote the target to a thread participant.
        let res_uncorrelated = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-vendor-2@supplier.com>\nIn-Reply-To: <msg-client-1@external.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                from: "vendor@supplier.com".to_string(),
                subject: Some("Re: Need quote".to_string()),
                text: Some("Uncorrelated follow-up".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(!res_uncorrelated.accepted);
    }

    #[tokio::test]
    async fn internal_channel_callback_resumes_original_task_without_new_task() {
        let company_id = Uuid::new_v4();
        let channel_a_id = Uuid::new_v4();
        let channel_b_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![
                Channel {
                    id: channel_a_id,
                    company_id,
                    name: "Agent A".to_string(),
                    slug: "agent-a".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: Some(vec!["@public".into()]),
                    agent_ids: Some(vec![Uuid::new_v4()]),
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: channel_b_id,
                    company_id,
                    name: "Agent B".to_string(),
                    slug: "agent-b".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: Some(vec![Uuid::new_v4()]),
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
            ]),
        });
        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });
        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });
        let use_cases = ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence.clone(),
            internal_test_config(),
        );

        let initial = use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some("Message-ID: <human-request@example.com>\n".to_string()),
                to: "agent-a@acme.mailagents.com".to_string(),
                from: "human@example.com".to_string(),
                subject: Some("Acquire data".to_string()),
                text: Some("Please obtain supplier data".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        let parent_task_id = initial.task_id.unwrap();
        let thread_a = initial.thread.unwrap();
        let call = use_cases
            .prepare_internal_channel_delivery(
                OutboundEmail {
                    channel_id: channel_a_id,
                    channel_name: "Agent A".to_string(),
                    channel_slug: "agent-a".into(),
                    company_slug: "acme".into(),
                    trigger_message_id: "<human-request@example.com>".into(),
                    thread_references: Vec::new(),
                    recipient_to: "agent-b@acme.mailagents.com".into(),
                    recipients_cc: Vec::new(),
                    subject: "Acquire data".to_string(),
                    body_text: "Obtain supplier data".to_string(),
                    hop_count: 0,
                    trace_channels: Vec::new(),
                },
                Some("agent-a-call"),
            )
            .await
            .unwrap()
            .unwrap();
        let call_message_id = call.outbound_message_id.clone();
        thread_persistence
            .create_message(&Message {
                id: Uuid::new_v4(),
                thread_id: thread_a.id,
                message_id: call_message_id.clone(),
                in_reply_to: Some(call.in_reply_to.clone()),
                references_list: call.references.clone(),
                sender: call.from_address.clone(),
                recipients_to: call.recipients_to.clone(),
                recipients_cc: call.recipients_cc.clone(),
                subject: call.subject.clone(),
                clean_text_body: call.body_text.clone(),
                raw_text_body: None,
                raw_html_body: None,
                attachments: None,
                direction: MessageDirection::Outbound,
                role: MessageRole::Agent,
                thread_index: None,
                created_at: Utc::now().naive_utc(),
            })
            .await
            .unwrap();
        let outreach_id = Uuid::new_v4();
        {
            let mut tasks = task_persistence.tasks.lock().unwrap();
            let parent_task = tasks
                .iter_mut()
                .find(|task| task.id == parent_task_id)
                .unwrap();
            parent_task.payload = serde_json::json!({
                "test_outreach": {
                    "outreach_id": outreach_id,
                    "target_email": "agent-b@acme.mailagents.com",
                    "outbound_message_id": call_message_id.clone()
                }
            });
            parent_task.status = crate::entities::task::TaskStatus::WaitingForThirdPartyReply;
        }
        let delegated = use_cases
            .ingest_prepared_internal_message(&call)
            .await
            .unwrap();
        assert!(delegated.accepted);
        assert!(delegated.task_id.is_some());
        assert_ne!(delegated.thread.as_ref().unwrap().id, thread_a.id);
        assert_eq!(delegated.thread.as_ref().unwrap().channel_id, channel_b_id);

        let prepared = use_cases
            .prepare_internal_channel_delivery(
                OutboundEmail {
                    channel_id: channel_b_id,
                    channel_name: "Agent B".to_string(),
                    channel_slug: "agent-b".into(),
                    company_slug: "acme".into(),
                    trigger_message_id: call_message_id.clone(),
                    thread_references: vec![
                        "<human-request@example.com>".into(),
                        call_message_id.clone(),
                    ],
                    recipient_to: "agent-a@acme.mailagents.com".into(),
                    recipients_cc: Vec::new(),
                    subject: "Acquire data".to_string(),
                    body_text: "Supplier confirmed 2,000 units".to_string(),
                    hop_count: 1,
                    trace_channels: vec![channel_a_id],
                },
                Some("agent-b-result"),
            )
            .await
            .unwrap()
            .unwrap();
        let callback = use_cases
            .ingest_prepared_internal_message(&prepared)
            .await
            .unwrap();

        assert!(callback.accepted);
        assert!(callback.task_id.is_none());
        assert_eq!(callback.thread.unwrap().id, thread_a.id);
        assert_eq!(callback.inbound_message.unwrap().role, MessageRole::Agent);
        assert_eq!(
            task_persistence
                .get_task_by_id(parent_task_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            crate::entities::task::TaskStatus::Pending
        );
        assert_eq!(task_persistence.tasks.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn uncorrelated_inter_channel_cycle_is_rejected() {
        let company_id = Uuid::new_v4();
        let channel_a_id = Uuid::new_v4();
        let channel_b_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![
                Channel {
                    id: channel_a_id,
                    company_id,
                    name: "Agent A".to_string(),
                    slug: "agent-a".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: Some(vec![Uuid::new_v4()]),
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: channel_b_id,
                    company_id,
                    name: "Agent B".to_string(),
                    slug: "agent-b".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: Some(vec![Uuid::new_v4()]),
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
            ]),
        });
        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });
        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });
        let use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            internal_test_config(),
        );

        let unsolicited_prepared = use_cases
            .prepare_internal_channel_delivery(
                OutboundEmail {
                    channel_id: channel_b_id,
                    channel_name: "Agent B".to_string(),
                    channel_slug: "agent-b".into(),
                    company_slug: "acme".into(),
                    trigger_message_id: "<unsolicited@example.com>".into(),
                    thread_references: vec!["<unsolicited@example.com>".into()],
                    recipient_to: "agent-a@acme.mailagents.com".into(),
                    recipients_cc: Vec::new(),
                    subject: "Unsolicited message".to_string(),
                    body_text: "Spontaneous message".to_string(),
                    hop_count: 1,
                    trace_channels: vec![channel_a_id],
                },
                None,
            )
            .await
            .unwrap()
            .unwrap();

        let result = use_cases
            .ingest_prepared_internal_message(&unsolicited_prepared)
            .await
            .unwrap();

        assert!(!result.accepted);
        assert_eq!(
            result.reason.as_deref(),
            Some("Inter-channel loop cycle detected")
        );
    }

    #[tokio::test]
    async fn inter_channel_max_hops_exceeded_is_rejected() {
        let company_id = Uuid::new_v4();
        let channel_a_id = Uuid::new_v4();
        let channel_b_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        }]));
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![
                Channel {
                    id: channel_a_id,
                    company_id,
                    name: "Agent A".to_string(),
                    slug: "agent-a".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: Some(vec![Uuid::new_v4()]),
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
                Channel {
                    id: channel_b_id,
                    company_id,
                    name: "Agent B".to_string(),
                    slug: "agent-b".into(),
                    api_key: None,
                    provider: None,
                    model: None,
                    participant_emails: None,
                    agent_ids: Some(vec![Uuid::new_v4()]),
                    channel_config: None,
                    created_at: Utc::now().naive_utc(),
                },
            ]),
        });
        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });
        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });
        let use_cases = ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence,
            internal_test_config(),
        );

        let max_hop_prepared = use_cases
            .prepare_internal_channel_delivery(
                OutboundEmail {
                    channel_id: channel_a_id,
                    channel_name: "Agent A".to_string(),
                    channel_slug: "agent-a".into(),
                    company_slug: "acme".into(),
                    trigger_message_id: "<msg@example.com>".into(),
                    thread_references: Vec::new(),
                    recipient_to: "agent-b@acme.mailagents.com".into(),
                    recipients_cc: Vec::new(),
                    subject: "Deep loop".to_string(),
                    body_text: "Exceeding max hops".to_string(),
                    hop_count: crate::use_cases::thread::MAX_CHANNEL_HOPS,
                    trace_channels: Vec::new(),
                },
                None,
            )
            .await
            .unwrap()
            .unwrap();

        let result = use_cases
            .ingest_prepared_internal_message(&max_hop_prepared)
            .await
            .unwrap();

        assert!(!result.accepted);
        assert_eq!(
            result.reason.as_deref(),
            Some("Max inter-channel hop count reached")
        );
    }

    #[tokio::test]
    async fn test_third_party_thread_participants_addition_and_authorization() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
            vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }],
            vec![(company_id, "team@acme.com".to_string())],
        ));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Support Channel".to_string(),
                slug: "support".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None, // Default = company team members
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        // 1. Team member (workflow participant) sends email to channel with third-party in CC
        let res1 = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some("Message-ID: <msg-team-1@acme.com>\n".to_string()),
                to: "support@acme.mailagents.com".to_string(),
                cc: Some("client@external.com".to_string()),
                from: "team@acme.com".to_string(),
                subject: Some("Client Inquiry".to_string()),
                text: Some("Adding third-party client to thread".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(res1.accepted);
        let thread = res1.thread.unwrap();
        assert!(
            thread
                .participant_emails
                .iter()
                .any(|p| p.eq_ignore_ascii_case("client@external.com"))
        );

        // 2. Third-party client replies to thread -> ACCEPTED because they were added to thread participants
        let res_reply = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-client-reply-1@external.com>\nIn-Reply-To: <msg-team-1@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                from: "client@external.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Thanks for adding me".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(res_reply.accepted);

        // 3. Third-party client tries to send a NEW email to channel without thread reference -> REJECTED
        let res_unauth = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some("Message-ID: <msg-client-new-1@external.com>\n".to_string()),
                to: "support@acme.mailagents.com".to_string(),
                from: "client@external.com".to_string(),
                subject: Some("Unrelated Subject".to_string()),
                text: Some("Starting a new conversation".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(!res_unauth.accepted);
        assert_eq!(
            res_unauth.reason.as_deref(),
            Some("Sender unauthorized for channel")
        );

        // 4. Team member replies to existing thread, adding another third party (vendor@supplier.com) in To
        let res_expand = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-team-2@acme.com>\nIn-Reply-To: <msg-team-1@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com, vendor@supplier.com".to_string(),
                from: "team@acme.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Looping in supplier".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(res_expand.accepted);
        let updated_thread = res_expand.thread.unwrap();
        assert!(
            updated_thread
                .participant_emails
                .iter()
                .any(|p| p.eq_ignore_ascii_case("vendor@supplier.com"))
        );

        // 5. vendor@supplier.com replies to thread -> ACCEPTED
        let res_vendor_reply = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-vendor-reply-1@supplier.com>\nIn-Reply-To: <msg-team-2@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                from: "vendor@supplier.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Vendor here".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(res_vendor_reply.accepted);

        // 6. External non-team-member (client@external.com) replies and tries to CC unauthorized@other.com -> unauthorized@other.com is NOT added
        let res_external_cc = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-client-reply-2@external.com>\nIn-Reply-To: <msg-team-1@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                cc: Some("unauthorized@other.com".to_string()),
                from: "client@external.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Adding random CC".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(res_external_cc.accepted);
        let current_thread = res_external_cc.thread.unwrap();
        assert!(
            !current_thread
                .participant_emails
                .iter()
                .any(|p| p.eq_ignore_ascii_case("unauthorized@other.com"))
        );
    }

    #[tokio::test]
    async fn test_context_only_quiet_mode_ingestion() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
            vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }],
            vec![(company_id, "team@acme.com".to_string())],
        ));

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: channel_id,
                company_id,
                name: "Support Channel".to_string(),
                slug: "support".into(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        // 1. Ingest email with .quiet suffix in address -> accepted, task_id is None (agent execution skipped)
        let res_quiet_addr = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some("Message-ID: <msg-quiet-1@acme.com>\n".to_string()),
                to: "support.quiet@acme.mailagents.com".to_string(),
                from: "team@acme.com".to_string(),
                subject: Some("Quiet Note".to_string()),
                text: Some("Background context note".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(res_quiet_addr.accepted);
        assert!(res_quiet_addr.task_id.is_none());
        assert!(res_quiet_addr.parsed_email.unwrap().is_context_only);

        // 2. Ingest email with [[quiet]] body tag -> accepted, task_id is None, tag stripped from text
        let res_quiet_body = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-quiet-2@acme.com>\nIn-Reply-To: <msg-quiet-1@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                from: "team@acme.com".to_string(),
                subject: Some("Re: Quiet Note".to_string()),
                text: Some("[[quiet]] Additional background note".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(res_quiet_body.accepted);
        assert!(res_quiet_body.task_id.is_none());
        let parsed_body = res_quiet_body.parsed_email.unwrap();
        assert!(parsed_body.is_context_only);
        assert_eq!(parsed_body.clean_text_body, "Additional background note");
    }
}
