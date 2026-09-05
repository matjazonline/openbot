//! Everything around one agent turn that is ours whichever runtime executes it.
//!
//! The runner composes the prompt, runs the spam guardrail, assembles the ports a harness may
//! reach back through -- approvals, our native tools, tracing -- resolves the harness the agent's
//! capability spec asks for, and records what the run cost. It does not know what a `tools:` list
//! looks like, how a skill becomes steps, or which crate answers `chat`. That is the harness's,
//! behind [`AgentHarness`].
//!
//! [`AgentHarness`]: crate::services::harness::AgentHarness

mod params;
mod prompt;

pub use params::{
    DEFAULT_AGENT_NAME, DEFAULT_SYSTEM_PROMPT, ResolvedAgentParams, resolve_agent_params,
};

use std::sync::Arc;

use tracing::info;
use uuid::Uuid;

use crate::app_error::{AppError, AppResult};
use crate::domain::monitoring::{AiExecutionMetrics, MonitoringService};
use crate::entities::approval::ApprovalSubject;
use crate::entities::company::Company;
use crate::entities::correlation::CorrelationId;
use crate::entities::message_view::AgentHistoryMessage;
use crate::entities::task::TokenUsage;
use crate::entities::tool_catalogue::{AGENT_DIRECTORY_TOOL_ID, OUTREACH_TOOL_ID};
use crate::infra::config::AppConfig;
use crate::services::agent_channel_tool::{
    AgentChannelProvisioning, AgentChannelToolContext, CreateAgentChannelTool,
};
use crate::services::agent_directory_tool::{AgentDirectoryContext, ListCompanyAgentsTool};
use crate::services::agent_trace_hooks::{AgentTraceContext, AgentTraceHooks};
use crate::services::harness::{
    AgentApprovalHandler, AgentExecutionOutput, AgentHarness, AgentRun, HarnessApprovals,
    HarnessRegistry, HarnessToolHost, HarnessTrace, InternalDelegationPolicy, TextClassifier,
    internal_requires_approval, sanitize_text,
};
use crate::services::llm_guardrail::GuardrailCheck;
use crate::services::native_tools::NativeToolHost;
use crate::services::outreach_tool::{OutreachAndAwaitQuorumTool, OutreachToolContext};
use crate::services::prompt_fence::UntrustedFence;
use crate::task_queue::TaskPersistence;
use crate::transport::DeliveryComposer;
use crate::use_cases::approval::ApprovalUseCases;
use crate::use_cases::{
    agent::AgentPersistence, channel::ChannelPersistence, integration::ChannelBindingPersistence,
    thread::RecipientRole,
};

use prompt::PromptParts;

pub struct AgentRunner<'a> {
    prompt: &'a str,
    /// Subject of the message `prompt` came from, if it has one.
    subject: Option<&'a str>,
    history: &'a [AgentHistoryMessage],
    params: &'a ResolvedAgentParams,
    approval_use_cases: Option<Arc<ApprovalUseCases>>,
    approval_context: Option<ApprovalSubject>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    app_config: Option<Arc<AppConfig>>,
    company: Option<Company>,
    company_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    skip_spam_guardrail: bool,
    recipient_role: Option<RecipientRole>,
    upstream_pipeline_context: Option<String>,
    task_persistence: Option<Arc<dyn TaskPersistence>>,
    channel_persistence: Option<Arc<dyn ChannelPersistence>>,
    agent_persistence: Option<Arc<dyn AgentPersistence>>,
    binding_persistence: Option<Arc<dyn ChannelBindingPersistence>>,
    /// Freezes what an outreach question will be mailed as. Set with the outreach tool, because
    /// the two are the same capability: an agent that may ask a third party a question is an agent
    /// that may queue mail.
    delivery_composer: Option<DeliveryComposer>,
    outreach_context: Option<OutreachToolContext>,
    agent_channel_tool: Option<(Arc<dyn AgentChannelProvisioning>, AgentChannelToolContext)>,
    /// Set when the run belongs to a durable chain, which is every run driven by a task. Absent
    /// only for a direct, task-less ingest, whose actions have nothing to be correlated with.
    trace: Option<AgentTraceContext>,
    /// The harnesses this deployment can run. Absent means no agent can execute at all, which is
    /// a wiring fault and is reported as one rather than quietly falling back to a default.
    harnesses: Option<Arc<HarnessRegistry>>,
    classifier: Option<Arc<dyn TextClassifier>>,
}

