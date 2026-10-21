//! The lower-level completion API provides the transaction boundary the agent runner cannot:
//! capture and commit the *whole* assistant turn before the first tool is invoked.
use super::{
    compile::{CompileContext, CompiledRun},
    providers::{ProviderRegistry, ResolvedModelRequest},
    tools::ToolCorrelationId,
};
use crate::adapters::response_schema::JsonResponseValidator;
use crate::services::response_contract::{ResponseContractValidator, StructuredResponse};
use crate::{
    app_error::{AppError, AppResult},
    entities::{harness::HarnessKind, transport::RecipientRole},
    services::harness::{
        AgentExecutionDisposition, AgentExecutionOutput, AgentHarness, AgentRun,
        context::{RuntimeContext, SystemClock},
        runs::*,
        sanitize_text,
    },
    use_cases::skill::AgentCapabilityReader,
};
use async_trait::async_trait;
use rig::{agent::ModelHandle, completion::CompletionRequest};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub struct RigHarness {
    providers: ProviderRegistry,
    pub(super) capabilities: Arc<dyn AgentCapabilityReader>,
}
impl RigHarness {
    pub fn new(capabilities: Arc<dyn AgentCapabilityReader>) -> AppResult<Self> {
        Self::with_providers(
            ProviderRegistry::standard()
                .map_err(|_| AppError::Internal("Rig provider transport unavailable".into()))?,
            capabilities,
        )
    }

    pub fn with_providers(
        providers: ProviderRegistry,
        capabilities: Arc<dyn AgentCapabilityReader>,
    ) -> AppResult<Self> {
        providers
            .validate_coverage()
            .map_err(|_| AppError::Internal("Rig provider coverage incomplete".into()))?;
        validate_deployment()?;
        Ok(Self {
            providers,
            capabilities,
        })
    }
}

#[async_trait]
impl AgentHarness for RigHarness {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Rig
    }
    async fn run(&self, mut run: AgentRun<'_>) -> AppResult<AgentExecutionOutput> {
        run.deadline = run
            .deadline
            .min(tokio::time::Instant::now() + std::time::Duration::from_millis(MAX_CLAIM_MS));
        let deadline = run.deadline;
        let trace = run.trace.clone();
        // Cover database preparation, discovery and the actual loop with one cancellation boundary.
        let result = tokio::time::timeout_at(deadline, Box::pin(self.execute_claim(run)))
            .await
            .unwrap_or_else(|_| Err(AppError::Timeout("Rig deadline exceeded".into())));
        if result.is_err()
            && let Some(trace) = trace
        {
            super::trace::optional(trace.run_failed()).await;
        }
        result
    }
}

impl RigHarness {
    fn compile(&self, run: &AgentRun<'_>, scope: &RunExecution) -> AppResult<CompiledRun> {
        let mut compiled = CompiledRun::compile(
            &run.spec,
            CompileContext {
                facts: RuntimeContext {
                    company_id: scope.company_id,
                    agent_id: run.agent_id,
                    recipient_role: run.recipient_role.unwrap_or(RecipientRole::To),
                    timezone: chrono_tz::UTC,
                },
                clock: Arc::new(SystemClock),
                capabilities: self.capabilities.clone(),
                token_budget: 1_048_576,
            },
        )?;
        let tools = Arc::get_mut(&mut compiled.tools)
            .ok_or_else(|| invalid("Rig tool state was shared before execution"))?;
        tools.restrict_deadline(run.deadline);
        tools.set_trace(run.trace.clone());
        Ok(compiled)
    }

    pub(super) fn model(&self, run: &AgentRun<'_>) -> AppResult<ModelHandle> {
        let secret = secrecy::SecretString::from(run.api_key.to_string());
        let model = self
            .providers
            .model(&ResolvedModelRequest {
                provider: &run.spec.provider,
                model: &run.spec.model,
                secret: &secret,
                endpoint: run.spec.provider_base_url.as_deref(),
            })
            .map_err(|_| invalid("Rig model configuration unavailable"))?;
        Ok(model)
    }

