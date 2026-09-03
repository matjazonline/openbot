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
        approval::ApprovalSubject,
        channel::{PUBLIC_PARTICIPANT, ParticipantAccess},
        correlation::CorrelationId,
        memory::{MAX_MEMORY_UPSTREAM_CONTEXT_CHARS, truncate_memory_text},
        message::{CanonicalMessageId, MessageDirection, MessageRole},
        task::TokenUsage,
        transport::PrincipalId,
        value_objects::{EmailAddress, MessageId},
    },
    services::{
        agent_channel_tool::AgentChannelToolContext,
        agent_runner::{
            AgentExecutionDisposition, AgentRunner, ResolvedAgentParams, resolve_agent_params,
        },
        memory_coordinator::{MemoryPersistInput, MemoryRecallAudience, MemoryRecallInput},
        outbound_dispatcher::{OutboundEmail, SentEmailResult, agent_response_email_body},
        outreach_tool::OutreachToolContext,
    },
    transport::InboundEnvelope,
};

use super::{
    AgentAuthor, AgentExecutionResult, ChannelMatch, InboundIngestResult, MessageAuthorWrite,
    MessageCorrelation, MessageWrite, PipelineStep, RecipientRole, ReplyDelivery, ThreadUseCases,
    scrub_json_secrets,
    support::{DirectoryCache, build_prompt_text, outbound_reference_ids, rfc_message_id},
};

/// How this dispatch's reply reaches the outside world.
///
/// Replaces a `send_email: bool` crossed with `ingest.task_id: Option<Uuid>`. That matrix had two
/// combinations nothing could mean -- a durable send with no task to key it on, and a simulated
/// send that still queued an outbox row -- and both compiled. Here the task id lives inside the one
/// variant that has one, so a durable send cannot be requested without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplyDeliveryMode {
    /// The agents run and their answer is recorded, but nothing is handed to a transport: a
    /// mailbox send the user marked as a test, or a simulation.
    Simulated,
    /// Nothing durable to key an idempotent send on, so the send happens inline in this call.
    /// Only a direct ingest with no task row takes this path.
    Direct,
    /// Queued in the same transaction as the reply and delivered by the outbox poller, under a key
    /// derived from the task so two workers racing the same task queue one send.
    Durable { task_id: Uuid },
}

impl ReplyDeliveryMode {
    /// What the message asked for at ingest, crossed with whether this run owns a durable task.
    ///
    /// One place, so no call site re-derives it: the ingest decision alone cannot tell direct from
    /// durable, and the task id alone cannot tell either from simulated.
    const fn resolve(delivery: ReplyDelivery, task_id: Option<Uuid>) -> Self {
        match (delivery, task_id) {
            (ReplyDelivery::InAppOnly, _) => Self::Simulated,
            (ReplyDelivery::Send, Some(task_id)) => Self::Durable { task_id },
            (ReplyDelivery::Send, None) => Self::Direct,
        }
    }

    /// Whether anything is actually handed to a transport.
    const fn reaches_a_transport(self) -> bool {
        !matches!(self, Self::Simulated)
    }
}

/// Whether a run may skip the inbound guardrail.
///
/// Trust is a property of the *sender*: [`Channel::participant_access`] grants it to a listed
/// channel participant or to a member of the owning company's team. A forward carries text that
/// sender did not write and that no authentication covers -- DMARC passed for the forwarder, not
/// for whoever wrote the quoted body, and [`strip_quoted_history`] deliberately keeps the whole
/// quoted chain intact on a forward. A teammate forwarding a hostile email is therefore the one
/// shape where the envelope is trusted and the payload is not, so it does not inherit the
/// forwarder's trust.
///
/// [`Channel::participant_access`]: crate::entities::channel::Channel::participant_access
/// [`strip_quoted_history`]: super::support::strip_quoted_history
fn guardrail_may_be_skipped(access: &ParticipantAccess, envelope: &InboundEnvelope) -> bool {
    access.trusted && !envelope.directives.is_forwarded
}

/// The RFC id an outbound reply answers.
///
/// One of the three helpers below that are deliberately email-shaped: they build the *delivery
/// intent* -- the `In-Reply-To`, the `From` and the `Cc` a mail goes out with -- and no longer
/// touch the stored reply, which carries none of it. They are here rather than in the mail adapter
/// only until the generic delivery model gives them somewhere better to live.
///
/// A transport with no message key of its own is unreachable here -- every dispatch answers
/// something that arrived -- so the source key is used directly rather than defaulted.
fn trigger_message_id(envelope: &InboundEnvelope) -> MessageId {
    rfc_message_id(envelope)
        .cloned()
        .unwrap_or_else(|| MessageId::from(envelope.source.message_key.as_str()))
}