impl<'a> AgentRunner<'a> {
    pub fn new(prompt: &'a str, params: &'a ResolvedAgentParams) -> Self {
        Self {
            prompt,
            subject: None,
            history: &[],
            params,
            approval_use_cases: None,
            approval_context: None,
            monitoring: None,
            app_config: None,
            company: None,
            company_id: None,
            channel_id: None,
            agent_id: None,
            skip_spam_guardrail: false,
            recipient_role: None,
            upstream_pipeline_context: None,
            task_persistence: None,
            channel_persistence: None,
            delivery_composer: None,
            agent_persistence: None,
            binding_persistence: None,
            outreach_context: None,
            agent_channel_tool: None,
            trace: None,
            harnesses: None,
            classifier: None,
        }
    }

    pub fn config(mut self, config: Option<Arc<AppConfig>>) -> Self {
        self.app_config = config;
        self
    }

    pub fn company(mut self, company: Option<Company>) -> Self {
        self.company = company;
        self
    }

    pub fn history(mut self, history: &'a [AgentHistoryMessage]) -> Self {
        self.history = history;
        self
    }

    pub fn subject(mut self, subject: Option<&'a str>) -> Self {
        self.subject = subject;
        self
    }

    pub fn params(mut self, params: &'a ResolvedAgentParams) -> Self {
        self.params = params;
        self
    }

    pub fn approval_use_cases(mut self, use_cases: Option<Arc<ApprovalUseCases>>) -> Self {
        self.approval_use_cases = use_cases;
        self
    }

    pub fn approval_context(mut self, ctx: Option<ApprovalSubject>) -> Self {
        self.approval_context = ctx;
        self
    }

    pub fn monitoring(mut self, monitoring: Option<Arc<dyn MonitoringService>>) -> Self {
        self.monitoring = monitoring;
        self
    }

    /// Which runtimes this deployment can execute an agent on, and what answers the spam
    /// guardrail's one-shot classification.
    ///
    /// Both arrive together because both are deployment facts rather than per-run choices, and a
    /// run that has one and not the other is a half-wired deployment worth failing on.
    pub fn harnesses(
        mut self,
        harnesses: Arc<HarnessRegistry>,
        classifier: Arc<dyn TextClassifier>,
    ) -> Self {
        self.harnesses = Some(harnesses);
        self.classifier = Some(classifier);
        self
    }

    pub fn ids(
        mut self,
        company_id: Option<Uuid>,
        channel_id: Option<Uuid>,
        agent_id: Option<Uuid>,
    ) -> Self {
        self.company_id = company_id;
        self.channel_id = channel_id;
        self.agent_id = agent_id;
        self
    }

    pub fn skip_spam_guardrail(mut self, skip: bool) -> Self {
        self.skip_spam_guardrail = skip;
        self
    }

    pub fn recipient_role(mut self, role: Option<RecipientRole>) -> Self {
        self.recipient_role = role;
        self
    }

    pub fn upstream_pipeline_context(mut self, ctx: Option<String>) -> Self {
        self.upstream_pipeline_context = ctx;
        self
    }

    /// Attribute every action this run takes -- each tool call, each handoff, each approval it
    /// asks for -- to the chain that caused the run.
    ///
    /// Takes the ids rather than reading them off the approval or outreach context, because a run
    /// that has neither of those tools still takes actions worth tracing.
    pub fn trace(mut self, correlation_id: CorrelationId, task_id: Option<Uuid>) -> Self {
        self.trace = Some(AgentTraceContext {
            correlation_id,
            task_id,
            company_id: self.company_id,
            channel_id: self.channel_id,
            agent_id: self.agent_id,
        });
        self
    }

    pub fn outreach_tool(
        mut self,
        persistence: Arc<dyn TaskPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        deliveries: DeliveryComposer,
        context: OutreachToolContext,
    ) -> Self {
        self.task_persistence = Some(persistence);
        self.channel_persistence = Some(channel_persistence);
        self.delivery_composer = Some(deliveries);
        self.outreach_context = Some(context);
        self
    }

    /// Let the agent discover which sibling channels it may call.
    ///
    /// Separate from [`AgentRunner::outreach_tool`] because the directory is a read, not a send:
    /// an agent can be given the address book without being given the ability to write to it.
    ///
    /// The bindings come along because a directory entry says which interfaces a channel is
    /// reachable on -- display data an agent can mention, never the routing key it delegates by.
    pub fn agent_directory(
        mut self,
        agent_persistence: Arc<dyn AgentPersistence>,
        binding_persistence: Arc<dyn ChannelBindingPersistence>,
    ) -> Self {
        self.agent_persistence = Some(agent_persistence);
        self.binding_persistence = Some(binding_persistence);
        self
    }

    pub fn agent_channel_tool(
        mut self,
        persistence: Arc<dyn AgentChannelProvisioning>,
        context: AgentChannelToolContext,
    ) -> Self {
        self.agent_channel_tool = Some((persistence, context));
        self
    }