    async fn execute_claim(&self, mut run: AgentRun<'_>) -> AppResult<AgentExecutionOutput> {
        if run.execution.is_none() {
            return Box::pin(super::direct::execute(self, run)).await;
        }
        let scope = run
            .execution
            .as_ref()
            .ok_or_else(|| invalid("Rig execution requires a durable task"))?;
        if run.deadline <= tokio::time::Instant::now() {
            return Err(AppError::Timeout("Rig deadline exceeded".into()));
        }
        if let Some(contract) = &run.spec.response_contract {
            JsonResponseValidator.validate_contract(contract)?;
        }
        let policy = execution_policy(&run)?;
        let authorization = authorization(
            self.capabilities.as_ref(),
            scope.company_id,
            run.agent_id,
            &run.spec,
        )
        .await?;
        let identity = identity(&run, scope, &authorization)?;
        let checkpoint = applied(
            scope
                .store
                .open_run(OpenRun {
                    lease: scope.lease,
                    identity: &identity,
                    initial_prompt: run.full_prompt,
                    policy,
                })
                .await?,
        )?;
        if let Some(output) = checkpoint.final_output.clone() {
            return Ok(output);
        }
        let write = RunWrite {
            company_id: scope.company_id,
            run_id: checkpoint.run_id,
            lease: scope.lease,
            expected_revision: checkpoint.revision,
        };
        let allowance_ms = run
            .deadline
            .checked_duration_since(tokio::time::Instant::now())
            .ok_or_else(|| AppError::Timeout("Rig deadline exceeded".into()))?
            .as_millis()
            .max(1) as u64;
        let checkpoint = applied(scope.store.reserve_execution(&write, allowance_ms).await?)?;
        run.deadline = execution_deadline(&checkpoint, scope, run.deadline)?;
        let compiled = self.compile(&run, scope)?;
        let write = RunWrite {
            company_id: scope.company_id,
            run_id: checkpoint.run_id,
            lease: scope.lease,
            expected_revision: checkpoint.revision,
        };
        let checkpoint = applied(
            scope
                .store
                .bind_tool_catalogue(&write, compiled.tools.catalogue_fingerprint())
                .await?,
        )?;
        compiled.tools.restore_local_state(&checkpoint).await?;
        let model = self.model(&run)?;
        let inputs = super::diagnostics::InputCounts::from(&run);
        let execution = Execution {
            inputs,
            binding: Binding::Legacy(Box::new(LegacyBinding {
                scope,
                compiled,
                capabilities: self.capabilities.clone(),
                authorization,
                spec: run.spec,
            })),
            checkpoint,
            model,
            deadline: run.deadline,
            secret: run.api_key,
        };
        // Keep the external runtime state out of the worker/dispatch poll frame. No detached work.
        let driven = tokio::time::timeout_at(run.deadline, Box::pin(execution.drive_legacy()))
            .await
            .map_err(|_| AppError::Timeout("Rig deadline exceeded".into()))??;
        Ok(driven)
    }
}

pub(super) fn execution_policy(run: &AgentRun<'_>) -> AppResult<RigExecutionPolicy> {
    Ok(RigExecutionPolicy {
        model_calls: u16::from(
            run.spec
                .harness_config
                .rig()
                .ok_or_else(|| invalid("Rig configuration required"))?
                .effective_max_turns(crate::entities::harness::MAX_RIG_MAX_TURNS),
        ),
        ..Default::default()
    })
}

fn execution_deadline(
    checkpoint: &RunCheckpoint,
    scope: &RunExecution,
    deadline: tokio::time::Instant,
) -> AppResult<tokio::time::Instant> {
    let claim = checkpoint
        .executions
        .iter()
        .find(|claim| claim.generation == scope.lease.execution_generation)
        .ok_or_else(|| invalid("Execution allowance missing"))?;
    let remaining = (claim.started_at + chrono::Duration::milliseconds(claim.allowance_ms as i64)
        - chrono::Utc::now())
    .to_std()
    .map_err(|_| AppError::Timeout("Rig claim allowance exhausted".into()))?;
    Ok(deadline.min(tokio::time::Instant::now() + remaining))
}

