//! Everything around one agent turn that is ours whichever runtime executes it.
//!
//! The runner composes the prompt, assembles the ports a harness may reach back through --
//! approvals, our native tools, tracing -- resolves the harness the agent's capability spec asks
//! for, and records what the run cost. It does not know what a `tools:` list
//! looks like, how a skill becomes steps, or which crate answers `chat`. That is the harness's,
//! behind [`AgentHarness`].
//!
//! [`AgentHarness`]: crate::services::harness::AgentHarness

mod params;
mod prompt;

pub use params::{
    DEFAULT_AGENT_NAME, DEFAULT_SYSTEM_PROMPT, ResolvedAgentCapabilities,
    resolve_agent_capabilities,
};

use std::sync::Arc;

use tracing::info;
use uuid::Uuid;

use crate::app_error::{AppError, AppResult};
use crate::domain::monitoring::{AiExecutionMetrics, MonitoringService};
use crate::entities::approval::ApprovalSubject;
use crate::entities::correlation::CorrelationId;
use crate::entities::internal_note::AgentInstructionNote;
use crate::entities::message_view::AgentHistoryMessage;
use crate::entities::task::TokenUsage;
use crate::services::agent_trace_hooks::{AgentTraceContext, AgentTraceHooks};
use crate::services::harness::{
    AgentExecutionOutput, AgentHarness, AgentRun, HarnessRegistry, HarnessTrace, sanitize_text,
};
use crate::services::prompt_fence::UntrustedFence;
use crate::use_cases::thread::RecipientRole;

use prompt::PromptParts;

pub struct AgentRunner<'a> {
    run_store: Option<Arc<dyn crate::services::harness::runs::HarnessRunStore>>,
    deadline: Option<tokio::time::Instant>,
    prompt: &'a str,
    /// Subject of the message `prompt` came from, if it has one.
    subject: Option<&'a str>,
    history: &'a [AgentHistoryMessage],
    internal_notes: &'a [AgentInstructionNote],
    params: &'a ResolvedAgentCapabilities,
    approval_context: Option<ApprovalSubject>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    company_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    recipient_role: Option<RecipientRole>,
    upstream_pipeline_context: Option<String>,
    /// Set when the run belongs to a durable chain, which is every run driven by a task. Absent
    /// only for a direct, task-less ingest, whose actions have nothing to be correlated with.
    trace: Option<AgentTraceContext>,
    /// The harnesses this deployment can run. Absent means no agent can execute at all, which is
    /// a wiring fault and is reported as one rather than quietly falling back to a default.
    harnesses: Option<Arc<HarnessRegistry>>,
}

impl<'a> AgentRunner<'a> {
    pub fn new(prompt: &'a str, params: &'a ResolvedAgentCapabilities) -> Self {
        Self {
            run_store: None,
            deadline: None,
            prompt,
            subject: None,
            history: &[],
            internal_notes: &[],
            params,
            approval_context: None,
            monitoring: None,
            company_id: None,
            channel_id: None,
            agent_id: None,
            recipient_role: None,
            upstream_pipeline_context: None,
            trace: None,
            harnesses: None,
        }
    }

    pub fn history(mut self, history: &'a [AgentHistoryMessage]) -> Self {
        self.history = history;
        self
    }

    pub fn internal_notes(mut self, notes: &'a [AgentInstructionNote]) -> Self {
        self.internal_notes = notes;
        self
    }

    pub fn subject(mut self, subject: Option<&'a str>) -> Self {
        self.subject = subject;
        self
    }

    pub fn params(mut self, params: &'a ResolvedAgentCapabilities) -> Self {
        self.params = params;
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

    /// Which runtimes this deployment can execute an agent on -- a deployment fact rather than a
    /// per-run choice.
    pub fn harnesses(mut self, harnesses: Arc<HarnessRegistry>) -> Self {
        self.harnesses = Some(harnesses);
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

    pub fn run_store(
        mut self,
        store: Option<Arc<dyn crate::services::harness::runs::HarnessRunStore>>,
    ) -> Self {
        self.run_store = store;
        self
    }

    pub fn deadline(mut self, deadline: tokio::time::Instant) -> Self {
        self.deadline = Some(deadline);
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
            internal_notes: self.internal_notes,
            upstream: self.upstream_pipeline_context.as_deref(),
            recipient_role: self.recipient_role,
        }
        .compose(&fence);
        let key = self.params.api_key();

        let harness = self.resolve_harness()?;
        let full_prompt = sanitize_text(&raw_full_prompt, Some(key));
        info!("Full prompt context length: {}", full_prompt.len());

        let deadline = self
            .deadline
            .unwrap_or_else(|| tokio::time::Instant::now() + std::time::Duration::from_secs(300));
        let execution = self
            .run_store
            .clone()
            .zip(self.approval_context.as_ref())
            .and_then(|(store, context)| {
                context
                    .suspension
                    .and_then(|suspension| suspension.lease())
                    .map(|lease| crate::services::harness::runs::RunExecution {
                        company_id: context.company_id,
                        lease,
                        store,
                    })
            });
        let run = AgentRun {
            company_id: self.company_id,
            execution,
            deadline,
            spec: Box::new(self.params.spec().clone()),
            agent_id: self.params.agent_id,
            api_key: key,
            full_prompt: &full_prompt,
            history_message_count,
            recipient_role: self.recipient_role,
            trace: self.tracer(),
        };

        // Keep the harness future inside the lease/timeout supervisor. Dropping this future drops
        // the provider call itself instead of detaching a still-running Tokio task. Boxing keeps
        // that property while leaving only a pointer in this frame -- and it is the one seam
        // `src/AGENTS.md` asks for at a descent into an external runtime.
        let task_result = tokio::time::timeout_at(deadline, Box::pin(harness.run(run)))
            .await
            .unwrap_or_else(|_| {
                Err(AppError::Timeout(
                    "Agent execution deadline exceeded".into(),
                ))
            });
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
        AppError::Execution(kind) => AppError::Execution(kind),
        AppError::Database(message) => AppError::Database(clean(message)),
        AppError::DatabaseTimeout(message) => AppError::DatabaseTimeout(clean(message)),
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
        AppError::Execution(_) => "execution_terminal",
        AppError::Database(_) => "database",
        AppError::DatabaseTimeout(_) => "database_timeout",
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