    pub async fn execute(self) -> AppResult<AgentExecutionOutput> {
        let start_time = std::time::Instant::now();
        let history_message_count = self.history.len();
        info!(
            "Executing AI Agent with prompt length {} and history count {}",
            self.prompt.len(),
            self.history.len()
        );

        let fence = UntrustedFence::new();
        let raw_full_prompt = PromptParts {
            message: self.prompt,
            subject: self.subject,
            history: self.history,
            upstream: self.upstream_pipeline_context.as_deref(),
            recipient_role: self.recipient_role,
        }
        .compose(&fence);
        let key = self.params.api_key();

        // Stage 3: Optional LLM Spam & Guardrail Evaluation (skipped for trusted participants)
        if !self.skip_spam_guardrail
            && let Some(ref cfg) = self.app_config
        {
            GuardrailCheck {
                config: cfg,
                company: self.company.as_ref(),
                monitoring: self.monitoring.as_ref(),
                classifier: self.classifier.as_ref(),
                prompt_text: &raw_full_prompt,
                provider: self.params.provider(),
                model: self.params.model(),
                api_key: key,
            }
            .evaluate()
            .await?;
        }

        let harness = self.resolve_harness()?;
        let full_prompt = sanitize_text(&raw_full_prompt, Some(key));
        info!("Full prompt context length: {}", full_prompt.len());

        let run = AgentRun {
            spec: Box::new(self.params.spec().clone()),
            api_key: key,
            full_prompt: &full_prompt,
            history_message_count,
            recipient_role: self.recipient_role,
            approvals: self.approvals(),
            tool_host: self.tool_host(),
            trace: self.tracer(),
        };

        // Keep the harness future inside the lease/timeout supervisor. Dropping this future drops
        // the provider call itself instead of detaching a still-running Tokio task. Boxing keeps
        // that property while leaving only a pointer in this frame -- and it is the one seam
        // `src/AGENTS.md` asks for at a descent into an external runtime.
        let task_result = Box::pin(harness.run(run)).await;
        let duration_ms = start_time.elapsed().as_millis() as u64;

        match task_result {
            Ok(mut output) => {
                // Wall-clock time is only known here, after the harness has been awaited.
                output.stamp_duration(duration_ms);
                self.record_execution(duration_ms, Some(&output.token_usage), None);
                Ok(output)
            }
            Err(err) => {
                let err = sanitize_error(err, key);
                let error_type = error_type(&err);
                tracing::warn!(error_type, "AI Agent execution failed");
                self.record_execution(duration_ms, None, Some(error_type.to_string()));
                Err(err)
            }
        }
    }

    /// The harness this agent's spec asks for.
    ///
    /// A missing registry and an unregistered kind are both hard errors. Falling back to whatever
    /// else is registered would run the agent on a runtime with different tools and a different
    /// sandbox, and nothing in the reply would say so.
    fn resolve_harness(&self) -> AppResult<Arc<dyn AgentHarness>> {
        let kind = self.params.spec().harness;
        let registry = self.harnesses.as_ref().ok_or_else(|| {
            AppError::BadRequest(format!(
                "No agent harness is configured for this deployment ({kind})"
            ))
        })?;
        registry
            .require(kind)
            .map(Arc::clone)
            .map_err(|error| AppError::BadRequest(error.to_string()))
    }

    /// Who decides the actions this run must not take on its own.
    ///
    /// `None` when the run has no approval subject: nothing about it can be approved, which is
    /// also the reason nothing about it may be auto-approved.
    fn approvals(&self) -> Option<Arc<dyn HarnessApprovals>> {
        let (use_cases, context) = self
            .approval_use_cases
            .clone()
            .zip(self.approval_context.clone())?;
        // Only an outreach-capable run can delegate, so only one can be exempted from asking.
        let delegation = self
            .outreach_context
            .as_ref()
            .zip(self.channel_persistence.as_ref())
            .map(|(context, channels)| InternalDelegationPolicy {
                channel_persistence: channels.clone(),
                company_id: context.company_id,
                source_channel_id: context.channel_id,
                requires_approval: internal_requires_approval(self.params.config()),
            });
        Some(Arc::new(AgentApprovalHandler {
            approval_use_cases: use_cases,
            context,
            delegation,
        }))
    }