fn identity(
    run: &AgentRun<'_>,
    scope: &RunExecution,
    authorization: &str,
) -> AppResult<RunIdentity> {
    let config = run
        .spec
        .harness_config
        .to_json()
        .map_err(AppError::BadRequest)?;
    let content = serde_json::json!({ "name":run.spec.name, "instructions":run.spec.system_prompt,"authorization":authorization,
        "provider":run.spec.provider, "model":run.spec.model,"endpoint":run.spec.provider_base_url, "skills":run.spec.skills,
        "sub_agents":match &run.spec.sub_agents { crate::entities::harness::SubAgentScope::AllCompanySiblings => serde_json::json!({"scope":"all_company_siblings"}), crate::entities::harness::SubAgentScope::Restricted(ids) => serde_json::json!({"scope":"restricted","ids":ids}) }, "config":config, "response_contract":run.spec.response_contract });
    Ok(RunIdentity {
        company_id: scope.company_id,
        task_id: scope.lease.task_id,
        agent_id: run.agent_id,
        harness: HarnessKind::Rig,
        provider: run.spec.provider.clone(),
        model: run.spec.model.clone(),
        response_contract: run.spec.response_contract.clone(),
        capability_fingerprint: format!("{:x}", Sha256::digest(content.to_string().as_bytes())),
    })
}
async fn authorization(
    reader: &dyn AgentCapabilityReader,
    company: uuid::Uuid,
    agent: uuid::Uuid,
    spec: &crate::entities::harness::AgentCapabilitySpec,
) -> AppResult<String> {
    let current = reader
        .load_for_execution(company, agent)
        .await?
        .ok_or_else(|| invalid("Agent is unavailable"))?;
    if current.agent.id != agent || current.agent.company_id.is_some_and(|id| id != company) {
        return Err(invalid("Agent scope changed"));
    }
    if current.agent.response_contract != spec.response_contract
        || serde_json::to_value(&current.skills)
            .map_err(|_| invalid("Invalid capability snapshot"))?
            != serde_json::to_value(&spec.skills)
                .map_err(|_| invalid("Invalid capability snapshot"))?
        || current.sub_agent_scope.allowed_ids() != spec.sub_agents.allowed_ids()
    {
        return Err(invalid("Capabilities changed after dispatch preparation"));
    }
    let identity = serde_json::json!({"name":current.agent.name,"instructions":current.agent.system_prompt,
        "provider":current.agent.provider,"model":current.agent.model,"config":current.agent.config_json,
        "skills":current.skills,
        "sub_agents":current.sub_agent_scope.allowed_ids(), "response_contract":current.agent.response_contract});
    Ok(format!(
        "{:x}",
        Sha256::digest(identity.to_string().as_bytes())
    ))
}

pub(super) fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
pub(super) fn applied(outcome: WriteOutcome<RunCheckpoint>) -> AppResult<RunCheckpoint> {
    match outcome {
        WriteOutcome::Applied(run) | WriteOutcome::AlreadyApplied(run) => Ok(run),
        WriteOutcome::OwnershipLost => Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        )),
        WriteOutcome::RevisionConflict => Err(AppError::Conflict(
            "Rig checkpoint writer superseded".into(),
        )),
    }
}

/// The closed set of durable owners of one checkpoint. Each binding supplies its own
/// authority, checkpoint writes and tool dispatch; the drive loop below is shared, so a
/// workflow run never constructs legacy task ownership or enters the legacy tool bridge.
pub(super) enum Binding<'a> {
    Legacy(Box<LegacyBinding<'a>>),
    Workflow(Box<super::workflow::WorkflowBinding<'a>>),
}

pub(super) struct LegacyBinding<'a> {
    scope: &'a RunExecution,
    compiled: CompiledRun,
    capabilities: Arc<dyn AgentCapabilityReader>,
    authorization: String,
    spec: Box<crate::entities::harness::AgentCapabilitySpec>,
}

/// A durable admission either enters the provider or parks the execution before I/O.
pub(super) enum Admitted {
    Enter(Box<RunCheckpoint>),
    Suspend(crate::domain::workflow::WaitId),
}

/// The next state after a durable write: keep driving, or stop because the owner parked.
/// Legacy suspension has no workflow wait identity.
pub(super) enum Progress {
    Continue(RunCheckpoint),
    Suspend(RunCheckpoint, Option<crate::domain::workflow::WaitId>),
}

pub(super) enum Driven {
    Completed(AgentExecutionOutput),
    Suspended(Option<crate::domain::workflow::WaitId>),
}

