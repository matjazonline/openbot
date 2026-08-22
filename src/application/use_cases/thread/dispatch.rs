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
    adapters::persistence::task::{OutboundSend, TASK_LEASE_SECONDS, report_outcome},
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        channel::{ChannelType, PUBLIC_PARTICIPANT, ParticipantIdentity},
        company_member::CompanyMembership,
        message::{Message, MessageDirection, MessageRole},
        message_contract::NormalizedOutboundMessage,
        task::TokenUsage,
        value_objects::{EmailAddress, MessageId},
    },
    services::{
        agent_channel_tool::AgentChannelToolContext,
        agent_runner::{
            AgentExecutionDisposition, AgentRunner, ApprovalContext as AgentRunnerApprovalContext,
            ResolvedAgentParams,
        },
        email_parser::ParsedEmail,
        outbound_dispatcher::{
            OutboundDispatcher, OutboundEmail, SentEmailResult, agent_response_email_body,
        },
        outreach_tool::OutreachToolContext,
    },
};

use super::{
    AgentExecutionResult, ChannelMatch, InboundIngestResult, RecipientRole, ThreadUseCases,
    durable_ingest_payload, scrub_json_secrets,
    support::{DirectoryCache, outbound_reference_ids},
};

/// The worker running this dispatch, which already holds the task's lease and will close the task
/// out itself once this returns.
#[derive(Debug, Clone, Copy)]
struct ActiveClaim {
    worker_id: Uuid,
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
    pub async fn execute_claimed_agent_task_and_dispatch(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
        worker_id: Uuid,
    ) -> AppResult<Option<AgentExecutionResult>> {
        if let Some(message_id) = context_only_message_id(ingest) {
            info!("Skipping agent execution for context-only message ID {message_id}");
            return Ok(None);
        }
        self.run_claimed_dispatch(ingest, send_email, &ActiveClaim { worker_id })
            .await
    }

