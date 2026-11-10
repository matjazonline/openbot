//! Workflow binding of the shared durable drive loop. Ownership is the claimed
//! WorkflowFence; effects enter ActionService only. Current agent/model authority is
//! enforced by the store inside model admission and invocation preparation, so a
//! revocation cannot race a separate pre-check.
use super::{
    diagnostics::InputCounts,
    execution::{Admitted, Driven, Execution, Progress, applied, invalid},
    providers::ResolvedModelRequest,
};
use crate::adapters::response_schema::JsonResponseValidator;
use crate::services::response_contract::ResponseContractValidator;
use crate::{
    app_error::{AppError, AppResult, ExecutionFailure},
    application::workflow::{
        actions::ModelToolCallId,
        agent::{AgentPreparation, AgentToolDispatch, AgentToolObservation, PreparedAgent},
        checkpoints::{
            WorkflowCheckpointStore, WorkflowCheckpointUpdate, WorkflowCheckpointWrite,
            WorkflowModelAdmission, WorkflowModelRequest, WorkflowModelSettlement,
        },
        decisions::DecisionNotices,
        lease::{
            ClaimedWorkflow, LeasePolicy, SuspendedWorkflow, WorkflowFailure, WorkflowFence,
            WorkflowHandlerOutcome,
        },
        polling::WorkflowStepKind,
        worker::WorkflowHandler,
    },
    domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure},
    entities::value_objects::ToolId,
    services::harness::{AgentExecutionOutput, runs::*},
};
use async_trait::async_trait;
use rig_core::completion::ToolDefinition;
use secrecy::ExposeSecret;
use serde_json::json;
use sha2::{Digest, Sha256};

pub(super) struct WorkflowBinding<'a> {
    pub(super) store: &'a dyn WorkflowCheckpointStore,
    pub(super) tools: &'a dyn AgentToolDispatch,
    pub(super) prepared: &'a PreparedAgent,
    pub(super) fence: WorkflowFence,
    pub(super) lease: LeasePolicy,
    pub(super) notices: Option<&'a DecisionNotices>,
    /// A request parked before I/O; consumed by the next admission attempt.
    pub(super) blocked: Option<ModelReservation>,
}