impl Binding<'_> {
    async fn authorize(&self, checkpoint: &RunCheckpoint) -> AppResult<()> {
        match self {
            Self::Legacy(legacy) => legacy.authorize(checkpoint).await,
            // The workflow store rechecks current authority inside admission/preparation.
            Self::Workflow(_) => Ok(()),
        }
    }

    fn preamble(&self) -> AppResult<String> {
        match self {
            Self::Legacy(legacy) => legacy.compiled.render_preamble(),
            Self::Workflow(workflow) => Ok(workflow.preamble()),
        }
    }

    fn definitions(&self) -> Vec<rig::completion::ToolDefinition> {
        match self {
            Self::Legacy(legacy) => legacy.compiled.tools.definitions(),
            Self::Workflow(workflow) => workflow.definitions(),
        }
    }

    pub(super) fn supported_ids(&self) -> Vec<crate::entities::value_objects::ToolId> {
        match self {
            Self::Legacy(legacy) => legacy.compiled.tools.supported_ids(),
            Self::Workflow(workflow) => workflow.supported_ids(),
        }
    }

    /// A workflow request parked before I/O keeps its saved identity until one fresh
    /// admission consumes it; every other request gets a new identity.
    fn reservation(&mut self, computed: ModelReservation) -> AppResult<ModelReservation> {
        match self {
            Self::Legacy(_) => Ok(computed),
            Self::Workflow(workflow) => workflow.reservation(computed),
        }
    }

    async fn admit(
        &mut self,
        checkpoint: &RunCheckpoint,
        reservation: ModelReservation,
        fingerprint: [u8; 32],
    ) -> AppResult<Admitted> {
        match self {
            Self::Legacy(legacy) => Ok(Admitted::Enter(Box::new(applied(
                legacy
                    .scope
                    .store
                    .reserve_model(&legacy.write(checkpoint), reservation)
                    .await?,
            )?))),
            Self::Workflow(workflow) => workflow.admit(checkpoint, reservation, fingerprint).await,
        }
    }

    async fn settle(
        &self,
        checkpoint: &RunCheckpoint,
        turn: SavedModelTurn,
    ) -> AppResult<Progress> {
        match self {
            Self::Legacy(legacy) => Ok(Progress::Continue(applied(
                legacy
                    .scope
                    .store
                    .commit_model_turn(&legacy.write(checkpoint), turn)
                    .await?,
            )?)),
            Self::Workflow(workflow) => workflow.settle(checkpoint, turn).await,
        }
    }

    async fn tool(&self, checkpoint: &RunCheckpoint, call: SavedToolCall) -> AppResult<Progress> {
        match self {
            Self::Legacy(legacy) => legacy.tool(checkpoint, call).await,
            Self::Workflow(workflow) => workflow.tool(checkpoint, call).await,
        }
    }

    async fn final_output(
        &self,
        checkpoint: &RunCheckpoint,
        output: AgentExecutionOutput,
    ) -> AppResult<RunCheckpoint> {
        match self {
            Self::Legacy(legacy) => applied(
                legacy
                    .scope
                    .store
                    .save_final_output(&legacy.write(checkpoint), output)
                    .await?,
            ),
            Self::Workflow(workflow) => workflow.final_output(checkpoint, output).await,
        }
    }

    async fn invalid_output(&self, checkpoint: &RunCheckpoint) -> AppResult<RunCheckpoint> {
        match self {
            Self::Legacy(legacy) => applied(
                legacy
                    .scope
                    .store
                    .fail_invalid_output(&legacy.write(checkpoint))
                    .await?,
            ),
            Self::Workflow(workflow) => workflow.invalid_output(checkpoint).await,
        }
    }
}

impl LegacyBinding<'_> {
    fn write(&self, checkpoint: &RunCheckpoint) -> RunWrite {
        RunWrite {
            company_id: self.scope.company_id,
            run_id: checkpoint.run_id,
            lease: self.scope.lease,
            expected_revision: checkpoint.revision,
        }
    }

    async fn authorize(&self, checkpoint: &RunCheckpoint) -> AppResult<()> {
        if authorization(
            self.capabilities.as_ref(),
            self.scope.company_id,
            checkpoint.identity.agent_id,
            &self.spec,
        )
        .await?
            != self.authorization
        {
            return Err(invalid("Execution authorization changed"));
        }
        Ok(())
    }

    async fn tool(&self, checkpoint: &RunCheckpoint, call: SavedToolCall) -> AppResult<Progress> {
        let prepared = applied(
            self.scope
                .store
                .prepare_invocation(&self.write(checkpoint), call.invocation_id)
                .await?,
        )?;
        let correlation = ToolCorrelationId::parse(&call.invocation_id.0.to_string())?;
        let invocation = self
            .compiled
            .tools
            .invoke(&call.tool_id, &correlation, call.arguments)
            .await?;
        let stored = self
            .scope
            .store
            .load_run(
                self.scope.company_id,
                self.scope.lease.task_id,
                prepared.run_id,
            )
            .await?
            .ok_or_else(|| invalid("Tool continuation disappeared"))?;
        if invocation.suspends_run() {
            let settled = stored
                .invocations
                .iter()
                .find(|inv| inv.call.invocation_id == call.invocation_id)
                .is_some_and(|inv| {
                    matches!(
                        inv.state,
                        InvocationState::Waiting
                            | InvocationState::Ready
                            | InvocationState::Completed
                            | InvocationState::Failed
                    )
                });
            if stored.revision.0 <= prepared.revision.0 || !settled {
                return Err(invalid(
                    "Tool suspension has no matching durable transition",
                ));
            }
            return Ok(Progress::Suspend(stored, None));
        }
        if let Some(receipt) = stored.invocations.iter().find(|saved| {
            saved.call.invocation_id == call.invocation_id
                && saved.state == InvocationState::Completed
        }) {
            if receipt.result.as_ref() != Some(&invocation.output) {
                return Err(invalid("Tool receipt does not match its committed output"));
            }
            return Ok(Progress::Continue(stored));
        }
        Ok(Progress::Continue(applied(
            self.scope
                .store
                .record_result(
                    &self.write(&prepared),
                    call.invocation_id,
                    invocation.output,
                )
                .await?,
        )?))
    }
}

