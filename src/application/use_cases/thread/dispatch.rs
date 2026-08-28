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

use crate::entities::task::{TaskLeaseRef, TaskSuspension};

use crate::{
    adapters::persistence::task::{
        AgentDispatchCommit, DispatchCommit, OutboundSend, TASK_LEASE_SECONDS,
    },
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        channel::{ChannelType, PUBLIC_PARTICIPANT, ParticipantIdentity},
        company_member::CompanyMembership,
        memory::{MAX_MEMORY_UPSTREAM_CONTEXT_CHARS, truncate_memory_text},
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
        memory_coordinator::{MemoryPersistInput, MemoryRecallAudience, MemoryRecallInput},
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

/// One agent's contribution to the reply.
struct AgentOutput<'a> {
    channel_match: &'a ChannelMatch,
    agent: Option<Agent>,
    memory_user_context: String,
    content: String,
    metadata: Option<serde_json::Value>,
}

/// Everything the agent phase produced, before anything is sent.
struct AgentRun<'a> {
    outputs: Vec<AgentOutput<'a>>,
    prompt_tokens: usize,
    completion_tokens: usize,
    /// The first agent error from any matched channel. Its presence means the whole logical task
    /// failed: no reply is committed for any channel, and the worker retries the task.
    failure: Option<String>,
    primary_params: Option<ResolvedAgentParams>,
    primary_agent: Option<Agent>,
}

/// What one dispatch did. `Option<AgentExecutionResult>` used to carry this, which conflated a
/// message there was nothing to do for with a task an agent deliberately parked -- and the worker
/// has to close those out differently.
#[derive(Debug)]
pub enum DispatchOutcome {
    /// Nothing about this message needed an agent; the task is complete.
    Skipped,
    /// An agent parked the task awaiting an approval or an outreach reply.
    Suspended,
    /// The agents ran and their reply was committed.
    Replied(Box<AgentExecutionResult>),
}