impl WorkflowBinding<'_> {
    pub(super) fn write(&self, checkpoint: &RunCheckpoint) -> WorkflowCheckpointWrite {
        WorkflowCheckpointWrite {
            fence: self.fence,
            checkpoint: checkpoint.run_id,
            expected_revision: checkpoint.revision,
            lease_policy: self.lease,
        }
    }

    pub(super) fn preamble(&self) -> String {
        self.prepared.preamble().to_owned()
    }

    pub(super) fn definitions(&self) -> Vec<ToolDefinition> {
        self.prepared
            .declarations()
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name.as_str().to_owned(),
                description: tool.description,
                parameters: tool.parameters,
            })
            .collect()
    }

    pub(super) fn supported_ids(&self) -> Vec<ToolId> {
        self.prepared
            .tools()
            .iter()
            .map(|tool| ToolId::from(tool.declared.as_str()))
            .collect()
    }

    pub(super) fn reservation(
        &mut self,
        computed: ModelReservation,
    ) -> AppResult<ModelReservation> {
        let policy = self.prepared.policy()?;
        if self.prepared.history_exchange().is_some()
            && computed
                .input_tokens
                .checked_add(computed.output_tokens)
                .is_none_or(|total| total > policy.request_input_tokens)
        {
            return Err(AppError::Execution(ExecutionFailure::Budget));
        }
        let Some(saved) = self.blocked.take() else {
            return Ok(computed);
        };
        if saved.repair != computed.repair
            || saved.input_tokens != computed.input_tokens
            || saved.output_tokens != computed.output_tokens
        {
            return Err(AppError::Conflict("Approved model request changed".into()));
        }
        Ok(saved)
    }

    pub(super) async fn admit(
        &self,
        checkpoint: &RunCheckpoint,
        reservation: ModelReservation,
        fingerprint: [u8; 32],
    ) -> AppResult<Admitted> {
        let request = WorkflowModelRequest {
            reservation,
            fingerprint,
        };
        match self
            .store
            .reserve_workflow_model(self.write(checkpoint), request, self.notices)
            .await?
        {
            // Only a fresh committed admission enters the provider, immediately.
            WorkflowModelAdmission::New(admission) => {
                Ok(Admitted::Enter(Box::new(admission.consume())))
            }
            WorkflowModelAdmission::Parked(wait) => Ok(Admitted::Suspend(wait)),
            WorkflowModelAdmission::BudgetExhausted => {
                Err(AppError::Execution(ExecutionFailure::Budget))
            }
            WorkflowModelAdmission::TokenRefused => {
                Err(AppError::Execution(ExecutionFailure::CheckpointDenied))
            }
            WorkflowModelAdmission::OwnershipLost => {
                Err(AppError::Execution(ExecutionFailure::OwnershipLost))
            }
            // Replay or a still-owned slot never grants another provider entry.
            WorkflowModelAdmission::PreviouslyAdmitted(_)
            | WorkflowModelAdmission::ActiveRequest
            | WorkflowModelAdmission::RevisionConflict => Err(AppError::Conflict(
                "Rig model admission was not granted".into(),
            )),
        }
    }

    pub(super) async fn settle(
        &self,
        checkpoint: &RunCheckpoint,
        turn: SavedModelTurn,
    ) -> AppResult<Progress> {
        match self
            .store
            .settle_workflow_model(self.write(checkpoint), turn, self.notices)
            .await?
        {
            WorkflowModelSettlement::Saved(outcome) => Ok(Progress::Continue(applied(*outcome)?)),
            WorkflowModelSettlement::Parked { checkpoint, wait } => {
                Ok(Progress::Suspend(*checkpoint, wait))
            }
            // Received truth is saved; it grants no further progression.
            WorkflowModelSettlement::Refused(_) => {
                Err(AppError::Execution(ExecutionFailure::CheckpointDenied))
            }
            WorkflowModelSettlement::ReviewUnavailable(_) => Err(AppError::BadRequest(
                "Token allowance review is unavailable".into(),
            )),
        }
    }

    pub(super) async fn update(
        &self,
        checkpoint: &RunCheckpoint,
        update: WorkflowCheckpointUpdate,
    ) -> AppResult<RunCheckpoint> {
        applied(
            self.store
                .update_workflow_checkpoint(self.write(checkpoint), update)
                .await?,
        )
    }

    pub(super) async fn tool(
        &self,
        checkpoint: &RunCheckpoint,
        call: SavedToolCall,
    ) -> AppResult<Progress> {
        let invocation = call.invocation_id;
        let prepared = self
            .update(
                checkpoint,
                WorkflowCheckpointUpdate::PrepareInvocation(invocation),
            )
            .await?;
        // Only prepared declarations dispatch; a model-invented name or target never does.
        let Some(tool) = self.prepared.tool(call.tool_id.as_str()) else {
            let refused = json!({"error":{"code":"unknown_tool","message":"Tool is not available to this agent"}});
            return Ok(Progress::Continue(
                self.update(
                    &prepared,
                    WorkflowCheckpointUpdate::InvocationResult {
                        invocation,
                        result: refused,
                    },
                )
                .await?,
            ));
        };
        let id = ModelToolCallId::parse(invocation.0.to_string())?;
        match self
            .tools
            .dispatch(self.fence, tool, id, call.arguments)
            .await?
        {
            AgentToolObservation::Completed(result) => Ok(Progress::Continue(
                self.update(
                    &prepared,
                    WorkflowCheckpointUpdate::InvocationResult { invocation, result },
                )
                .await?,
            )),
            AgentToolObservation::Parked(wait) => Ok(Progress::Suspend(prepared, wait)),
            AgentToolObservation::Uncertain => {
                Err(AppError::Execution(ExecutionFailure::IndeterminateEffect))
            }
        }
    }

    pub(super) async fn final_output(
        &self,
        checkpoint: &RunCheckpoint,
        output: AgentExecutionOutput,
    ) -> AppResult<RunCheckpoint> {
        self.update(
            checkpoint,
            WorkflowCheckpointUpdate::FinalOutput(Box::new(output)),
        )
        .await
    }

    pub(super) async fn invalid_output(
        &self,
        checkpoint: &RunCheckpoint,
    ) -> AppResult<RunCheckpoint> {
        self.update(checkpoint, WorkflowCheckpointUpdate::InvalidOutput)
            .await
    }
}

