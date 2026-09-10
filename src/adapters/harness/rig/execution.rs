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
    mcp: Arc<crate::services::mcp_runtime::McpRuntime>,
}
impl RigHarness {
    pub fn new(
        capabilities: Arc<dyn AgentCapabilityReader>,
        mcp: Arc<crate::services::mcp_runtime::McpRuntime>,
    ) -> AppResult<Self> {
        Self::with_providers(
            ProviderRegistry::standard()
                .map_err(|_| AppError::Internal("Rig provider transport unavailable".into()))?,
            capabilities,
            mcp,
        )
    }

    pub fn with_providers(
        providers: ProviderRegistry,
        capabilities: Arc<dyn AgentCapabilityReader>,
        mcp: Arc<crate::services::mcp_runtime::McpRuntime>,
    ) -> AppResult<Self> {
        providers
            .validate_coverage()
            .map_err(|_| AppError::Internal("Rig provider coverage incomplete".into()))?;
        validate_deployment()?;
        Ok(Self {
            providers,
            capabilities,
            mcp,
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
    fn compile(
        &self,
        run: &AgentRun<'_>,
        scope: &RunExecution,
        mcp: Arc<dyn crate::services::harness::mcp::HarnessMcpToolHost>,
    ) -> AppResult<CompiledRun> {
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
                approvals: run.approvals.clone(),
                mcp: Some(mcp),
            },
            run.tool_host.clone(),
        )?;
        if !compiled.tools.diagnostics.missing_context.is_empty() {
            return Err(invalid(
                "Rig tool grants require unavailable execution context",
            ));
        }
        if run
            .spec
            .required_tool_ids()
            .iter()
            .any(|id| id.as_str() == "request_approval")
            && run.approvals.is_none()
        {
            return Err(invalid("Human checkpoint requires approval context"));
        }
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
        let mcp = tokio::time::timeout_at(
            run.deadline,
            self.mcp.prepare(
                crate::services::mcp_runtime::McpRunScope {
                    company_id: scope.company_id,
                    agent_id: run.agent_id,
                    task_id: scope.lease.task_id,
                    run_id: checkpoint.run_id.0,
                },
                HarnessKind::Rig,
                scope.store.mcp_journal(scope.lease),
            ),
        )
        .await
        .map_err(|_| AppError::Timeout("Rig preparation deadline exceeded".into()))??;
        let compiled = self.compile(&run, scope, mcp)?;
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
            scope,
            compiled,
            checkpoint,
            model,
            deadline: run.deadline,
            secret: run.api_key,
            capabilities: self.capabilities.clone(),
            authorization,
            spec: run.spec,
        };
        // Keep the external runtime state out of the worker/dispatch poll frame. No detached work.
        tokio::time::timeout_at(run.deadline, Box::pin(execution.drive()))
            .await
            .map_err(|_| AppError::Timeout("Rig deadline exceeded".into()))?
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
        "grants":run.spec.required_tool_ids(), "sub_agents":match &run.spec.sub_agents { crate::entities::harness::SubAgentScope::AllCompanySiblings => serde_json::json!({"scope":"all_company_siblings"}), crate::entities::harness::SubAgentScope::Restricted(ids) => serde_json::json!({"scope":"restricted","ids":ids}) }, "config":config, "response_contract":run.spec.response_contract });
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
    if current.agent.id != agent
        || current.agent.company_id.is_some_and(|id| id != company)
        || current.agent.harness_kind != HarnessKind::Rig
    {
        return Err(invalid("Agent scope changed"));
    }
    if current.agent.response_contract != spec.response_contract
        || current.agent.granted_tool_ids != spec.granted_tools
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
        "grants":current.agent.granted_tool_ids,"policy":current.agent.native_tool_policy,"skills":current.skills,
        "sub_agents":current.sub_agent_scope.allowed_ids(), "response_contract":current.agent.response_contract});
    Ok(format!(
        "{:x}",
        Sha256::digest(identity.to_string().as_bytes())
    ))
}

fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
fn applied(outcome: WriteOutcome<RunCheckpoint>) -> AppResult<RunCheckpoint> {
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

struct Execution<'a> {
    inputs: super::diagnostics::InputCounts,
    scope: &'a RunExecution,
    compiled: CompiledRun,
    checkpoint: RunCheckpoint,
    model: ModelHandle,
    deadline: tokio::time::Instant,
    secret: &'a str,
    capabilities: Arc<dyn AgentCapabilityReader>,
    authorization: String,
    spec: Box<crate::entities::harness::AgentCapabilitySpec>,
}
impl Execution<'_> {
    fn write(&self) -> RunWrite {
        RunWrite {
            company_id: self.scope.company_id,
            run_id: self.checkpoint.run_id,
            lease: self.scope.lease,
            expected_revision: self.checkpoint.revision,
        }
    }

    async fn drive(mut self) -> AppResult<AgentExecutionOutput> {
        loop {
            if tokio::time::Instant::now() >= self.deadline {
                return Err(AppError::Timeout("Rig deadline exceeded".into()));
            }
            if let Some(output) = &self.checkpoint.final_output {
                return Ok(output.clone());
            }
            if self.checkpoint.state == RunState::InvalidOutput {
                return Err(crate::services::response_contract::invalid_output());
            }
            if self.checkpoint.state != RunState::Active {
                return Err(invalid("Rig continuation is not executable"));
            }
            if authorization(
                self.capabilities.as_ref(),
                self.scope.company_id,
                self.checkpoint.identity.agent_id,
                &self.spec,
            )
            .await?
                != self.authorization
            {
                return Err(invalid("Execution authorization changed"));
            }
            if let Some(call) = self.checkpoint.pending_call().cloned() {
                if self.tool(call).await? {
                    return Ok(self.output(String::new(), AgentExecutionDisposition::Suspended));
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
                        self.checkpoint =
                            applied(self.scope.store.fail_invalid_output(&self.write()).await?)?;
                        return Err(crate::services::response_contract::invalid_output());
                    }
                    self.model_turn().await?;
                    continue;
                }
                let mut output = self.output(
                    sanitize_text(&turn.text, Some(self.secret)),
                    AgentExecutionDisposition::Completed,
                );
                if let Some(contract) = &self.checkpoint.identity.response_contract {
                    let structured = StructuredResponse::validate(
                        contract,
                        &turn.text,
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
                self.checkpoint = applied(
                    self.scope
                        .store
                        .save_final_output(&self.write(), output.clone())
                        .await?,
                )?;
                return Ok(output);
            }
            self.model_turn().await?;
        }
    }

    async fn model_turn(&mut self) -> AppResult<()> {
        let repair = super::final_response::repair_reservation(&self.checkpoint)?;
        let request = CompletionRequest {
            model: None,
            preamble: Some(super::final_response::response_preamble(
                &self.checkpoint,
                &self.compiled,
                repair.as_ref(),
            )?),
            chat_history: super::protocol::history(&self.checkpoint)?,
            documents: vec![],
            tools: if repair.is_some() {
                vec![]
            } else {
                self.compiled.tools.definitions()
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
        let reservation = ModelReservation {
            repair,
            request_id: ModelRequestId(uuid::Uuid::new_v4()),
            input_tokens,
            output_tokens: self.checkpoint.policy.request_output_tokens,
        };
        self.checkpoint = applied(
            self.scope
                .store
                .reserve_model(&self.write(), reservation.clone())
                .await?,
        )?;
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
                supported_tools: self.compiled.tools.supported_ids(),
            },
        )?;
        self.checkpoint = applied(
            self.scope
                .store
                .commit_model_turn(&self.write(), turn)
                .await?,
        )?;
        Ok(())
    }

    async fn tool(&mut self, call: SavedToolCall) -> AppResult<bool> {
        if self.checkpoint.invocations.iter().any(|inv| {
            inv.call.invocation_id == call.invocation_id
                && inv.state == InvocationState::Indeterminate
        }) {
            return Err(AppError::Execution(
                crate::app_error::ExecutionFailure::IndeterminateEffect,
            ));
        }
        self.checkpoint = applied(
            self.scope
                .store
                .prepare_invocation(&self.write(), call.invocation_id)
                .await?,
        )?;
        let reference = InvocationRef {
            run_id: self.checkpoint.run_id,
            invocation_id: call.invocation_id,
            expected_revision: self.checkpoint.revision,
        };
        let correlation = ToolCorrelationId::parse(&call.invocation_id.0.to_string())?;
        let invocation = self
            .compiled
            .tools
            .invoke_saved(&call.tool_id, &correlation, call.arguments, Some(reference))
            .await?;
        let stored = self
            .scope
            .store
            .load_run(
                self.scope.company_id,
                self.scope.lease.task_id,
                self.checkpoint.run_id,
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
            if stored.revision.0 <= self.checkpoint.revision.0 || !settled {
                return Err(invalid(
                    "Tool suspension has no matching durable transition",
                ));
            }
            self.checkpoint = stored;
            return Ok(true);
        }
        if let Some(receipt) = stored.invocations.iter().find(|saved| {
            saved.call.invocation_id == call.invocation_id
                && saved.state == InvocationState::Completed
        }) {
            if receipt.result.as_ref() != Some(&invocation.output) {
                return Err(invalid("Tool receipt does not match its committed output"));
            }
            self.checkpoint = stored;
        } else {
            self.checkpoint = applied(
                self.scope
                    .store
                    .record_result(&self.write(), call.invocation_id, invocation.output)
                    .await?,
            )?;
        }
        Ok(false)
    }

    fn output(
        &self,
        content: String,
        disposition: AgentExecutionDisposition,
    ) -> AgentExecutionOutput {
        super::diagnostics::output(
            &self.checkpoint,
            self.inputs,
            self.compiled.tools.supported_ids(),
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