/// The address a reply goes back to.
fn sender_address(envelope: &InboundEnvelope) -> EmailAddress {
    EmailAddress::from(envelope.author.subject().as_str())
}

/// The `Cc` line as it arrived, in the order the message carried it.
fn inbound_cc_addresses(envelope: &InboundEnvelope) -> Vec<String> {
    envelope
        .addressed_in(RecipientRole::Cc)
        .map(|identity| identity.subject().as_str().to_string())
        .collect()
}

/// `Re:` a subject exactly once.
fn reply_subject(subject: &str) -> String {
    if subject.to_lowercase().starts_with("re:") {
        subject.to_string()
    } else {
        format!("Re: {subject}")
    }
}

/// One agent's contribution to the reply.
struct AgentOutput<'a> {
    channel_match: &'a ChannelMatch,
    agent: Option<Agent>,
    /// The actor this turn is attributed to, so persisting memory names the same principal recall
    /// read from rather than re-deriving it from a handle.
    subject_principal: Option<PrincipalId>,
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
    failure: Option<AppError>,
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
    reply: AgentReply,
}

/// The agent's answer: one canonical message, and every other thread it also answered.
///
/// The split is the point. `message` is written once; `also_in_threads` gets associations onto
/// that same row, so three channels reading the same answer are reading one message rather than
/// three copies that have to be kept byte-identical to stay recognisable as one.
pub struct AgentReply {
    pub message: MessageWrite,
    pub also_in_threads: Vec<Uuid>,
}

/// The outbound side of the reply, real or simulated.
///
/// What the *message* takes from a delivery, and nothing else. The addresses, the RFC ids and the
/// recipient lines used to travel through here on their way onto the stored reply; they now stop
/// at the delivery intent, which is where a fact about how an answer was sent belongs.
struct OutboundDelivery {
    /// The subject the answer went out under, which is also the subject it is filed under.
    subject: String,
    /// Whether anything was really handed to a transport. A simulated run answers in the thread
    /// and sends nothing, and the audit payload has to be able to say so.
    email_sent: bool,
}

struct AgentDelivery<'a> {
    matches: &'a [ChannelMatch],
    envelope: &'a InboundEnvelope,
    ingest: &'a InboundIngestResult,
    lease: TaskLeaseRef,
    response: &'a str,
    mode: ReplyDeliveryMode,
    correlation_id: CorrelationId,
}

/// What the task's audit payload is assembled from.
///
/// A struct because seven arguments of which three are references to run state is exactly the
/// call site nobody can read -- and because `response` and the delivery's own subject are both
/// `&str`.
struct DispatchAudit<'a, 'run> {
    ingest: &'a InboundIngestResult,
    envelope: &'a InboundEnvelope,
    run: &'a AgentRun<'run>,
    reply: &'a AgentReply,
    delivery: &'a OutboundDelivery,
    response: &'a str,
    metadata: &'a Option<serde_json::Value>,
}

struct DispatchCommitInput<'a, 'run> {
    ingest: &'a InboundIngestResult,
    envelope: &'a InboundEnvelope,
    lease: TaskLeaseRef,
    run: &'a AgentRun<'run>,
    commit: PreparedDispatch,
    response: &'a str,
    metadata: &'a Option<serde_json::Value>,
}

impl ThreadUseCases {
    /// `correlation_id` comes from the claimed task row rather than from the ingest payload it
    /// carries. Both describe the same chain, but the row is the durable one: it is `NOT NULL`,
    /// the queue maintains it across retries and resumes, and a payload assembled by anything
    /// other than `finalize_ingest` still gets a correct trail.
    pub async fn execute_claimed_agent_task_and_dispatch(
        &self,
        ingest: &InboundIngestResult,
        delivery: ReplyDelivery,
        lease: TaskLeaseRef,
        correlation_id: CorrelationId,
    ) -> AppResult<DispatchOutcome> {
        if let Some(message_id) = context_only_message_id(ingest) {
            info!("Skipping agent execution for context-only message ID {message_id}");
            return Ok(DispatchOutcome::Skipped);
        }
        let mode = ReplyDeliveryMode::resolve(delivery, ingest.task_id);
        // The guard above is the whole reason this function exists, so the dispatch it delegates to
        // is boxed rather than stored inline -- an `async fn` that only forwards still pays its
        // child future's size in stack. See `scripts/stack-frames.sh`.
        Box::pin(self.run_claimed_dispatch(ingest, mode, lease, correlation_id)).await
    }