    /// The dispatch proper: run the agents, deliver the reply, and record what happened on the
    /// task. Marking the task done is the worker's job, not this one's.
    async fn run_claimed_dispatch(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
        claim: &ActiveClaim,
    ) -> AppResult<Option<AgentExecutionResult>> {
        let Some(parsed) = ingest.parsed_email.as_ref() else {
            return Ok(None);
        };
        let Some(matches) = channel_matches_of(ingest) else {
            return Ok(None);
        };

        let Some(run) = self.run_agents(&matches, parsed, ingest, claim).await? else {
            info!("Agent execution suspended for task approval or outreach");
            return Ok(None);
        };

        let response = self.combine_responses(&run.outputs);
        let metadata = combine_metadata(&run.outputs);
        let delivery = self
            .deliver_agent_response(&matches, parsed, ingest, claim, &response, send_email)
            .await?;

        self.save_outbound_messages(&matches, &delivery, &response)
            .await?;
        self.record_dispatch_outcome(ingest, parsed, claim, &run, &delivery, &response, &metadata)
            .await?;

        Ok(Some(AgentExecutionResult {
            outbound_message_id: Some(delivery.message_id),
            agent_response: response,
            email_sent: delivery.email_sent,
            token_usage: Some(TokenUsage::new(run.prompt_tokens, run.completion_tokens)),
            metadata,
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
        let mut membership_cache: HashMap<(Uuid, String), CompanyMembership> = HashMap::new();

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
            let membership = match membership_cache.get(&member_key) {
                Some(cached) => *cached,
                None => {
                    let loaded = self
                        .company_persistence
                        .membership_for_email(channel_match.company.id, sender)
                        .await?;
                    membership_cache.insert(member_key, loaded);
                    loaded
                }
            };
            let access = channel_match.channel.participant_access(sender, membership);

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
                    if let Some(task_id) = ingest.task_id {
                        runner = runner.outreach_tool(
                            self.task_persistence.clone(),
                            self.channel_persistence.clone(),
                            self.outreach_context_for(
                                channel_match,
                                parsed,
                                task_id,
                                claim.worker_id,
                            ),
                        );
                        // The address book only makes sense alongside the tool that uses it.
                        if let Some(agent_persistence) = self.agent_persistence() {
                            runner = runner.agent_directory(agent_persistence.clone());
                        }
                        if let (Some(provisioning), Some(agent)) =
                            (self.agent_channel_provisioning.clone(), agent.as_ref())
                        {
                            runner = runner.agent_channel_tool(
                                provisioning,
                                AgentChannelToolContext {
                                    company_id: channel_match.company.id,
                                    company_slug: channel_match.company.slug.clone(),
                                    source_agent_id: agent.id,
                                    source_agent_name: agent.name.clone(),
                                    source_channel_id: channel_match.channel.id,
                                    task_id,
                                    app_domain_name: self.config.app_domain_name.clone(),
                                },
                            );
                        }
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
            return self.simulated_delivery(primary, parsed).await;
        }

        let references = outbound_reference_ids(parsed);
        let recipients_cc = self.outbound_cc_for(primary, parsed).await?;

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
            body_text: agent_response_email_body(response),
            hop_count: parsed.hop_count,
            trace_channels: parsed.trace_channels.clone(),
        };

        // The lease must still be ours at the moment of sending, or two workers could both reply.
        if let Some(task_id) = ingest.task_id {
            let renewed = self
                .task_persistence
                .renew_task_lease(
                    task_id,
                    claim.worker_id,
                    chrono::Utc::now() + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                )
                .await?;
            if !renewed {
                return Err(AppError::Internal(
                    "Task lease was lost before outbound dispatch".into(),
                ));
            }
        }

        let sent = self
            .queue_agent_reply(outbound_email, ingest, primary.company.id)
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

    /// Who the agent's reply is copied to: whoever the inbound mail copied, plus this channel's
    /// participants (other than `@public` and the sender).
    ///
    /// A channel with `add_3rd_party` off first loses every outsider from the inbound Cc line, so
    /// the reply reaches only the platform's own addresses and the channel's own people.
    pub(super) async fn outbound_cc_for(
        &self,
        primary: &ChannelMatch,
        parsed: &ParsedEmail,
    ) -> AppResult<Vec<String>> {
        let mut recipients_cc = self.inbound_cc_for(primary, parsed).await?;
        let Some(participants) = primary.channel.participant_emails.as_ref() else {
            return Ok(recipients_cc);
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
        Ok(recipients_cc)
    }

    /// The inbound Cc line, minus the outsiders a channel with `add_3rd_party` off refuses to
    /// copy. Platform addresses survive the filter — pipeline steps ride the Cc line.
    async fn inbound_cc_for(
        &self,
        primary: &ChannelMatch,
        parsed: &ParsedEmail,
    ) -> AppResult<Vec<String>> {
        if primary.channel.add_3rd_party {
            return Ok(parsed.recipients_cc.clone());
        }
        let mut directory = DirectoryCache::new(self);
        let mut kept = Vec::with_capacity(parsed.recipients_cc.len());
        for address in &parsed.recipients_cc {
            if self
                .is_third_party_address(address, &parsed.sender, &mut directory)
                .await?
            {
                continue;
            }
            kept.push(address.clone());
        }
        Ok(kept)
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

    /// Hand the agent's reply to the transport, exactly once per task.
    ///
    /// Nothing is sent here. The reply is written to the outbox and the poller delivers it, so a
    /// delivery problem is retried on its own — never by re-running the agent, which would mean a
    /// second LLM call and possibly different reply text for the same customer email.
    ///
    /// The returned [`SentEmailResult`] is *prepared*, not sent: `prepare_idempotent` derives the
    /// Message-ID from the same stable key the poller will send under, so the caller can persist
    /// the outbound message immediately and still match the mail that eventually goes out.
    ///
    /// Two workers racing the same task both compute the same key, so the unique index admits one
    /// row — and because they also compute the same Message-ID, the loser's thread write collapses
    /// on `ON CONFLICT (company_id, message_id)` rather than duplicating.
    ///
    /// A task-less caller (direct ingest) has nothing to key on and sends inline as before.
    async fn queue_agent_reply(
        &self,
        outbound_email: OutboundEmail,
        ingest: &InboundIngestResult,
        company_id: Uuid,
    ) -> AppResult<SentEmailResult> {
        let Some(task_id) = ingest.task_id else {
            return self.send_or_route_internally(outbound_email, None).await;
        };
        let idempotency_key = format!("task:{task_id}:agent-reply");
        let prepared = OutboundDispatcher::prepare_idempotent(
            &self.config,
            outbound_email.clone(),
            &idempotency_key,
        )?;

        let queued = self
            .task_persistence
            .enqueue_outbound_send(OutboundSend {
                company_id,
                channel_id: outbound_email.channel_id,
                task_id: Some(task_id),
                idempotency_key,
                payload: serde_json::to_value(&outbound_email).map_err(|error| {
                    AppError::Internal(format!(
                        "Could not serialise the reply for delivery: {error}"
                    ))
                })?,
            })
            .await?;

        match queued {
            Some(outbox_id) => info!("Queued agent reply for task {task_id} as outbox {outbox_id}"),
            None => warn!(
                "Agent reply for task {task_id} is already queued or delivered; not queueing again"
            ),
        }

        Ok(prepared)
    }

    async fn simulated_delivery(
        &self,
        primary: &ChannelMatch,
        parsed: &ParsedEmail,
    ) -> AppResult<OutboundDelivery> {
        let message_id = format!(
            "<simulated-test-{}@{}>",
            Uuid::new_v4(),
            self.config.app_domain_name
        );
        info!(
            "Simulation test mode (Run_Test): Skipped SMTP email dispatch for Message-ID {message_id}"
        );
        Ok(OutboundDelivery {
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
            recipients_cc: self.outbound_cc_for(primary, parsed).await?,
            subject: if parsed.subject.to_lowercase().starts_with("re:") {
                parsed.subject.clone()
            } else {
                format!("Re: {}", parsed.subject)
            },
            email_sent: false,
        })
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
                    created_at: chrono::Utc::now(),
                })
                .await?;
        }
        Ok(())
    }

    /// Write the audit payload back onto the task, and surface a failed agent run to the worker,
    /// which owns the retry decision.
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
            let outcome = self
                .task_persistence
                .update_claimed_task_payload(task_id, claim.worker_id, payload)
                .await;
            report_outcome("Task", task_id, "payload update", outcome);
        }

        // Failing the run is reported upwards rather than written here: the worker owns the lease
        // and turns this error into the retry-or-dead-letter decision in `close_out_task`.
        if let Some(ref error) = run.primary_error {
            return Err(AppError::Internal(error.clone()));
        }

        if let Some(task_id) = ingest.task_id {
            self.task_persistence.complete_outreach(task_id).await?;
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
                    "executed_at": chrono::Utc::now().to_rfc3339(),
                })
            }
            None => serde_json::json!({
                "prompt": parsed.prompt_text,
                "executed_at": chrono::Utc::now().to_rfc3339(),
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