/// The model-step workflow handler (`agent.run`, and tool-free `ai.classify` and
/// `decision.agent`, 06-D6 C): trusted preparation, one checkpoint per execution and the
/// shared durable drive loop. No legacy lease, MCP host or tool bridge.
pub struct WorkflowAgentRuntime<S, T> {
    providers: super::providers::ProviderRegistry,
    store: S,
    tools: T,
    notices: Option<DecisionNotices>,
    lease: LeasePolicy,
    /// Automatic recall and revalidation (phase 05 E7). Always present: until a configured
    /// recall replaces it, nothing is recalled and earlier memory is never replayed unvalidated
    /// (U5), so no composition can leave a resumed attempt unchecked.
    recall: std::sync::Arc<crate::application::workflow::memory_recall::MemoryRecall>,
}

impl<S, T> WorkflowAgentRuntime<S, T>
where
    S: WorkflowCheckpointStore + AgentPreparation,
    T: AgentToolDispatch,
{
    /// Composition-time access to the tool dispatch, before the runtime is shared.
    pub fn tools_mut(&mut self) -> &mut T {
        &mut self.tools
    }

    /// Composition-time automatic recall (E7), before the runtime is shared.
    pub fn set_recall(
        &mut self,
        recall: std::sync::Arc<crate::application::workflow::memory_recall::MemoryRecall>,
    ) {
        self.recall = recall;
    }

    pub fn new(
        providers: super::providers::ProviderRegistry,
        store: S,
        tools: T,
        notices: Option<DecisionNotices>,
        lease: LeasePolicy,
    ) -> Self
    where
        S: crate::application::workflow::memory_recall::MemoryRecallPersistence + Clone + 'static,
    {
        let recall = std::sync::Arc::new(
            crate::application::workflow::memory_recall::MemoryRecall::unconfigured(
                std::sync::Arc::new(store.clone()),
            ),
        );
        Self {
            providers,
            store,
            tools,
            notices,
            lease,
            recall,
        }
    }

    async fn run(&self, claim: &ClaimedWorkflow) -> AppResult<WorkflowHandlerOutcome> {
        let deadline = claim
            .window
            .run_deadline
            .min(tokio::time::Instant::now() + std::time::Duration::from_millis(MAX_CLAIM_MS));
        use crate::application::workflow::agent::AgentPreparationResult;
        use crate::application::workflow::memory_recall::{RecallStep, RecallValidation};
        // Boxed: recall descends into the memory service; the parent holds a pointer (src/AGENTS.md).
        let step = Box::pin(self.recall.step(claim, self.lease)).await?;
        let (fetched, resumed) = match step {
            RecallStep::None => (None, false),
            RecallStep::Fresh(fetched) => (Some(fetched), false),
            RecallStep::Resumed => (None, true),
        };
        let launch = match self
            .store
            .prepare_agent_with_recall(
                claim,
                self.lease,
                crate::application::workflow::memory_recall::MemoryPreparation {
                    recall: fetched,
                    capture: self.recall.configured(),
                },
            )
            .await?
        {
            AgentPreparationResult::Ready(launch) => launch,
            AgentPreparationResult::Parked(wait) => {
                return Ok(WorkflowHandlerOutcome::Suspended(SuspendedWorkflow {
                    wait,
                }));
            }
            AgentPreparationResult::AtCapacity => {
                return Ok(WorkflowHandlerOutcome::Completed(Err(WorkflowFailure {
                    failure: StepFailure::new(
                        FailureClass::Terminal,
                        FailureCode::parse("agent.history_queue_full")
                            .expect("static failure code"),
                        None,
                    )
                    .expect("bounded queue refusal"),
                    safety: RetrySafety::SafeToRetry,
                })));
            }
        };
        // Once per claimed attempt, before its first provider entry (E7); never per tool round.
        let validation = self
            .recall
            .validate(claim, self.lease, &launch.prepared, resumed);
        match Box::pin(validation).await? {
            RecallValidation::Valid => {}
            RecallValidation::Revoked => {
                return Ok(memory_failure(
                    FailureClass::Terminal,
                    "memory.context_revoked",
                ));
            }
            RecallValidation::Unavailable => {
                return Ok(memory_failure(
                    FailureClass::Retryable,
                    "memory.context_unavailable",
                ));
            }
        }
        Ok(
            match Box::pin(self.run_ready(claim, &launch, deadline)).await? {
                Driven::Completed(output) => WorkflowHandlerOutcome::Completed(
                    structured(&output).map_err(|error| failure(&error)),
                ),
                Driven::Suspended(wait) => {
                    WorkflowHandlerOutcome::Suspended(SuspendedWorkflow { wait })
                }
            },
        )
    }

    async fn run_ready(
        &self,
        claim: &ClaimedWorkflow,
        launch: &crate::application::workflow::agent::AgentLaunch,
        deadline: tokio::time::Instant,
    ) -> AppResult<Driven> {
        let spec = launch.prepared.spec(claim.fence)?;
        JsonResponseValidator.validate_contract(
            spec.identity
                .response_contract
                .as_ref()
                .ok_or_else(|| invalid("Model step requires an output schema"))?,
        )?;
        let checkpoint = applied(
            self.store
                .open_workflow_checkpoint(claim.fence, &spec, self.lease)
                .await?,
        )?;
        launch
            .prepared
            .validate_mcp_results(&checkpoint.invocations)?;
        if let Some(output) = checkpoint.final_output.clone() {
            return Ok(Driven::Completed(output));
        }
        let mut binding = WorkflowBinding {
            store: &self.store,
            tools: &self.tools,
            prepared: &launch.prepared,
            fence: claim.fence,
            lease: self.lease,
            notices: self.notices.as_ref(),
            blocked: None,
        };
        let allowance_ms = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .ok_or_else(|| AppError::Timeout("Rig deadline exceeded".into()))?
            .as_millis()
            .max(1) as u64;
        let checkpoint = applied(
            self.store
                .reserve_workflow_execution(binding.write(&checkpoint), allowance_ms)
                .await?,
        )?;
        let catalogue = serde_json::to_vec(&binding.definitions())
            .map_err(|_| invalid("Invalid tool declarations"))?;
        let checkpoint = binding
            .update(
                &checkpoint,
                WorkflowCheckpointUpdate::BindTools(format!("{:x}", Sha256::digest(catalogue))),
            )
            .await?;
        binding.blocked = self
            .store
            .blocked_workflow_model(claim.fence, self.lease)
            .await?;
        let selection = launch.prepared.selection().agent();
        let model = self
            .providers
            .model(&ResolvedModelRequest {
                provider: &selection.model.provider.as_str().into(),
                model: &selection.model.model.as_str().into(),
                secret: &launch.secret,
                endpoint: endpoint(selection.agent_id.as_uuid()).as_deref(),
            })
            .map_err(|_| invalid("Rig model configuration unavailable"))?;
        let secret = launch.secret.expose_secret();
        let mut execution = Execution {
            inputs: InputCounts::prepared(&spec.initial_prompt, &spec.seed)?,
            binding: Box::new(binding),
            checkpoint,
            model,
            deadline,
            secret,
        };
        tokio::time::timeout_at(deadline, Box::pin(execution.drive()))
            .await
            .map_err(|_| AppError::Timeout("Rig deadline exceeded".into()))?
    }
}

