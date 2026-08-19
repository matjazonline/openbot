//! Agent execution and outbound dispatch.
//!
//! A durable task claim guards the whole flow, then:
//!
//! 1. [`ThreadUseCases::run_agents`] — run each matched channel's agent, threading each step's
//!    output into the next as upstream pipeline context.
//! 2. [`ThreadUseCases::deliver_agent_response`] — send the combined reply (or fabricate a
//!    simulated delivery when the caller is only testing).
//! 3. [`ThreadUseCases::record_dispatch_outcome`] — persist outbound messages and close out the
//!    task, failing it loudly if the primary agent errored.

use std::collections::HashMap;

use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    adapters::persistence::task::{OutboundSendReservation, TASK_LEASE_SECONDS},
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        channel::{ChannelType, PUBLIC_PARTICIPANT, ParticipantIdentity},
        message::{Message, MessageDirection, MessageRole},
        message_contract::NormalizedOutboundMessage,
        task::TokenUsage,
        value_objects::{EmailAddress, MessageId},
    },
    services::{
        agent_runner::{
            AgentExecutionDisposition, AgentRunner, ApprovalContext as AgentRunnerApprovalContext,
            ResolvedAgentParams,
        },
        email_parser::ParsedEmail,
        outbound_dispatcher::{OutboundDispatcher, OutboundEmail, SentEmailResult},
        outreach_tool::OutreachToolContext,
    },
};

use super::{
    AgentExecutionResult, ChannelMatch, InboundIngestResult, RecipientRole, ThreadUseCases,
    durable_ingest_payload, scrub_json_secrets, support::outbound_reference_ids,
};

/// Who owns the durable task while the agent runs.
#[derive(Debug, Clone, Copy)]
enum TaskClaim {
    /// The caller already holds the lease (the background worker path).
    Held(Uuid),
    /// This call must take the lease itself (the inline/simulation path).
    TakeHere,
}

/// The lease actually in force for this dispatch.
struct ActiveClaim {
    /// Whoever holds the lease, whether inherited from the caller or taken here.
    worker_id: Option<Uuid>,
    /// Set only when *this* call took the lease, and so is responsible for closing the task out.
    owned_worker_id: Option<Uuid>,
}

/// One agent's contribution to the reply.
struct AgentOutput<'a> {
    channel_match: &'a ChannelMatch,
    content: String,
    metadata: Option<serde_json::Value>,
}

/// Everything the agent phase produced, before anything is sent.
struct AgentRun<'a> {
    outputs: Vec<AgentOutput<'a>>,
    prompt_tokens: usize,
    completion_tokens: usize,
    /// Error from the *first* channel, which decides whether the task is failed and retried.
    primary_error: Option<String>,
    primary_params: Option<ResolvedAgentParams>,
    primary_agent: Option<Agent>,
}

/// The outbound side of the reply, real or simulated.
struct OutboundDelivery {
    message_id: String,
    in_reply_to: String,
    references: Vec<String>,
    from_address: String,
    recipients_to: Vec<String>,
    recipients_cc: Vec<String>,
    subject: String,
    email_sent: bool,
}

impl ThreadUseCases {
    pub async fn execute_agent_and_dispatch(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
    ) -> AppResult<Option<AgentExecutionResult>> {
        self.dispatch(ingest, send_email, TaskClaim::TakeHere).await
    }

    pub async fn execute_claimed_agent_task_and_dispatch(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
        worker_id: Uuid,
    ) -> AppResult<Option<AgentExecutionResult>> {
        self.dispatch(ingest, send_email, TaskClaim::Held(worker_id))
            .await
    }

    async fn dispatch(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
        claim: TaskClaim,
    ) -> AppResult<Option<AgentExecutionResult>> {
        if let Some(message_id) = context_only_message_id(ingest) {
            info!("Skipping agent execution for context-only message ID {message_id}");
            return Ok(None);
        }

        let Some(claim) = self.acquire_claim(ingest, claim).await? else {
            return Ok(None);
        };
        let Some(parsed) = ingest.parsed_email.as_ref() else {
            return Ok(None);
        };
        let Some(matches) = channel_matches_of(ingest) else {
            return Ok(None);
        };

        let Some(run) = self.run_agents(&matches, parsed, ingest, &claim).await? else {
            info!("Agent execution suspended for task approval or outreach");
            return Ok(None);
        };

        let response = self.combine_responses(&run.outputs);
        let metadata = combine_metadata(&run.outputs);
        let delivery = self
            .deliver_agent_response(&matches, parsed, ingest, &claim, &response, send_email)
            .await?;

        self.save_outbound_messages(&matches, &delivery, &response)
            .await?;
        self.record_dispatch_outcome(
            ingest, parsed, &claim, &run, &delivery, &response, &metadata,
        )
        .await?;

        Ok(Some(AgentExecutionResult {
            outbound_message_id: Some(delivery.message_id),
            agent_response: response,
            email_sent: delivery.email_sent,
            token_usage: Some(TokenUsage::new(run.prompt_tokens, run.completion_tokens)),
            metadata,
        }))
    }