    /// The dispatch proper: run the agents, deliver the reply, and record what happened on the
    /// task. Marking the task done is the worker's job, not this one's.
    async fn run_claimed_dispatch(
        &self,
        ingest: &InboundIngestResult,
        mode: ReplyDeliveryMode,
        lease: TaskLeaseRef,
        correlation_id: CorrelationId,
    ) -> AppResult<DispatchOutcome> {
        let Some(envelope) = ingest.envelope.as_deref() else {
            return Ok(DispatchOutcome::Skipped);
        };
        let Some(matches) = channel_matches_of(ingest) else {
            return Ok(DispatchOutcome::Skipped);
        };

        // The fattest of this function's children by a wide margin: it runs the agents.
        let Some(run) =
            Box::pin(self.run_agents(&matches, envelope, ingest, lease, correlation_id)).await?
        else {
            info!("Agent execution suspended for task approval or outreach");
            return Ok(DispatchOutcome::Suspended);
        };

        // Nothing is committed for a failed run. The reply, the thread messages and the outbox
        // row are one logical effect: if any matched agent failed, none of them may land, or a
        // retry would deliver a second, contradictory answer into the same thread.
        if let Some(error) = run.failure {
            return Err(error);
        }

        let response = self.combine_responses(&run.outputs);
        let metadata = combine_metadata(&run.outputs);
        let (delivery, outbound) = self
            .deliver_agent_response(AgentDelivery {
                matches: &matches,
                envelope,
                ingest,
                lease,
                response: &response,
                mode,
                correlation_id,
            })
            .await?;
        let reply = Self::agent_reply_write(
            &matches,
            run.primary_agent.as_ref(),
            &delivery,
            &response,
            correlation_id,
        )?;

        let reply_message_id = reply.message.id;
        let email_sent = delivery.email_sent;
        self.commit_dispatch(DispatchCommitInput {
            ingest,
            envelope,
            lease,
            run: &run,
            commit: PreparedDispatch {
                delivery,
                outbound,
                reply,
            },
            response: &response,
            metadata: &metadata,
        })
        .await?;
        self.persist_memories(ingest, &run).await;

        Ok(DispatchOutcome::Replied(Box::new(AgentExecutionResult {
            reply_message_id: Some(reply_message_id),
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
        envelope: &InboundEnvelope,
        ingest: &InboundIngestResult,
        lease: TaskLeaseRef,
        run_correlation_id: CorrelationId,
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

        for (index, channel_match) in matches.iter().enumerate() {
            let history = self
                .thread_persistence
                .list_agent_history(channel_match.thread.id)
                .await?;
            let agent = Some(
                self.first_agent_for(channel_match, &mut agent_cache)
                    .await?,
            );
            // Box the credential-resolution seam so this already-deep dispatch future does not
            // absorb another provider-facing async frame.
            let params = Box::pin(resolve_agent_params(
                self.company_persistence.as_ref(),
                &channel_match.company,
                agent.as_ref(),
            ))
            .await;
            if index == 0 {
                run.primary_params = params.as_ref().ok().cloned();
                run.primary_agent = agent.clone();
            }

            let prompt_text = build_prompt_text(envelope);
            let context = self
                .observe_ingress_identity(channel_match.company.id, &envelope.author)
                .await?;
            let membership = context.membership;
            // The actor behind the handle, resolved once here. Memory is scoped to it and nothing
            // downstream reads the handle string as a key.
            let subject_principal = context.principal_id;
            let access = channel_match.channel.participant_access(context);

            let upstream_context = self
                .upstream_context_for(&run.outputs, ingest.task_id)
                .await?;

            let memory_user_context = match upstream_context.as_deref() {
                Some(upstream) => format!("{upstream}\n\n{prompt_text}"),
                None => prompt_text.clone(),
            };
            let mut agent_prompt = prompt_text.clone();
            if let Some(memory) = self.memory.as_ref() {
                let task_id = ingest.task_id.ok_or_else(|| {
                    AppError::Internal("Memory recall requires a durable task id.".into())
                })?;
                if let Some(context) = memory
                    .recall(MemoryRecallInput {
                        company: &channel_match.company,
                        channel: &channel_match.channel,
                        agent: agent.as_ref(),
                        subject_principal,
                        audience: if membership.is_team() {
                            MemoryRecallAudience::MemberOrSystem
                        } else {
                            MemoryRecallAudience::External
                        },
                        task_id,
                        latest_prompt: &prompt_text,
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
                        .subject(Some(envelope.content.subject()))
                        .history(&history)
                        .approval_use_cases(self.approval_use_cases.clone())
                        .approval_context(Some(
                            self.approval_context_for(
                                channel_match,
                                ingest,
                                lease,
                                run_correlation_id,
                            )
                            .await,
                        ))
                        .monitoring(self.monitoring.clone())
                        .config(Some(self.config.clone()))
                        .company(Some(channel_match.company.clone()))
                        .skip_spam_guardrail(guardrail_may_be_skipped(&access, envelope))
                        .recipient_role(Some(channel_match.recipient_role))
                        .upstream_pipeline_context(upstream_context)
                        .ids(
                            Some(channel_match.company.id),
                            Some(channel_match.channel.id),
                            agent.as_ref().map(|a| a.id),
                        )
                        // After `ids`, which is where the hook context reads them from.
                        .trace(run_correlation_id, ingest.task_id);
                    if let Some(task_id) = ingest.task_id {
                        runner = runner.outreach_tool(
                            self.task_persistence.clone(),
                            self.channel_persistence.clone(),
                            self.outreach_context_for(
                                channel_match,
                                envelope,
                                task_id,
                                lease.worker_id,
                                run_correlation_id,
                            ),
                        );
                        // The address book only makes sense alongside the tool that uses it.
                        if let Some(agent_persistence) = self.agent_persistence() {
                            runner = runner.agent_directory(
                                agent_persistence.clone(),
                                self.binding_persistence.clone(),
                            );
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
                                    channel_defaults: channel_match
                                        .company
                                        .channel_defaults
                                        .clone(),
                                    spam_scanning: if self.config.is_spam_scan_enabled() {
                                        crate::use_cases::agent::SpamScanning::Available
                                    } else {
                                        crate::use_cases::agent::SpamScanning::Unavailable
                                    },
                                },
                            );
                        }
                    }
                    let run_timeout = agent
                        .as_ref()
                        .map(|agent| agent.run_timeout(self.agent_run_timeout))
                        .unwrap_or(self.agent_run_timeout);
                    // Boxed, not detached: dropping the `Timeout` still drops the provider call.
                    match tokio::time::timeout(run_timeout, Box::pin(runner.execute())).await {
                        Ok(result) => result.map_err(AppError::from),
                        Err(_) => Err(AppError::Timeout(format!(
                            "agent run exceeded the {}s limit",
                            run_timeout.as_secs()
                        ))),
                    }
                }
                Err(err) => Err(AppError::Internal(err.to_string())),
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
                        subject_principal,
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
                        run.failure = Some(err);
                    }
                }
            }
        }

        Ok(Some(run))
    }

    async fn persist_memories(&self, ingest: &InboundIngestResult, run: &AgentRun<'_>) {
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
                    subject_principal: output.subject_principal,
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
    ) -> AppResult<Agent> {
        let Some(persistence) = self.agent_persistence.as_ref() else {
            return Err(AppError::Internal(
                "Agent persistence is unavailable for an enabled channel.".into(),
            ));
        };
        let Some(&agent_id) = channel_match
            .channel
            .agent_ids
            .as_ref()
            .and_then(|ids| ids.first())
        else {
            return Err(AppError::Internal(format!(
                "Enabled channel '{}' has no active agent at position 0.",
                channel_match.channel.slug
            )));
        };
        // A miss is cached too, so a channel pointing at a deleted agent costs one lookup per
        // dispatch rather than one per matched channel. Both paths converge before the agent is
        // unwrapped, so "was not found" is stated once.
        let loaded = match cache.get(&agent_id).cloned() {
            Some(cached) => cached,
            None => {
                let loaded = persistence.get_by_id(agent_id).await?;
                cache.insert(agent_id, loaded.clone());
                loaded
            }
        };
        loaded.ok_or_else(|| {
            AppError::Internal(format!(
                "Active agent {agent_id} for channel '{}' was not found.",
                channel_match.channel.slug
            ))
        })
    }

    /// Approvals go to the first non-public channel participant, falling back to any company team
    /// member.
    async fn approval_context_for(
        &self,
        channel_match: &ChannelMatch,
        ingest: &InboundIngestResult,
        lease: TaskLeaseRef,
        correlation_id: CorrelationId,
    ) -> ApprovalSubject {
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

        ApprovalSubject {
            company_id: channel_match.company.id,
            channel_id: channel_match.channel.id,
            channel_name: channel_match.channel.name.clone(),
            channel_slug: channel_match.reply_slug(),
            company_slug: channel_match.company.slug.clone(),
            thread_id: Some(channel_match.thread.id),
            // Only a task-driven run has a task to park; a direct ingest has no task row at
            // all. When it does, it parks itself under its own lease.
            correlation_id,
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
        envelope: &InboundEnvelope,
        task_id: Uuid,
        worker_id: Uuid,
        correlation_id: CorrelationId,
    ) -> OutreachToolContext {
        OutreachToolContext {
            task_id,
            correlation_id,
            worker_id,
            company_id: channel_match.company.id,
            channel_id: channel_match.channel.id,
            channel_name: channel_match.channel.name.clone(),
            channel_slug: channel_match.reply_slug(),
            company_slug: channel_match.company.slug.clone(),
            trigger_message_id: trigger_message_id(envelope),
            thread_references: outbound_reference_ids(envelope),
            hop_count: envelope.directives.hop_count,
            trace_channels: envelope.directives.trace_channels.clone(),
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
                step = output.channel_match.step.index + 1,
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
        delivery: AgentDelivery<'_>,
    ) -> AppResult<(OutboundDelivery, Option<OutboundSend>)> {
        let AgentDelivery {
            matches,
            envelope,
            ingest,
            lease,
            response,
            mode,
            correlation_id,
        } = delivery;
        let primary = &matches[0];
        if !mode.reaches_a_transport() {
            return Ok((self.simulated_delivery(primary, envelope).await?, None));
        }

        let references = outbound_reference_ids(envelope);
        let recipients_cc = self.outbound_cc_for(primary, envelope).await?;

        let outbound_email = OutboundEmail {
            channel_id: primary.channel.id,
            channel_name: primary.channel.name.clone(),
            channel_slug: primary.reply_slug(),
            company_slug: primary.company.slug.clone(),
            trigger_message_id: trigger_message_id(envelope),
            thread_references: references.clone(),
            recipient_to: sender_address(envelope),
            recipients_cc: recipients_cc
                .iter()
                .cloned()
                .map(EmailAddress::from)
                .collect(),
            subject: envelope.content.subject().to_string(),
            body_text: agent_response_email_body(response),
            hop_count: envelope.directives.hop_count,
            trace_channels: envelope.directives.trace_channels.clone(),
            correlation_id,
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
            .prepare_agent_reply(outbound_email, mode, primary.company.id)
            .await?;

        Ok((
            OutboundDelivery {
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
        envelope: &InboundEnvelope,
    ) -> AppResult<Vec<String>> {
        let mut recipients_cc = self.inbound_cc_for(primary, envelope).await?;
        let Some(participants) = primary.channel.participant_emails.as_ref() else {
            return Ok(recipients_cc);
        };
        for participant in participants {
            if participant.eq_ignore_ascii_case(PUBLIC_PARTICIPANT)
                || participant.eq_ignore_ascii_case(envelope.author.subject().as_str())
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
        envelope: &InboundEnvelope,
    ) -> AppResult<Vec<String>> {
        if primary.channel.add_3rd_party {
            return Ok(inbound_cc_addresses(envelope));
        }
        let mut directory = DirectoryCache::new(self);
        let sender = envelope.author.subject().as_str().to_string();
        let mut kept = Vec::new();
        for address in inbound_cc_addresses(envelope) {
            if self
                .is_third_party_address(&address, &sender, &mut directory)
                .await?
            {
                continue;
            }
            kept.push(address);
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
            // A redelivery is accepted and returns the first delivery's ids, so there is no
            // duplicate case to special-case here any more: the commit recognises it.
            let ingest = self.ingest_prepared_internal_message(&prepared).await?;
            if !ingest.accepted {
                return Err(AppError::Internal(
                    ingest
                        .reason()
                        .unwrap_or("Internal channel delivery was rejected")
                        .to_string(),
                ));
            }
            info!(
                "Delivered agent response {} through trusted internal channel transport",
                prepared.outbound_message_id
            );
            return Ok(prepared);
        }
        match idempotency_key {
            Some(key) => {
                self.mail_dispatcher
                    .send_idempotent(outbound_email, key)
                    .await
            }
            None => self.mail_dispatcher.send(outbound_email).await,
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
        mode: ReplyDeliveryMode,
        company_id: Uuid,
    ) -> AppResult<(SentEmailResult, Option<OutboundSend>)> {
        let ReplyDeliveryMode::Durable { task_id } = mode else {
            let sent = self.send_or_route_internally(outbound_email, None).await?;
            return Ok((sent, None));
        };
        let idempotency_key = format!("task:{task_id}:agent-reply");
        let prepared = self
            .mail_dispatcher
            .prepare_idempotent(outbound_email.clone(), &idempotency_key)?;

        let pending = OutboundSend {
            company_id,
            channel_id: outbound_email.channel_id,
            task_id: Some(task_id),
            correlation_id: outbound_email.correlation_id,
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
        envelope: &InboundEnvelope,
    ) -> AppResult<OutboundDelivery> {
        info!(
            channel = %primary.channel.slug,
            "Simulation test mode (Run_Test): skipped SMTP dispatch for the agent reply"
        );
        Ok(OutboundDelivery {
            subject: reply_subject(envelope.content.subject()),
            email_sent: false,
        })
    }

    /// The agent's answer, as one canonical message associated with every thread it answered.
    ///
    /// One message, not one per channel. A dispatch that answered three channels' threads used to
    /// write three canonical rows carrying the same body and the same RFC `Message-ID`, which made
    /// "the reply" three different things to anything reading it back and forced the outbound
    /// header into the message model to keep them recognisable as one.
    ///
    /// So the header is gone from here: the reply is authored by the *agent's* principal, over no
    /// transport, and everything about how it goes out -- the sender address, the To and Cc lines,
    /// the RFC ids -- belongs to the delivery intent that carries it. A reply that also goes to
    /// Slack is then the same message with a second intent, rather than a second body.
    ///
    /// Built rather than written: this goes into the dispatch commit alongside the outbox row and
    /// the task payload, because a reply visible in a thread but never queued for delivery -- or
    /// queued but invisible -- is worse than neither.
    fn agent_reply_write(
        matches: &[ChannelMatch],
        agent: Option<&Agent>,
        delivery: &OutboundDelivery,
        response: &str,
        correlation_id: CorrelationId,
    ) -> AppResult<AgentReply> {
        let primary = matches.first().ok_or_else(|| {
            AppError::Internal("An agent dispatch answered no channel at all".into())
        })?;
        // An agent ran, so there is an agent to attribute the answer to. Falling back to the
        // platform's system principal rather than to the channel's mailbox keeps the claim honest
        // when a run somehow produced an answer without one.
        let author = match agent {
            Some(agent) => MessageAuthorWrite::Agent(AgentAuthor {
                agent_id: agent.id,
                display_label: agent.name.clone(),
            }),
            None => MessageAuthorWrite::Platform,
        };

        let message = MessageWrite {
            id: CanonicalMessageId::random(),
            thread_id: primary.thread.id,
            author,
            subject: delivery.subject.clone(),
            clean_text_body: response.to_string(),
            attachments: Vec::new(),
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            correlation_id,
            // No sender/to/cc rows: who this went out to is a fact of the delivery, and the
            // delivery holds it. A reply fanned out to two transports has two recipient lists and
            // one body.
            participants: Vec::new(),
            correlation: MessageCorrelation::Internal,
            created_at: chrono::Utc::now(),
        };

        Ok(AgentReply {
            also_in_threads: matches[1..]
                .iter()
                .map(|channel_match| channel_match.thread.id)
                .collect(),
            message,
        })
    }

    /// Make this dispatch durable: the reply in every thread it answered, the outbox row that
    /// delivers it, and the audit payload on the task, as one lease-fenced transaction.
    ///
    /// A failed run never reaches here -- it is rejected before delivery -- so this only ever
    /// commits a run that produced a real reply.
    async fn commit_dispatch(&self, input: DispatchCommitInput<'_, '_>) -> AppResult<()> {
        let DispatchCommitInput {
            ingest,
            envelope,
            lease,
            run,
            commit,
            response,
            metadata,
        } = input;
        let PreparedDispatch {
            delivery,
            outbound,
            reply,
        } = commit;

        // A task-less caller (direct ingest) has no lease to fence on and no payload to write, so
        // its reply is stored on its own. It also has no outbox row: it sent inline.
        let Some(task_id) = ingest.task_id else {
            let stored = self
                .thread_persistence
                .create_message(&reply.message)
                .await?;
            for thread_id in &reply.also_in_threads {
                self.thread_persistence
                    .associate_message(*thread_id, stored.canonical_id)
                    .await?;
            }
            return Ok(());
        };

        let payload = self.dispatch_audit_payload(DispatchAudit {
            ingest,
            envelope,
            run,
            reply: &reply,
            delivery: &delivery,
            response,
            metadata,
        });

        match self
            .task_persistence
            .commit_agent_dispatch(AgentDispatchCommit {
                lease,
                reply: &reply,
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

    fn dispatch_audit_payload(&self, audit: DispatchAudit<'_, '_>) -> serde_json::Value {
        let DispatchAudit {
            ingest,
            envelope,
            run,
            reply,
            delivery,
            response,
            metadata,
        } = audit;
        let execution_parameters = match &run.primary_params {
            Some(params) => {
                let mut config = params.config().clone();
                scrub_json_secrets(Some(&mut config));
                serde_json::json!({
                    "provider": params.provider(),
                    "model": params.model(),
                    "agent_id": run.primary_agent.as_ref().map(|a| a.id),
                    "agent_name": run.primary_agent.as_ref().map(|a| a.name.as_str()),
                    "prompt": build_prompt_text(envelope),
                    "config": config,
                    "executed_at": chrono::Utc::now().to_rfc3339(),
                })
            }
            None => serde_json::json!({
                "prompt": build_prompt_text(envelope),
                "executed_at": chrono::Utc::now().to_rfc3339(),
            }),
        };

        let mut execution_result = serde_json::json!({
            "response": response,
            "email_sent": delivery.email_sent,
            // The canonical reply, named by the id it is about to be written under in this same
            // transaction. The mail this run also queued is named by its own delivery key, which
            // belongs to the outbox row rather than to the message.
            "reply_message_id": reply.message.id,
            "error": run.failure.as_ref().map(ToString::to_string),
            "token_usage": TokenUsage::new(run.prompt_tokens, run.completion_tokens),
        });
        if let (Some(meta), Some(object)) = (metadata, execution_result.as_object_mut()) {
            object.insert("metadata".to_string(), meta.clone());
        }

        let mut payload = ingest.durable_task_payload();
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
    if let Some(envelope) = ingest.envelope.as_deref()
        && !envelope.directives.disposition.answers()
    {
        return Some(envelope.source.message_key.as_str());
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
        step: PipelineStep::only(),
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

#[cfg(test)]
mod guardrail_trust_tests {
    use super::*;
    use crate::transport::test_support::envelope_from;

    fn access(trusted: bool) -> ParticipantAccess {
        ParticipantAccess {
            authorized: true,
            trusted,
        }
    }

    fn envelope(is_forwarded: bool) -> InboundEnvelope {
        let mut envelope = envelope_from("colleague@acme.test", "Quick question", "hello");
        envelope.directives.is_forwarded = is_forwarded;
        envelope
    }

    #[test]
    fn a_trusted_sender_skips_the_guardrail_on_their_own_words() {
        assert!(guardrail_may_be_skipped(&access(true), &envelope(false)));
    }

    /// The forwarded-injection case: a teammate's envelope around a stranger's words. The marker
    /// is set by the mail adapter, from the subject or the body; what matters here is that trust
    /// stops at it.
    #[test]
    fn a_forward_does_not_inherit_the_forwarders_trust() {
        assert!(!guardrail_may_be_skipped(&access(true), &envelope(true)));
    }

    #[test]
    fn an_untrusted_sender_never_skips_it() {
        assert!(!guardrail_may_be_skipped(&access(false), &envelope(false)));
        assert!(!guardrail_may_be_skipped(&access(false), &envelope(true)));
    }
}

#[cfg(test)]
mod agent_reply_tests {
    use super::*;
    use crate::entities::{
        channel::{Channel, ChannelAccessMode},
        company::Company,
        creation::CreationProvenance,
        thread::Thread,
    };

    fn company() -> Company {
        Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme".into(),
            slug: "acme".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        }
    }

    fn channel(company_id: Uuid, slug: &str) -> Channel {
        Channel {
            owner_agent_id: None,
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id,
            name: slug.to_string(),
            description: None,
            slug: slug.into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            agent_ids: Some(vec![Uuid::new_v4()]),
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        }
    }

    fn matched(company: &Company, slug: &str, index: usize, total: usize) -> ChannelMatch {
        let channel = channel(company.id, slug);
        let thread = Thread {
            id: Uuid::new_v4(),
            channel_id: channel.id,
            subject: "Hello".into(),
            participant_principal_ids: Vec::new(),
            participant_projection: Default::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        ChannelMatch {
            company: company.clone(),
            channel,
            matched_slug: None,
            inbound_message: super::super::test_support::stored_email(
                super::super::test_support::EmailMessageDraft {
                    thread_id: thread.id,
                    ..Default::default()
                },
            ),
            thread,
            recipient_role: RecipientRole::To,
            step: PipelineStep { index, total },
        }
    }

    fn agent(id: Uuid) -> Agent {
        Agent {
            memory_enabled: false,
            id,
            company_id: Some(Uuid::new_v4()),
            name: "Triage Agent".into(),
            slug: "triage".into(),
            provider: None,
            model: None,
            run_timeout_secs: None,
            system_prompt: None,
            description: None,
            config_json: None,
            avatar_url: None,
            memory_persistence_mode: Default::default(),
            memory_recall_mode: Default::default(),
            memory_max_results: crate::entities::memory::default_memory_max_results(),
            created_by: CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        }
    }

    fn delivery() -> OutboundDelivery {
        OutboundDelivery {
            subject: "Re: Hello".into(),
            email_sent: true,
        }
    }

    /// The heart of it: three channels' threads answered by one canonical message, associated
    /// three times. Three writes carrying the same body -- and, before this, the same RFC
    /// `Message-ID` -- made "the reply" three different rows that anything reading it back had to
    /// keep in step by hand.
    #[test]
    fn one_dispatch_produces_one_canonical_reply_and_one_association_per_answered_thread() {
        let company = company();
        let matches = vec![
            matched(&company, "support", 0, 3),
            matched(&company, "billing", 1, 3),
            matched(&company, "legal", 2, 3),
        ];
        let responder = agent(Uuid::new_v4());

        let reply = ThreadUseCases::agent_reply_write(
            &matches,
            Some(&responder),
            &delivery(),
            "Here is the answer.",
            CorrelationId::new(),
        )
        .unwrap();

        assert_eq!(reply.message.thread_id, matches[0].thread.id);
        assert_eq!(
            reply.also_in_threads,
            vec![matches[1].thread.id, matches[2].thread.id]
        );
        assert_eq!(reply.message.clean_text_body, "Here is the answer.");
        assert_eq!(reply.message.subject, "Re: Hello");
    }

    /// Attribution is the agent's own principal. It used to be the channel's mailbox, observed as
    /// an external identity -- which made every agent in a channel one actor, and made the actor a
    /// mailbox.
    #[test]
    fn the_reply_is_attributed_to_the_agent_that_wrote_it() {
        let company = company();
        let matches = vec![matched(&company, "support", 0, 1)];
        let agent_id = Uuid::new_v4();

        let reply = ThreadUseCases::agent_reply_write(
            &matches,
            Some(&agent(agent_id)),
            &delivery(),
            "Answer",
            CorrelationId::new(),
        )
        .unwrap();

        match &reply.message.author {
            MessageAuthorWrite::Agent(author) => {
                assert_eq!(author.agent_id, agent_id);
                assert_eq!(author.display_label, "Triage Agent");
            }
            other => panic!("expected an agent author, got {other:?}"),
        }
    }

    /// A run that somehow produced an answer with no agent behind it is attributed to the
    /// platform, not to a mailbox invented for the occasion.
    #[test]
    fn an_answer_with_no_agent_behind_it_is_attributed_to_the_platform() {
        let company = company();
        let matches = vec![matched(&company, "support", 0, 1)];

        let reply = ThreadUseCases::agent_reply_write(
            &matches,
            None,
            &delivery(),
            "Answer",
            CorrelationId::new(),
        )
        .unwrap();

        assert!(matches!(reply.message.author, MessageAuthorWrite::Platform));
    }

    /// Nothing about how the answer went out is on the message: no headers, no sender, no `To`.
    /// Those are facts of the delivery, and the delivery intent holds them -- which is what lets
    /// one reply be delivered over two transports without becoming two bodies.
    #[test]
    fn the_reply_carries_no_transport_facts_at_all() {
        let company = company();
        let matches = vec![matched(&company, "support", 0, 1)];

        let reply = ThreadUseCases::agent_reply_write(
            &matches,
            Some(&agent(Uuid::new_v4())),
            &delivery(),
            "Answer",
            CorrelationId::new(),
        )
        .unwrap();

        assert!(matches!(
            reply.message.correlation,
            MessageCorrelation::Internal
        ));
        assert_eq!(reply.message.email_metadata(), None);
        assert!(reply.message.participants.is_empty());
    }

    /// A dispatch that answered nothing has no thread to file a reply in, and says so rather than
    /// writing one somewhere arbitrary.
    #[test]
    fn a_dispatch_that_answered_no_channel_is_an_error() {
        assert!(
            ThreadUseCases::agent_reply_write(
                &[],
                None,
                &delivery(),
                "Answer",
                CorrelationId::new(),
            )
            .is_err()
        );
    }
}