/// Production uses the provider default endpoint; tests register a scripted listener.
fn endpoint(agent: uuid::Uuid) -> Option<String> {
    #[cfg(test)]
    {
        crate::services::test_support::scripted_agent_base_url(agent)
    }
    #[cfg(not(test))]
    {
        let _ = agent;
        None
    }
}

#[async_trait]
impl<S, T> WorkflowHandler for WorkflowAgentRuntime<S, T>
where
    S: WorkflowCheckpointStore + AgentPreparation,
    T: AgentToolDispatch,
{
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        crate::application::workflow::registry::ModelStep::of(&kind.0).is_some()
    }

    async fn execute(
        &self,
        _kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        Box::pin(self.run(claim))
            .await
            .unwrap_or_else(|error| WorkflowHandlerOutcome::Completed(Err(failure(&error))))
    }
}

/// The validated final JSON value; never the sanitized prose of an unstructured reply.
/// A recall revalidation outcome as the step's failure (E7).
fn memory_failure(class: FailureClass, code: &'static str) -> WorkflowHandlerOutcome {
    WorkflowHandlerOutcome::Completed(Err(WorkflowFailure {
        failure: StepFailure::new(
            class,
            FailureCode::parse(code).expect("static failure code"),
            None,
        )
        .expect("bounded memory failure"),
        safety: RetrySafety::SafeToRetry,
    }))
}