    /// Take the task lease unless the caller already holds it. `Ok(None)` means another worker got
    /// there first and this dispatch must not duplicate the send.
    async fn acquire_claim(
        &self,
        ingest: &InboundIngestResult,
        claim: TaskClaim,
    ) -> AppResult<Option<ActiveClaim>> {
        let worker_id = match claim {
            TaskClaim::Held(worker_id) => {
                return Ok(Some(ActiveClaim {
                    worker_id: Some(worker_id),
                    owned_worker_id: None,
                }));
            }
            TaskClaim::TakeHere => Uuid::new_v4(),
        };
        let Some(task_id) = ingest.task_id else {
            return Ok(Some(ActiveClaim {
                worker_id: None,
                owned_worker_id: None,
            }));
        };

        let lease_expires_at =
            chrono::Utc::now().naive_utc() + chrono::Duration::seconds(TASK_LEASE_SECONDS);
        if !self
            .task_persistence
            .claim_task(task_id, worker_id, lease_expires_at)
            .await?
        {
            info!(
                "Task {task_id} already claimed or completed by another worker, skipping duplicate dispatch"
            );
            return Ok(None);
        }
        info!("Successfully claimed task {task_id} for execution");
        Ok(Some(ActiveClaim {
            worker_id: Some(worker_id),
            owned_worker_id: Some(worker_id),
        }))
    }