    /// Our own tools, assembled from the contexts this run was actually given.
    ///
    /// Registration is not a grant: what this returns is what the run *could* serve, and the
    /// harness intersects it with the capability spec's grant list.
    fn tool_host(&self) -> Option<Arc<dyn HarnessToolHost>> {
        let mut host = NativeToolHost::new();

        // All four or none: `AgentRunner::outreach_tool` sets them together, and a partial set
        // would be a caller that built half a capability.
        if let (
            Some(task_persistence),
            Some(channel_persistence),
            Some(deliveries),
            Some(context),
        ) = (
            self.task_persistence.clone(),
            self.channel_persistence.clone(),
            self.delivery_composer.clone(),
            self.outreach_context.clone(),
        ) {
            if let Some((agent_persistence, binding_persistence)) = self
                .agent_persistence
                .clone()
                .zip(self.binding_persistence.clone())
            {
                let mut directory = ListCompanyAgentsTool::new(
                    channel_persistence.clone(),
                    agent_persistence,
                    binding_persistence,
                    AgentDirectoryContext {
                        company_id: context.company_id,
                        source_channel_id: context.channel_id,
                    },
                );
                if let Some(policy) = self.tool_policy(AGENT_DIRECTORY_TOOL_ID) {
                    directory = directory.with_policy_config(policy);
                }
                host = host.with_directory(directory);
            }

            let mut outreach = OutreachAndAwaitQuorumTool::new(
                task_persistence,
                channel_persistence,
                deliveries,
                context,
            );
            if let Some(policy) = self.tool_policy(OUTREACH_TOOL_ID) {
                outreach = outreach.with_policy_config(policy);
            }
            host = host.with_outreach(outreach);
        }

        if let Some((persistence, context)) = self.agent_channel_tool.clone() {
            host = host.with_agent_channels(CreateAgentChannelTool::new(persistence, context));
        }

        (!host.is_empty()).then(|| Arc::new(host) as Arc<dyn HarnessToolHost>)
    }

    /// One tool's slice of the agent's own tool policy.
    ///
    /// Read from the agent's configuration rather than from a compiled harness config on purpose:
    /// every key this can carry has a platform default the tool applies for itself, and those
    /// defaults are the same values a harness would compile in. Reading the agent's overrides
    /// alone therefore gives the identical answer, and does it without the application layer
    /// having to know a runtime's config schema. Phase 4 replaces this with typed policy.
    fn tool_policy(&self, tool_id: &str) -> Option<serde_json::Value> {
        self.params
            .config()
            .get("tool_security")?
            .get("tools")?
            .get(tool_id)?
            .get("config")
            .cloned()
    }

    /// One hook object sees every tool the run reaches for, including the runtime's built-ins and
    /// anything behind MCP -- which is why this is a hook rather than logging inside each of our
    /// own three tools.
    fn tracer(&self) -> Option<Arc<dyn HarnessTrace>> {
        let context = self.trace.clone()?;
        Some(Arc::new(AgentTraceHooks::new(
            context,
            self.monitoring.clone(),
        )))
    }

    fn record_execution(
        &self,
        duration_ms: u64,
        token_usage: Option<&TokenUsage>,
        error_type: Option<String>,
    ) {
        let Some(ref monitoring) = self.monitoring else {
            return;
        };
        monitoring.record_ai_execution(&AiExecutionMetrics {
            company_id: self.company_id,
            channel_id: self.channel_id,
            agent_id: self.agent_id,
            provider: self.params.provider().to_string(),
            model: self.params.model().to_string(),
            prompt_tokens: token_usage.map_or(0, |t| t.prompt_tokens),
            completion_tokens: token_usage.map_or(0, |t| t.completion_tokens),
            total_tokens: token_usage.map_or(0, |t| t.total_tokens),
            duration_ms,
            success: error_type.is_none(),
            error_type,
        });
    }
}

/// Remove credentials without erasing the failure category the task worker uses to decide
/// whether another attempt can help.
fn sanitize_error(error: AppError, api_key: &str) -> AppError {
    let clean = |message: String| sanitize_text(&message, Some(api_key));
    match error {
        AppError::Database(message) => AppError::Database(clean(message)),
        AppError::InvalidCredentials => AppError::InvalidCredentials,
        AppError::BadRequest(message) => AppError::BadRequest(clean(message)),
        AppError::NotFound(message) => AppError::NotFound(clean(message)),
        AppError::Conflict(message) => AppError::Conflict(clean(message)),
        AppError::Timeout(message) => AppError::Timeout(clean(message)),
        AppError::Internal(message) => AppError::Internal(clean(message)),
    }
}

fn error_type(error: &AppError) -> &'static str {
    match error {
        AppError::Database(_) => "database",
        AppError::InvalidCredentials => "invalid_credentials",
        AppError::BadRequest(_) => "bad_request",
        AppError::NotFound(_) => "not_found",
        AppError::Conflict(_) => "conflict",
        AppError::Timeout(_) => "timeout",
        AppError::Internal(_) => "internal",
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