fn structured(output: &AgentExecutionOutput) -> AppResult<serde_json::Value> {
    let response = output
        .structured
        .as_ref()
        .ok_or_else(crate::services::response_contract::invalid_output)?;
    serde_json::from_str(response.body())
        .map_err(|_| crate::services::response_contract::invalid_output())
}

/// Typed, bounded step failure. Effects are ActionService-owned, so the shared retry
/// predicate (not this label) decides whether an earlier effect makes retry unsafe.
fn failure(error: &AppError) -> WorkflowFailure {
    use FailureClass::{Retryable, Terminal};
    if let AppError::KnowledgeRelease {
        content,
        destination,
    } = error
    {
        return WorkflowFailure {
            failure: StepFailure::from_knowledge_refusal(
                crate::domain::workflow::KnowledgeRefusal {
                    content: *content,
                    destination: *destination,
                },
            ),
            safety: RetrySafety::SafeToRetry,
        };
    }
    if let AppError::WorkflowGuard(violation) = error {
        return crate::application::workflow::actions::guard_step_failure("agent", violation);
    }
    let (class, suffix) = match error {
        AppError::Execution(ExecutionFailure::Budget) => (Terminal, "budget"),
        AppError::Execution(ExecutionFailure::RepeatedToolCall) => (Terminal, "repeated_tool_call"),
        AppError::Execution(ExecutionFailure::InvalidOutput) => (Terminal, "invalid_output"),
        AppError::Execution(ExecutionFailure::McpToolFailed) => (Terminal, "mcp_tool_failed"),
        AppError::Execution(ExecutionFailure::CheckpointDenied) => (Terminal, "tokens_refused"),
        AppError::Execution(ExecutionFailure::IndeterminateEffect) => (Terminal, "effect_unknown"),
        AppError::Execution(ExecutionFailure::Protocol) => (Retryable, "provider_protocol"),
        AppError::Execution(ExecutionFailure::OwnershipLost) => (Retryable, "interrupted"),
        AppError::Timeout(_) => (Retryable, "timeout"),
        AppError::BadRequest(_) | AppError::NotFound(_) | AppError::InvalidCredentials => {
            (Terminal, "rejected")
        }
        _ => (Retryable, "unresolved"),
    };
    let code = FailureCode::parse(format!("agent.{suffix}")).expect("static agent failure code");
    WorkflowFailure {
        failure: StepFailure::new(class, code, None).expect("bounded agent failure"),
        safety: RetrySafety::SafeToRetry,
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    fn workflow_guard_error_agent_handler_mapping_names_the_guard_and_stays_retryable() {
        let error = AppError::WorkflowGuard(
            crate::application::app_error::WorkflowGuardViolation::from_constraint(
                "workflow_guard:guard_workflow_token_proposal",
                "forced",
            )
            .unwrap(),
        );
        let result = failure(&error);
        assert_eq!(result.failure.class(), FailureClass::Retryable);
        assert_eq!(result.failure.code().as_str(), "agent.guard_violation");
        assert_eq!(
            result.failure.diagnostic(),
            Some("guard guard_workflow_token_proposal")
        );
        assert_eq!(result.safety, RetrySafety::SafeToRetry);
    }

    #[test]
    fn doop_agent_strict_mcp_error_maps_to_a_terminal_failure() {
        let result = super::failure(&crate::app_error::AppError::Execution(
            crate::app_error::ExecutionFailure::McpToolFailed,
        ));
        assert_eq!(result.failure.code().as_str(), "agent.mcp_tool_failed");
        assert_eq!(
            result.failure.class(),
            crate::domain::workflow::FailureClass::Terminal
        );
    }
}