    /// Run every matched channel's agent in pipeline order. `Ok(None)` means an agent suspended
    /// itself awaiting approval or an outreach reply, and the task stays open.
    async fn run_agents<'a>(
        &self,
        matches: &'a [ChannelMatch],
        parsed: &ParsedEmail,
        ingest: &InboundIngestResult,
        claim: &ActiveClaim,
    ) -> AppResult<Option<AgentRun<'a>>> {
        let mut run = AgentRun {
            outputs: Vec::with_capacity(matches.len()),
            prompt_tokens: 0,
            completion_tokens: 0,
            primary_error: None,
            primary_params: None,
            primary_agent: None,
        };
        let mut agent_cache: HashMap<Uuid, Option<Agent>> = HashMap::new();
        let mut membership_cache: HashMap<(Uuid, String), bool> = HashMap::new();

        for (index, channel_match) in matches.iter().enumerate() {
            let history = self
                .thread_persistence
                .list_messages_by_thread_id(channel_match.thread.id)
                .await?;
            let agent = self
                .first_agent_for(channel_match, &mut agent_cache)
                .await?;
            let params = ResolvedAgentParams::new(
                Some(&channel_match.company),
                Some(&channel_match.channel),
                agent.as_ref(),
            );
            if index == 0 {
                run.primary_params = params.as_ref().ok().cloned();
                run.primary_agent = agent.clone();
            }

            let sender = parsed.sender.trim();
            let member_key = (channel_match.company.id, sender.to_lowercase());
            let is_team_member = match membership_cache.get(&member_key) {
                Some(cached) => *cached,
                None => {
                    let loaded = self
                        .company_persistence
                        .is_company_team_member(channel_match.company.id, sender)
                        .await?;
                    membership_cache.insert(member_key, loaded);
                    loaded
                }
            };
            let access = channel_match
                .channel
                .participant_access(sender, is_team_member);

            let upstream_context = self
                .upstream_context_for(&run.outputs, ingest.task_id)
                .await?;

            let result = match &params {
                Ok(params) => {
                    let mut runner = AgentRunner::new(&parsed.prompt_text, params)
                        .history(&history)
                        .approval_use_cases(self.approval_use_cases.clone())
                        .approval_context(Some(
                            self.approval_context_for(channel_match, ingest).await,
                        ))
                        .monitoring(self.monitoring.clone())
                        .config(Some(self.config.clone()))
                        .company(Some(channel_match.company.clone()))
                        .skip_spam_guardrail(access.trusted)
                        .recipient_role(Some(channel_match.recipient_role))
                        .upstream_pipeline_context(upstream_context)
                        .ids(
                            Some(channel_match.company.id),
                            Some(channel_match.channel.id),
                            agent.as_ref().map(|a| a.id),
                        );
                    if let (Some(task_id), Some(worker_id)) = (ingest.task_id, claim.worker_id) {
                        runner = runner.outreach_tool(
                            self.task_persistence.clone(),
                            self.channel_persistence.clone(),
                            self.outreach_context_for(channel_match, parsed, task_id, worker_id),
                        );
                    }
                    runner.execute().await
                }
                Err(err) => Err(anyhow::anyhow!("{err}")),
            };

            match result {
                Ok(output) => {
                    if output.disposition == AgentExecutionDisposition::Suspended {
                        return Ok(None);
                    }
                    run.prompt_tokens += output.token_usage.prompt_tokens;
                    run.completion_tokens += output.token_usage.completion_tokens;
                    run.outputs.push(AgentOutput {
                        channel_match,
                        content: output.content,
                        metadata: output.metadata,
                    });
                }
                Err(err) => {
                    let message = format!("Agent execution failed: {err}");
                    if index == 0 {
                        run.primary_error = Some(message.clone());
                    }
                    run.outputs.push(AgentOutput {
                        channel_match,
                        content: message,
                        metadata: None,
                    });
                }
            }
        }

        Ok(Some(run))
    }

    async fn first_agent_for(
        &self,
        channel_match: &ChannelMatch,
        cache: &mut HashMap<Uuid, Option<Agent>>,
    ) -> AppResult<Option<Agent>> {
        let Some(persistence) = self.agent_persistence.as_ref() else {
            return Ok(None);
        };
        let Some(&agent_id) = channel_match
            .channel
            .agent_ids
            .as_ref()
            .and_then(|ids| ids.first())
        else {
            return Ok(None);
        };
        if let Some(cached) = cache.get(&agent_id) {
            return Ok(cached.clone());
        }
        let loaded = persistence.get_by_id(agent_id).await?;
        cache.insert(agent_id, loaded.clone());
        Ok(loaded)
    }

    /// Approvals go to the first non-public channel participant, falling back to any company team
    /// member.
    async fn approval_context_for(
        &self,
        channel_match: &ChannelMatch,
        ingest: &InboundIngestResult,
    ) -> AgentRunnerApprovalContext {
        let team_approver = self
            .company_persistence
            .list_company_team_emails(channel_match.company.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .next();
        let approver_email = channel_match
            .channel
            .participant_emails
            .as_ref()
            .and_then(|participants| {
                participants
                    .iter()
                    .find(|email| !email.eq_ignore_ascii_case(PUBLIC_PARTICIPANT))
                    .cloned()
            })
            .or(team_approver.map(EmailAddress::from))
            .unwrap_or_default();

        AgentRunnerApprovalContext {
            company_id: channel_match.company.id,
            channel_id: channel_match.channel.id,
            channel_name: channel_match.channel.name.clone(),
            channel_slug: channel_match.reply_slug(),
            company_slug: channel_match.company.slug.clone(),
            thread_id: Some(channel_match.thread.id),
            task_id: ingest.task_id,
            approver_email,
        }
    }

    fn outreach_context_for(
        &self,
        channel_match: &ChannelMatch,
        parsed: &ParsedEmail,
        task_id: Uuid,
        worker_id: Uuid,
    ) -> OutreachToolContext {
        OutreachToolContext {
            task_id,
            worker_id,
            company_id: channel_match.company.id,
            channel_id: channel_match.channel.id,
            channel_name: channel_match.channel.name.clone(),
            channel_slug: channel_match.reply_slug(),
            company_slug: channel_match.company.slug.clone(),
            trigger_message_id: parsed.message_id.clone().into(),
            thread_references: outbound_reference_ids(parsed)
                .into_iter()
                .map(MessageId::from)
                .collect(),
            hop_count: parsed.hop_count,
            trace_channels: parsed.trace_channels.clone(),
            app_domain_name: self.config.app_domain_name.clone(),
        }
    }

    /// Earlier pipeline steps' answers, plus any outreach replies gathered so far, prepended to the
    /// next agent's prompt.
    async fn upstream_context_for(
        &self,
        previous: &[AgentOutput<'_>],
        task_id: Option<Uuid>,
    ) -> AppResult<Option<String>> {
        let mut context = String::new();
        for output in previous {
            context.push_str(&format!(
                "--- Step {step}: {name} ({slug}) ---\n{content}\n\n",
                step = output.channel_match.step_index + 1,
                name = output.channel_match.channel.name,
                slug = output.channel_match.channel.slug,
                content = output.content,
            ));
        }
        if let Some(task_id) = task_id
            && let Some(outreach_context) =
                self.task_persistence.get_outreach_context(task_id).await?
        {
            context.push_str("--- Outreach Progress ---\n");
            context.push_str(&outreach_context);
            context.push_str("\n\n");
        }
        Ok((!context.is_empty()).then_some(context))
    }

    /// A single-channel reply is the agent's text verbatim; a pipeline reply is labelled per step
    /// so the recipient can tell the channels apart.
    fn combine_responses(&self, outputs: &[AgentOutput<'_>]) -> String {
        if let [only] = outputs {
            return only.content.clone();
        }
        outputs
            .iter()
            .map(|output| {
                let channel_match = output.channel_match;
                format!(
                    "[{name} ({slug}@{company}.{domain}) - {role}]\n{content}",
                    name = channel_match.channel.name,
                    slug = channel_match.channel.slug,
                    company = channel_match.company.slug,
                    domain = self.config.app_domain_name,
                    role = match channel_match.recipient_role {
                        RecipientRole::To => "TO",
                        RecipientRole::Cc => "CC",
                    },
                    content = output.content,
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n--------------------------------------------------\n\n")
    }

    async fn deliver_agent_response(
        &self,
        matches: &[ChannelMatch],
        parsed: &ParsedEmail,
        ingest: &InboundIngestResult,
        claim: &ActiveClaim,
        response: &str,
        send_email: bool,
    ) -> AppResult<OutboundDelivery> {
        let primary = &matches[0];
        if !send_email {
            return Ok(self.simulated_delivery(primary, parsed));
        }

        let references = outbound_reference_ids(parsed);
        let recipients_cc = self.outbound_cc_for(primary, parsed);

        let outbound_email = OutboundEmail {
            channel_id: primary.channel.id,
            channel_name: primary.channel.name.clone(),
            channel_slug: primary.reply_slug(),
            company_slug: primary.company.slug.clone(),
            trigger_message_id: parsed.message_id.clone().into(),
            thread_references: references.iter().cloned().map(MessageId::from).collect(),
            recipient_to: parsed.sender.clone().into(),
            recipients_cc: recipients_cc
                .iter()
                .cloned()
                .map(EmailAddress::from)
                .collect(),
            subject: parsed.subject.clone(),
            body_text: response.to_string(),
            hop_count: parsed.hop_count,
            trace_channels: parsed.trace_channels.clone(),
        };

        // The lease must still be ours at the moment of sending, or two workers could both reply.
        if let (Some(task_id), Some(worker_id)) = (ingest.task_id, claim.worker_id) {
            let renewed = self
                .task_persistence
                .renew_task_lease(
                    task_id,
                    worker_id,
                    chrono::Utc::now().naive_utc() + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                )
                .await?;
            if !renewed {
                return Err(AppError::Internal(
                    "Task lease was lost before outbound dispatch".into(),
                ));
            }
        }

        let sent = self
            .deliver_reply_at_most_once(outbound_email, ingest, claim, primary.company.id)
            .await?;

        // Registered egress adapters observe the normalized form of what we just sent.
        let norm_outbound = NormalizedOutboundMessage {
            thread_id: primary.thread.id,
            in_reply_to_ref: Some(MessageId::from(parsed.message_id.clone())),
            references: references.into_iter().map(MessageId::from).collect(),
            recipients_to: vec![ParticipantIdentity::email(&parsed.sender)],
            recipients_cc: recipients_cc
                .iter()
                .map(ParticipantIdentity::email)
                .collect(),
            subject: parsed.subject.clone(),
            content: response.to_string(),
            attachments: vec![],
            protocol: ChannelType::Email,
            channel_id: primary.channel.id,
            hop_count: parsed.hop_count,
            trace_channels: parsed.trace_channels.clone(),
        };
        let _ = self.egress_registry.get(&norm_outbound.protocol);

        Ok(OutboundDelivery {
            message_id: sent.outbound_message_id.into_string(),
            in_reply_to: sent.in_reply_to.into_string(),
            references: sent
                .references
                .into_iter()
                .map(MessageId::into_string)
                .collect(),
            from_address: sent.from_address.into_string(),
            recipients_to: sent
                .recipients_to
                .into_iter()
                .map(EmailAddress::into_string)
                .collect(),
            recipients_cc: sent
                .recipients_cc
                .into_iter()
                .map(EmailAddress::into_string)
                .collect(),
            subject: sent.subject,
            email_sent: true,
        })
    }

    /// Keep channel participants (other than `@public` and the sender) on the reply's CC list.
    fn outbound_cc_for(&self, primary: &ChannelMatch, parsed: &ParsedEmail) -> Vec<String> {
        let mut recipients_cc = parsed.recipients_cc.clone();
        let Some(participants) = primary.channel.participant_emails.as_ref() else {
            return recipients_cc;
        };
        for participant in participants {
            if participant.eq_ignore_ascii_case(PUBLIC_PARTICIPANT)
                || participant.eq_ignore_ascii_case(&parsed.sender)
                || recipients_cc
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(participant))
            {
                continue;
            }
            recipients_cc.push(participant.to_string());
        }
        recipients_cc
    }

    /// Deliver over the trusted in-process transport when the recipient is another platform
    /// channel, otherwise hand the mail to SMTP.
    async fn send_or_route_internally(
        &self,
        outbound_email: OutboundEmail,
        idempotency_key: Option<&str>,
    ) -> AppResult<crate::services::outbound_dispatcher::SentEmailResult> {
        if let Some(prepared) = self
            .prepare_internal_channel_delivery(outbound_email.clone(), idempotency_key)
            .await?
        {
            let ingest = self.ingest_prepared_internal_message(&prepared).await?;
            if !ingest.accepted
                && ingest.reason.as_deref() != Some("Duplicate Message-ID already processed")
            {
                return Err(AppError::Internal(ingest.reason.unwrap_or_else(|| {
                    "Internal channel delivery was rejected".into()
                })));
            }
            info!(
                "Delivered agent response {} through trusted internal channel transport",
                prepared.outbound_message_id
            );
            return Ok(prepared);
        }
        match idempotency_key {
            Some(key) => {
                OutboundDispatcher::send_idempotent(&self.config, outbound_email, key).await
            }
            None => OutboundDispatcher::send(&self.config, outbound_email).await,
        }
    }

    /// Send the agent's reply, guaranteeing at most one delivery per task.
    ///
    /// Renewing the lease just before dispatch narrows the race but cannot close it: the renewal
    /// and the SMTP handshake are not one atomic step, so a worker that stalls past its lease in
    /// between can still be racing a worker that reclaimed the task. The decisive guard is a row
    /// keyed on a deterministic idempotency key — `task:{id}:agent-reply` is identical across every
    /// re-run of the same task — so the unique index, not the timing, decides who delivers.
    ///
    /// A task-less caller (simulation, direct ingest) has nothing to key on and simply sends.
    async fn deliver_reply_at_most_once(
        &self,
        outbound_email: OutboundEmail,
        ingest: &InboundIngestResult,
        claim: &ActiveClaim,
        company_id: Uuid,
    ) -> AppResult<SentEmailResult> {
        let Some((task_id, worker_id)) = ingest.task_id.zip(claim.worker_id) else {
            return self.send_or_route_internally(outbound_email, None).await;
        };
        let idempotency_key = format!("task:{task_id}:agent-reply");

        let reserved = self
            .task_persistence
            .reserve_outbound_send(OutboundSendReservation {
                company_id,
                task_id: Some(task_id),
                idempotency_key: idempotency_key.clone(),
                payload: serde_json::to_value(&outbound_email).unwrap_or_default(),
                worker_id,
                lock_expires_at: chrono::Utc::now().naive_utc()
                    + chrono::Duration::seconds(TASK_LEASE_SECONDS),
            })
            .await?;

        let Some(outbox_id) = reserved else {
            // Another worker owns this reply. Rebuild what it sent rather than sending again: the
            // Message-ID is a digest of the same key, so the caller's bookkeeping still lines up
            // and `ON CONFLICT (company_id, message_id)` collapses the duplicate row.
            warn!(
                "Agent reply for task {task_id} is already being delivered by another worker; \
                 skipping duplicate send"
            );
            return OutboundDispatcher::prepare_idempotent(
                &self.config,
                outbound_email,
                &idempotency_key,
            );
        };

        match self
            .send_or_route_internally(outbound_email, Some(&idempotency_key))
            .await
        {
            Ok(sent) => {
                let _ = self
                    .task_persistence
                    .mark_outbox_email_sent(outbox_id, worker_id, sent.outbound_message_id.as_str())
                    .await;
                Ok(sent)
            }
            Err(error) => {
                // Give the key back, or the task's own retry would be locked out of its own send.
                let _ = self
                    .task_persistence
                    .release_outbound_send(outbox_id, worker_id)
                    .await;
                Err(error)
            }
        }
    }

    fn simulated_delivery(&self, primary: &ChannelMatch, parsed: &ParsedEmail) -> OutboundDelivery {
        let message_id = format!(
            "<simulated-test-{}@{}>",
            Uuid::new_v4(),
            self.config.app_domain_name
        );
        info!(
            "Simulation test mode (Run_Test): Skipped SMTP email dispatch for Message-ID {message_id}"
        );
        OutboundDelivery {
            message_id,
            in_reply_to: parsed.message_id.clone(),
            references: parsed.references.clone(),
            from_address: format!(
                "{}@{}.{}",
                primary.reply_slug(),
                primary.company.slug,
                self.config.app_domain_name
            ),
            recipients_to: vec![parsed.sender.clone()],
            recipients_cc: parsed.recipients_cc.clone(),
            subject: if parsed.subject.to_lowercase().starts_with("re:") {
                parsed.subject.clone()
            } else {
                format!("Re: {}", parsed.subject)
            },
            email_sent: false,
        }
    }

    /// The reply is stored in every thread it answered, so each channel's history stays complete.
    async fn save_outbound_messages(
        &self,
        matches: &[ChannelMatch],
        delivery: &OutboundDelivery,
        response: &str,
    ) -> AppResult<()> {
        for channel_match in matches {
            self.thread_persistence
                .create_message(&Message {
                    id: Uuid::new_v4(),
                    thread_id: channel_match.thread.id,
                    message_id: MessageId::from(delivery.message_id.clone()),
                    in_reply_to: Some(MessageId::from(delivery.in_reply_to.clone())),
                    references_list: delivery
                        .references
                        .iter()
                        .cloned()
                        .map(MessageId::from)
                        .collect(),
                    sender: EmailAddress::from(delivery.from_address.clone()),
                    recipients_to: delivery
                        .recipients_to
                        .iter()
                        .cloned()
                        .map(EmailAddress::from)
                        .collect(),
                    recipients_cc: delivery
                        .recipients_cc
                        .iter()
                        .cloned()
                        .map(EmailAddress::from)
                        .collect(),
                    subject: delivery.subject.clone(),
                    clean_text_body: response.to_string(),
                    raw_text_body: None,
                    raw_html_body: None,
                    attachments: None,
                    direction: MessageDirection::Outbound,
                    role: MessageRole::Agent,
                    thread_index: None,
                    created_at: chrono::Utc::now().naive_utc(),
                })
                .await?;
        }
        Ok(())
    }

    /// Write the audit payload back onto the task, then close it out — failing it (for retry) when
    /// the primary agent errored.
    async fn record_dispatch_outcome(
        &self,
        ingest: &InboundIngestResult,
        parsed: &ParsedEmail,
        claim: &ActiveClaim,
        run: &AgentRun<'_>,
        delivery: &OutboundDelivery,
        response: &str,
        metadata: &Option<serde_json::Value>,
    ) -> AppResult<()> {
        if let Some(task_id) = ingest.task_id {
            let payload =
                self.dispatch_audit_payload(ingest, parsed, run, delivery, response, metadata);
            match claim.worker_id {
                Some(worker_id) => {
                    let _ = self
                        .task_persistence
                        .update_claimed_task_payload(task_id, worker_id, payload)
                        .await;
                }
                None => {
                    let _ = self
                        .task_persistence
                        .update_task_payload(task_id, payload)
                        .await;
                }
            }
        }

        if let Some(ref error) = run.primary_error {
            if let (Some(task_id), Some(worker_id)) = (ingest.task_id, claim.owned_worker_id) {
                let _ = self
                    .task_persistence
                    .mark_task_failed(
                        task_id,
                        worker_id,
                        error,
                        chrono::Utc::now().naive_utc(),
                        true,
                    )
                    .await;
            }
            return Err(AppError::Internal(error.clone()));
        }

        if let Some(task_id) = ingest.task_id {
            self.task_persistence.complete_outreach(task_id).await?;
        }
        if let (Some(task_id), Some(worker_id)) = (ingest.task_id, claim.owned_worker_id) {
            let _ = self
                .task_persistence
                .mark_task_completed(task_id, worker_id)
                .await;
        }
        Ok(())
    }

    fn dispatch_audit_payload(
        &self,
        ingest: &InboundIngestResult,
        parsed: &ParsedEmail,
        run: &AgentRun<'_>,
        delivery: &OutboundDelivery,
        response: &str,
        metadata: &Option<serde_json::Value>,
    ) -> serde_json::Value {
        let execution_parameters = match &run.primary_params {
            Some(params) => {
                let mut config = params.config().clone();
                scrub_json_secrets(Some(&mut config));
                serde_json::json!({
                    "provider": params.provider(),
                    "model": params.model(),
                    "agent_id": run.primary_agent.as_ref().map(|a| a.id),
                    "agent_name": run.primary_agent.as_ref().map(|a| a.name.as_str()),
                    "prompt": parsed.prompt_text,
                    "config": config,
                    "executed_at": chrono::Utc::now().naive_utc().to_string(),
                })
            }
            None => serde_json::json!({
                "prompt": parsed.prompt_text,
                "executed_at": chrono::Utc::now().naive_utc().to_string(),
            }),
        };

        let mut execution_result = serde_json::json!({
            "response": response,
            "email_sent": delivery.email_sent,
            "outbound_message_id": delivery.message_id,
            "error": run.primary_error,
            "token_usage": TokenUsage::new(run.prompt_tokens, run.completion_tokens),
        });
        if let (Some(meta), Some(object)) = (metadata, execution_result.as_object_mut()) {
            object.insert("metadata".to_string(), meta.clone());
        }

        let mut payload = durable_ingest_payload(ingest);
        if let Some(object) = payload.as_object_mut() {
            object.insert("execution_parameters".to_string(), execution_parameters);
            object.insert("execution_result".to_string(), execution_result);
        }
        payload
    }
}

fn context_only_message_id(ingest: &InboundIngestResult) -> Option<&str> {
    if let Some(parsed) = ingest.parsed_email.as_ref()
        && parsed.is_context_only
    {
        return Some(parsed.message_id.as_str());
    }
    if let Some(norm) = ingest.normalized_message.as_ref()
        && norm.is_context_only
    {
        return Some(norm.message_id.as_str());
    }
    None
}

/// Matches recorded by ingest, falling back to the legacy single-channel fields on payloads
/// enqueued before pipelines existed.
fn channel_matches_of(ingest: &InboundIngestResult) -> Option<Vec<ChannelMatch>> {
    if !ingest.channel_matches.is_empty() {
        return Some(ingest.channel_matches.clone());
    }
    let (company, channel, thread, inbound_message) = (
        ingest.company.as_ref()?,
        ingest.channel.as_ref()?,
        ingest.thread.as_ref()?,
        ingest.inbound_message.as_ref()?,
    );
    Some(vec![ChannelMatch {
        company: company.clone(),
        channel: channel.clone(),
        matched_slug: None,
        thread: thread.clone(),
        inbound_message: inbound_message.clone(),
        recipient_role: RecipientRole::To,
        step_index: 0,
        total_steps: 1,
    }])
}

fn combine_metadata(outputs: &[AgentOutput<'_>]) -> Option<serde_json::Value> {
    if let [only] = outputs {
        return only.metadata.clone();
    }
    let map: serde_json::Map<String, serde_json::Value> = outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| {
            output.metadata.clone().map(|meta| {
                (
                    format!("step_{}_{}", index + 1, output.channel_match.channel.slug),
                    meta,
                )
            })
        })
        .collect();
    (!map.is_empty()).then_some(serde_json::Value::Object(map))
}