/// Canonical provider request identity: provider, model and the complete completion
/// request (frozen preamble, saved history, tool declarations, repair, limits). The
/// model handle carries credentials; they never enter this digest.
pub(super) fn request_fingerprint(
    identity: &RunIdentity,
    request: &CompletionRequest,
) -> AppResult<[u8; 32]> {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "provider": identity.provider,
        "model": identity.model,
        "request": request,
    }))
    .map_err(|_| invalid("Invalid Rig request"))?;
    Ok(Sha256::digest(canonical).into())
}

pub(super) struct Execution<'a> {
    pub(super) inputs: super::diagnostics::InputCounts,
    pub(super) binding: Binding<'a>,
    pub(super) checkpoint: RunCheckpoint,
    pub(super) model: ModelHandle,
    pub(super) deadline: tokio::time::Instant,
    pub(super) secret: &'a str,
}
impl Execution<'_> {
    async fn drive_legacy(mut self) -> AppResult<AgentExecutionOutput> {
        match self.drive().await? {
            Driven::Completed(output) => Ok(output),
            Driven::Suspended(_) => {
                Ok(self.output(String::new(), AgentExecutionDisposition::Suspended))
            }
        }
    }

    pub(super) async fn drive(&mut self) -> AppResult<Driven> {
        loop {
            if tokio::time::Instant::now() >= self.deadline {
                return Err(AppError::Timeout("Rig deadline exceeded".into()));
            }
            if let Some(output) = &self.checkpoint.final_output {
                return Ok(Driven::Completed(output.clone()));
            }
            if self.checkpoint.state == RunState::BudgetExceeded {
                return Err(AppError::Execution(
                    crate::app_error::ExecutionFailure::Budget,
                ));
            }
            if self.checkpoint.state == RunState::InvalidOutput {
                return Err(crate::services::response_contract::invalid_output());
            }
            if self.checkpoint.state != RunState::Active {
                return Err(invalid("Rig continuation is not executable"));
            }
            self.binding.authorize(&self.checkpoint).await?;
            if let Some(call) = self.checkpoint.pending_call().cloned() {
                if self.checkpoint.invocations.iter().any(|inv| {
                    inv.call.invocation_id == call.invocation_id
                        && inv.state == InvocationState::Indeterminate
                }) {
                    return Err(AppError::Execution(
                        crate::app_error::ExecutionFailure::IndeterminateEffect,
                    ));
                }
                match self.binding.tool(&self.checkpoint, call).await? {
                    Progress::Continue(checkpoint) => self.checkpoint = checkpoint,
                    Progress::Suspend(checkpoint, wait) => {
                        self.checkpoint = checkpoint;
                        return Ok(Driven::Suspended(wait));
                    }
                }
                continue;
            }
            if let Some(turn) = self.checkpoint.turns.last()
                && turn.calls.is_empty()
            {
                if turn.invalid_response.is_some() {
                    if self.checkpoint.repair_count()
                        >= crate::services::response_contract::MAX_REPAIR_CALLS
                    {
                        self.checkpoint = self.binding.invalid_output(&self.checkpoint).await?;
                        return Err(crate::services::response_contract::invalid_output());
                    }
                    if let Some(stop) = self.model_turn().await? {
                        return Ok(stop);
                    }
                    continue;
                }
                let output = self.final_candidate(&turn.text)?;
                self.checkpoint = self
                    .binding
                    .final_output(&self.checkpoint, output.clone())
                    .await?;
                return Ok(Driven::Completed(output));
            }
            if let Some(stop) = self.model_turn().await? {
                return Ok(stop);
            }
        }
    }

    fn final_candidate(&self, text: &str) -> AppResult<AgentExecutionOutput> {
        let mut output = self.output(
            sanitize_text(text, Some(self.secret)),
            AgentExecutionDisposition::Completed,
        );
        if let Some(contract) = &self.checkpoint.identity.response_contract {
            let structured = StructuredResponse::validate(
                contract,
                text,
                Some(self.secret),
                &JsonResponseValidator,
            )?
            .map_err(|_| crate::services::response_contract::invalid_output())?;
            output = self.output(
                structured.body().into(),
                AgentExecutionDisposition::Completed,
            );
            output.structured = Some(structured);
        }
        Ok(output)
    }

    /// Some when the owner parked instead of entering the provider, or instead of
    /// progressing past a settled response.
    async fn model_turn(&mut self) -> AppResult<Option<Driven>> {
        let repair = super::final_response::repair_reservation(&self.checkpoint)?;
        let request = CompletionRequest {
            model: None,
            preamble: Some(super::final_response::response_preamble(
                &self.checkpoint,
                self.binding.preamble()?,
                repair.as_ref(),
            )?),
            chat_history: super::protocol::history(&self.checkpoint)?,
            documents: vec![],
            tools: if repair.is_some() {
                vec![]
            } else {
                self.binding.definitions()
            },
            temperature: None,
            max_tokens: Some(self.checkpoint.policy.request_output_tokens),
            tool_choice: None,
            additional_params: None,
            output_schema: None,
            record_telemetry_content: false,
        };
        request
            .validate_message_content()
            .map_err(|_| invalid("Invalid Rig model history"))?;
        // One UTF-8 byte per token is a conservative fallback, covering schemas and metadata too.
        let input_tokens = serde_json::to_vec(&request)
            .map_err(|_| invalid("Invalid Rig request"))?
            .len() as u64;
        let fingerprint = request_fingerprint(&self.checkpoint.identity, &request)?;
        let reservation = self.binding.reservation(ModelReservation {
            repair,
            request_id: ModelRequestId(uuid::Uuid::new_v4()),
            input_tokens,
            output_tokens: self.checkpoint.policy.request_output_tokens,
        })?;
        match self
            .binding
            .admit(&self.checkpoint, reservation.clone(), fingerprint)
            .await?
        {
            Admitted::Enter(checkpoint) => self.checkpoint = *checkpoint,
            Admitted::Suspend(wait) => return Ok(Some(Driven::Suspended(Some(wait)))),
        }
        let response = super::providers::complete(&self.model, request).await?;
        let mut turn =
            super::protocol::capture(&self.checkpoint.identity.provider, &reservation, response)?;
        super::final_response::assess_candidate(
            &self.checkpoint,
            self.secret,
            &mut turn,
            reservation.repair.as_ref(),
            super::diagnostics::OutputContext {
                inputs: self.inputs,
                supported_tools: self.binding.supported_ids(),
            },
        )?;
        match self.binding.settle(&self.checkpoint, turn).await? {
            Progress::Continue(checkpoint) => {
                self.checkpoint = checkpoint;
                Ok(None)
            }
            Progress::Suspend(checkpoint, wait) => {
                self.checkpoint = checkpoint;
                Ok(Some(Driven::Suspended(wait)))
            }
        }
    }

    pub(super) fn output(
        &self,
        content: String,
        disposition: AgentExecutionDisposition,
    ) -> AgentExecutionOutput {
        super::diagnostics::output(
            &self.checkpoint,
            self.inputs,
            self.binding.supported_ids(),
            content,
            disposition,
        )
    }
}

fn validate_deployment() -> AppResult<()> {
    let policy = RigExecutionPolicy::default();
    policy.validate()?;
    if usize::from(policy.tool_calls) != super::tools::MAX_TOOL_INVOCATIONS
        || policy.request_input_tokens > policy.total_input_tokens
        || policy.request_output_tokens > policy.total_output_tokens
        || policy.model_calls > u16::from(crate::entities::harness::MAX_RIG_MAX_TURNS)
    {
        return Err(AppError::Internal(
            "Incoherent Rig deployment limits".into(),
        ));
    }
    Ok(())
}
