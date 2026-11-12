//! Workflow-local effects use the shared action owner and a durable parked wait.
mod schemas;
pub use schemas::{frozen_schema, input_schema, result_schema};

use crate::app_error::{AppError, AppResult};
use crate::application::agent::{ActionEffect, ActionRecovery, ApprovedActionPolicy};
use crate::application::workflow::{
    actions::ActionService,
    lease::{
        ClaimedWorkflow, LeasePolicy, SuspendedWorkflow, WorkflowFailure, WorkflowHandlerOutcome,
    },
    polling::WorkflowStepKind,
    publication::ToolSnapshot,
    registry::ToolContract,
    worker::WorkflowHandler,
};
use crate::domain::local_agents::{
    ExecutionSelection, LocalPrompt, OperationOutcome, ProfileId, SessionId,
};
use crate::domain::workflow::{
    CompanyId, FailureClass, FailureCode, RetrySafety, StepFailure, TypeName, WaitId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const KIND: &str = "local_agent.run";
pub const RESOURCE_KIND: &str = "local_agent_profile";
pub const MAX_WORKFLOW_TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRunInputs {
    pub profile_id: ProfileId,
    pub prompt: LocalPrompt,
    pub session_id: Option<SessionId>,
    pub approval: Option<Value>,
}

/// An internal approval subject; authored workflows cannot override these resolved facts.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenLocalRun {
    pub profile_id: ProfileId,
    pub prompt: LocalPrompt,
    pub session_id: Option<SessionId>,
    pub approval: Option<Value>,
    pub selection: ExecutionSelection,
    pub harness: crate::domain::local_agents::NativeHarnessSelection,
    pub deadline: DateTime<Utc>,
}

pub enum LocalRunProgress {
    Suspended(WaitId),
    Completed(Value),
    Failed(OperationOutcome),
}

/// The adapter commits the action, operation, wait and exact lease disposition together.
#[async_trait]
pub trait LocalAgentWorkflow: Send + Sync {
    async fn run_local_agent(
        &self,
        claim: &ClaimedWorkflow,
        inputs: &LocalRunInputs,
        policy: LeasePolicy,
    ) -> AppResult<LocalRunProgress>;
}

pub fn builtin_snapshot(company: CompanyId) -> AppResult<ToolSnapshot> {
    let name = TypeName::parse(KIND).map_err(|_| invalid())?;
    Ok(ToolSnapshot {
        company_id: company,
        workflow: None,
        contract: ToolContract {
            name: name.clone(),
            connection: None,
            input_schema: frozen_schema(),
            output_schema: result_schema(),
        },
        policy: ApprovedActionPolicy {
            capability: name,
            policy_revision: 1,
            effect: ActionEffect::Write,
            recovery: ActionRecovery::Reconcile,
        },
    })
}

pub fn is_trusted_snapshot(snapshot: &ToolSnapshot) -> AppResult<bool> {
    Ok(serde_json::to_value(snapshot).map_err(|_| invalid())?
        == serde_json::to_value(builtin_snapshot(snapshot.company_id)?).map_err(|_| invalid())?)
}

pub struct LocalAgentHandler<P> {
    actions: ActionService<P>,
    policy: LeasePolicy,
}
impl<P> LocalAgentHandler<P> {
    pub fn new(persistence: P, policy: LeasePolicy) -> Self {
        Self {
            actions: ActionService::new(persistence),
            policy,
        }
    }
}

#[async_trait]
impl<P: LocalAgentWorkflow> WorkflowHandler for LocalAgentHandler<P> {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        kind.0 == KIND
    }
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        if !self.supports(kind) {
            return failure("local_agent.unsupported", FailureClass::Terminal).into();
        }
        let inputs = match serde_json::from_value::<LocalRunInputs>(claim.activation.inputs.clone())
        {
            Ok(inputs) => inputs,
            Err(_) => return failure("local_agent.invalid_input", FailureClass::Terminal).into(),
        };
        // Box the persistence seam: this must not enlarge the worker's debug stack.
        match Box::pin(self.actions.run_local_agent(claim, &inputs, self.policy)).await {
            Ok(LocalRunProgress::Suspended(wait)) => {
                WorkflowHandlerOutcome::Suspended(SuspendedWorkflow { wait })
            }
            Ok(LocalRunProgress::Completed(result)) => {
                WorkflowHandlerOutcome::Completed(Ok(result))
            }
            Ok(LocalRunProgress::Failed(outcome)) => failure(
                match outcome {
                    OperationOutcome::Uncertain => "local_agent.uncertain",
                    OperationOutcome::Expired => "local_agent.expired",
                    OperationOutcome::Cancelled => "local_agent.cancelled",
                    _ => "local_agent.failed",
                },
                FailureClass::Terminal,
            )
            .into(),
            Err(AppError::KnowledgeRelease {
                content,
                destination,
            }) => WorkflowHandlerOutcome::Completed(Err(WorkflowFailure {
                failure: StepFailure::from_knowledge_refusal(
                    crate::domain::workflow::KnowledgeRefusal {
                        content,
                        destination,
                    },
                ),
                safety: RetrySafety::SafeToRetry,
            })),
            Err(AppError::WorkflowGuard(violation)) => WorkflowHandlerOutcome::Completed(Err(
                crate::application::workflow::actions::guard_step_failure(
                    "local_agent",
                    &violation,
                ),
            )),
            Err(
                AppError::NotFound(_)
                | AppError::Forbidden(_)
                | AppError::BadRequest(_)
                | AppError::Conflict(_),
            ) => failure("local_agent.refused", FailureClass::Terminal).into(),
            Err(_) => failure("local_agent.storage_unavailable", FailureClass::Retryable).into(),
        }
    }
}

fn failure(code: &'static str, class: FailureClass) -> Result<Value, WorkflowFailure> {
    Err(WorkflowFailure {
        failure: StepFailure::new(
            class,
            FailureCode::parse(code).expect("static local failure"),
            None,
        )
        .expect("bounded static failure"),
        safety: RetrySafety::SafeToRetry,
    })
}
pub fn invalid() -> AppError {
    AppError::BadRequest("Invalid local-agent workflow input".into())
}