/// One dispatch's result, worked out but not yet written.
///
/// Kept together because the three parts are one effect: the messages describe the reply, the
/// outbound send delivers it, and both are derived from the same `delivery`.
struct PreparedDispatch {
    delivery: OutboundDelivery,
    /// `None` for a simulated run, and for a task-less caller that already sent inline.
    outbound: Option<OutboundSend>,
    messages: Vec<Message>,
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
        lease: TaskLeaseRef,
    ) -> AppResult<DispatchOutcome> {
        if let Some(message_id) = context_only_message_id(ingest) {
            info!("Skipping agent execution for context-only message ID {message_id}");
            return Ok(DispatchOutcome::Skipped);
        }
        self.run_claimed_dispatch(ingest, send_email, lease).await
    }

    /// The dispatch proper: run the agents, deliver the reply, and record what happened on the
    /// task. Marking the task done is the worker's job, not this one's.
    async fn run_claimed_dispatch(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
        lease: TaskLeaseRef,
    ) -> AppResult<DispatchOutcome> {
        let Some(parsed) = ingest.parsed_email.as_ref() else {
            return Ok(DispatchOutcome::Skipped);
        };
        let Some(matches) = channel_matches_of(ingest) else {
            return Ok(DispatchOutcome::Skipped);
        };

        let Some(run) = self.run_agents(&matches, parsed, ingest, lease).await? else {
            info!("Agent execution suspended for task approval or outreach");
            return Ok(DispatchOutcome::Suspended);
        };

        // Nothing is committed for a failed run. The reply, the thread messages and the outbox
        // row are one logical effect: if any matched agent failed, none of them may land, or a
        // retry would deliver a second, contradictory answer into the same thread.
        if let Some(error) = run.failure {
            return Err(AppError::Internal(error));
        }

        let response = self.combine_responses(&run.outputs);
        let metadata = combine_metadata(&run.outputs);
        let (delivery, outbound) = self
            .deliver_agent_response(&matches, parsed, ingest, lease, &response, send_email)
            .await?;
        let messages = Self::outbound_messages(&matches, &delivery, &response);

        let outbound_message_id = delivery.message_id.clone();
        let email_sent = delivery.email_sent;
        self.commit_dispatch(
            ingest,
            parsed,
            lease,
            &run,
            PreparedDispatch {
                delivery,
                outbound,
                messages,
            },
            &response,
            &metadata,
        )
        .await?;
        self.persist_memories(ingest, parsed, &run).await;

        Ok(DispatchOutcome::Replied(Box::new(AgentExecutionResult {
            outbound_message_id: Some(outbound_message_id),
            agent_response: response,
            email_sent,
            token_usage: Some(TokenUsage::new(run.prompt_tokens, run.completion_tokens)),
            metadata,
        })))
    }

    /// Run every matched channel's agent in pipeline order. `Ok(None)` means an agent suspended
    /// itself awaiting approval or an outreach reply, and the task stays open.
    async fn run_agents<'a>(
        &self,
        matches: &'a [ChannelMatch],
        parsed: &ParsedEmail,
        ingest: &InboundIngestResult,
        lease: TaskLeaseRef,
    ) -> AppResult<Option<AgentRun<'a>>> {
        let mut run = AgentRun {
            outputs: Vec::with_capacity(matches.len()),
            prompt_tokens: 0,
            completion_tokens: 0,
            failure: None,
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

            let memory_user_context = match upstream_context.as_deref() {
                Some(upstream) => format!("{upstream}\n\n{}", parsed.prompt_text),
                None => parsed.prompt_text.clone(),
            };
            let mut agent_prompt = parsed.prompt_text.clone();
            if let Some(memory) = self.memory.as_ref() {
                let task_id = ingest.task_id.ok_or_else(|| {
                    AppError::Internal("Memory recall requires a durable task id.".into())
                })?;
                if let Some(context) = memory
                    .recall(MemoryRecallInput {
                        company: &channel_match.company,
                        channel: &channel_match.channel,
                        agent: agent.as_ref(),
                        sender: Some(&parsed.sender),
                        audience: if membership.is_team() {
                            MemoryRecallAudience::MemberOrSystem
                        } else {
                            MemoryRecallAudience::External
                        },
                        task_id,
                        latest_prompt: &parsed.prompt_text,
                    })
                    .await?
                {
                    agent_prompt.push_str("\n\n");
                    agent_prompt.push_str(&context);
                }
            }

            let result = match &params {
                Ok(params) => {
                    let mut runner = AgentRunner::new(&agent_prompt, params)
                        .subject(Some(&parsed.subject))
                        .history(&history)
                        .approval_use_cases(self.approval_use_cases.clone())
                        .approval_context(Some(
                            self.approval_context_for(channel_match, ingest, lease)
                                .await,
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
                                lease.worker_id,
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
                        agent,
                        memory_user_context,
                        content: output.content,
                        metadata: output.metadata,
                    });
                }
                Err(err) => {
                    // Deliberately no `AgentOutput`: a provider failure is an operational fact,
                    // not something an agent said. Pushing it here is how it used to reach the
                    // reply body, the thread history and the outbox.
                    if run.failure.is_none() {
                        run.failure = Some(err.to_string());
                    }
                }
            }
        }

        Ok(Some(run))
    }

    async fn persist_memories(
        &self,
        ingest: &InboundIngestResult,
        parsed: &ParsedEmail,
        run: &AgentRun<'_>,
    ) {
        if run.failure.is_some() {
            return;
        }
        let (Some(memory), Some(task_id)) = (self.memory.as_ref(), ingest.task_id) else {
            return;
        };
        for output in &run.outputs {
            memory
                .persist(MemoryPersistInput {
                    company: &output.channel_match.company,
                    channel: &output.channel_match.channel,
                    agent: output.agent.as_ref(),
                    sender: Some(&parsed.sender),
                    task_id,
                    user_context: &output.memory_user_context,
                    final_answer: &output.content,
                })
                .await;
        }
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
        lease: TaskLeaseRef,
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
            // Only a task-driven run has a task to park; a direct ingest has no task row at
            // all. When it does, it parks itself under its own lease.
            suspension: ingest
                .task_id
                .is_some()
                .then_some(TaskSuspension::Leased(lease)),
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
        let mut truncated = false;
        for output in previous {
            let header = format!(
                "--- Step {step}: {name} ({slug}) ---\n",
                step = output.channel_match.step_index + 1,
                name = output.channel_match.channel.name,
                slug = output.channel_match.channel.slug,
            );
            if !append_bounded_upstream(&mut context, &header)
                || !append_bounded_upstream(&mut context, &output.content)
                || !append_bounded_upstream(&mut context, "\n\n")
            {
                truncated = true;
                break;
            }
        }
        if !truncated
            && let Some(task_id) = task_id
            && let Some(outreach_context) =
                self.task_persistence.get_outreach_context(task_id).await?
        {
            truncated = !append_bounded_upstream(&mut context, "--- Outreach Progress ---\n")
                || !append_bounded_upstream(&mut context, &outreach_context)
                || !append_bounded_upstream(&mut context, "\n\n");
        }
        if truncated && let Some(monitoring) = self.monitoring.as_ref() {
            monitoring.increment_counter(
                "memory_truncations_total",
                1,
                &[("operation", "persist"), ("field", "upstream_context")],
            );
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
        lease: TaskLeaseRef,
        response: &str,
        send_email: bool,
    ) -> AppResult<(OutboundDelivery, Option<OutboundSend>)> {
        let primary = &matches[0];
        if !send_email {
            return Ok((self.simulated_delivery(primary, parsed).await?, None));
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
        if ingest.task_id.is_some() {
            let renewed = self
                .task_persistence
                .renew_task_lease(
                    lease,
                    chrono::Utc::now() + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                )
                .await?;
            if !renewed {
                return Err(AppError::Internal(
                    "Task lease was lost before outbound dispatch".into(),
                ));
            }
        }

        let (sent, pending) = self
            .prepare_agent_reply(outbound_email, ingest, primary.company.id)
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

        Ok((
            OutboundDelivery {
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
            },
            pending,
        ))
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

    /// Work out how the agent's reply will go out, without writing anything.
    ///
    /// Nothing is sent here, and for a task-driven run nothing is queued here either: the caller
    /// commits the returned [`OutboundSend`] together with the reply messages and the task
    /// payload, so the three land or fail as one. The poller then delivers it, which is why a
    /// delivery problem is retried on its own -- never by re-running the agent, which would mean
    /// a second LLM call and possibly different reply text for the same customer email.
    ///
    /// The returned [`SentEmailResult`] is *prepared*, not sent: `prepare_idempotent` derives the
    /// Message-ID from the same stable key the poller will send under, so the caller can persist
    /// the outbound message immediately and still match the mail that eventually goes out.
    ///
    /// Two workers racing the same task both compute the same key, so the unique index admits one
    /// row — and because they also compute the same Message-ID, the loser's thread write collapses
    /// on `ON CONFLICT (company_id, message_id)` rather than duplicating.
    ///
    /// A task-less caller (direct ingest) has nothing to key on and sends inline as before, so it
    /// gets back no pending send.
    async fn prepare_agent_reply(
        &self,
        outbound_email: OutboundEmail,
        ingest: &InboundIngestResult,
        company_id: Uuid,
    ) -> AppResult<(SentEmailResult, Option<OutboundSend>)> {
        let Some(task_id) = ingest.task_id else {
            let sent = self.send_or_route_internally(outbound_email, None).await?;
            return Ok((sent, None));
        };
        let idempotency_key = format!("task:{task_id}:agent-reply");
        let prepared = OutboundDispatcher::prepare_idempotent(
            &self.config,
            outbound_email.clone(),
            &idempotency_key,
        )?;

        let pending = OutboundSend {
            company_id,
            channel_id: outbound_email.channel_id,
            task_id: Some(task_id),
            idempotency_key,
            payload: serde_json::to_value(&outbound_email).map_err(|error| {
                AppError::Internal(format!(
                    "Could not serialise the reply for delivery: {error}"
                ))
            })?,
        };

        Ok((prepared, Some(pending)))
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

    /// The reply as it will be stored in every thread it answered, so each channel's history
    /// stays complete.
    ///
    /// Built rather than written: these go into the dispatch commit alongside the outbox row and
    /// the task payload, because a reply visible in a thread but never queued for delivery -- or
    /// queued but invisible -- is worse than neither.
    fn outbound_messages(
        matches: &[ChannelMatch],
        delivery: &OutboundDelivery,
        response: &str,
    ) -> Vec<Message> {
        matches
            .iter()
            .map(|channel_match| Message {
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
            .collect()
    }

    /// Make this dispatch durable: the reply in every thread it answered, the outbox row that
    /// delivers it, and the audit payload on the task, as one lease-fenced transaction.
    ///
    /// A failed run never reaches here -- it is rejected before delivery -- so this only ever
    /// commits a run that produced a real reply.
    async fn commit_dispatch(
        &self,
        ingest: &InboundIngestResult,
        parsed: &ParsedEmail,
        lease: TaskLeaseRef,
        run: &AgentRun<'_>,
        commit: PreparedDispatch,
        response: &str,
        metadata: &Option<serde_json::Value>,
    ) -> AppResult<()> {
        let PreparedDispatch {
            delivery,
            outbound,
            messages,
        } = commit;

        // A task-less caller (direct ingest) has no lease to fence on and no payload to write, so
        // its reply is stored on its own. It also has no outbox row: it sent inline.
        let Some(task_id) = ingest.task_id else {
            for message in &messages {
                self.thread_persistence.create_message(message).await?;
            }
            return Ok(());
        };

        let payload =
            self.dispatch_audit_payload(ingest, parsed, run, &delivery, response, metadata);

        match self
            .task_persistence
            .commit_agent_dispatch(AgentDispatchCommit {
                lease,
                messages: &messages,
                outbound,
                payload,
                complete_outreach: true,
            })
            .await?
        {
            DispatchCommit::Committed { outbox_id } => {
                match outbox_id {
                    Some(outbox_id) => {
                        info!("Queued agent reply for task {task_id} as outbox {outbox_id}")
                    }
                    None => warn!(
                        "Agent reply for task {task_id} was already queued or delivered; not queueing again"
                    ),
                }
                Ok(())
            }
            // Nothing was written. Reporting this as an error is what makes the task retryable:
            // the run that now owns the lease will produce the reply.
            DispatchCommit::LeaseLost => Err(AppError::Internal(
                "Task lease was lost before the dispatch could be committed".into(),
            )),
        }
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
            "error": run.failure,
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

fn append_bounded_upstream(context: &mut String, value: &str) -> bool {
    let used = context.chars().count();
    let remaining = MAX_MEMORY_UPSTREAM_CONTEXT_CHARS.saturating_sub(used);
    if remaining == 0 {
        return value.is_empty();
    }
    let (bounded, truncated) = truncate_memory_text(value, remaining);
    context.push_str(&bounded);
    !truncated
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

#[cfg(test)]
mod memory_bound_tests {
    use super::*;
    use crate::entities::memory::MEMORY_TRUNCATION_MARKER;

    #[test]
    fn upstream_context_is_unicode_safe_at_and_over_the_limit() {
        let mut exact = String::new();
        assert!(append_bounded_upstream(
            &mut exact,
            &"🦀".repeat(MAX_MEMORY_UPSTREAM_CONTEXT_CHARS)
        ));
        assert_eq!(exact.chars().count(), MAX_MEMORY_UPSTREAM_CONTEXT_CHARS);

        let mut over = String::new();
        assert!(!append_bounded_upstream(
            &mut over,
            &"🦀".repeat(MAX_MEMORY_UPSTREAM_CONTEXT_CHARS + 1)
        ));
        assert_eq!(over.chars().count(), MAX_MEMORY_UPSTREAM_CONTEXT_CHARS);
        assert!(over.ends_with(MEMORY_TRUNCATION_MARKER));
    }
}
