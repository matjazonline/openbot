//! Pure handlers and bounded advancement. This port never grants I/O ownership.
use super::{
    activation::ActivatedExecution, activation::ActivationRequest, publication::PublishedBundle,
};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::workflow::{
    CompletedStep, CompletionRoute, EngineDisposition, RouteSelection, RunState, TransitionTarget,
    select_route, validate_context_value,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct BatchBudget {
    steps: u8,
    work: Duration,
}
impl BatchBudget {
    pub fn new(steps: u8, work: Duration) -> AppResult<Self> {
        if steps == 0
            || steps > 64
            || work < Duration::from_millis(1)
            || work > Duration::from_secs(5)
        {
            return Err(AppError::BadRequest("Invalid workflow batch budget".into()));
        }
        Ok(Self { steps, work })
    }
    pub fn steps(self) -> u8 {
        self.steps
    }
    pub fn work(self) -> Duration {
        self.work
    }
    /// One second reserved for finishing a step and committing. The entire
    /// transaction, including lock waits, is cancelled at this hard ceiling.
    pub fn transaction_timeout(self) -> Duration {
        self.work + Duration::from_secs(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchDisposition {
    Completed,
    Yielded,
    Boundary,
    Replay,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchResult {
    pub disposition: BatchDisposition,
    pub completed: u8,
    pub last: ActivationRequest,
    pub continuation: Option<ActivationRequest>,
}

pub struct PureCompletion {
    pub output: Value,
    pub route: RouteSelection,
    pub target: TransitionTarget,
    pub state: RunState,
}

pub fn is_pure(
    bundle: &PublishedBundle,
    step: &crate::domain::workflow::StepId,
) -> AppResult<bool> {
    let definition = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(step)
        .ok_or_else(|| AppError::Database("Unknown workflow step".into()))?;
    Ok(matches!(
        definition.step_type.as_str(),
        "data.map" | "decision.rule"
    ))
}

pub fn execute_pure(
    bundle: &PublishedBundle,
    activation: &ActivatedExecution,
) -> AppResult<Option<PureCompletion>> {
    let compiled = bundle.compiled();
    let definition = compiled
        .graph()
        .definition()
        .steps
        .get(&activation.step)
        .ok_or_else(invalid)?;
    let (output, route) = match definition.step_type.as_str() {
        "data.map" => (
            activation.inputs.get("value").ok_or_else(invalid)?.clone(),
            RouteSelection::Success,
        ),
        "decision.rule" => {
            let choice = activation.choice.clone().ok_or_else(invalid)?;
            let data = activation.inputs.get("data").ok_or_else(invalid)?;
            (
                json!({"choice":choice.as_str(), "data":data}),
                RouteSelection::Choice(choice),
            )
        }
        _ => return Ok(None),
    };
    validate_context_value(&output, compiled.graph().context_limits()).map_err(|_| invalid())?;
    compiled
        .validate_step_output(&activation.step, &output)
        .map_err(|e| AppError::BadRequest(e.message.into()))?;
    let target = select_route(compiled.graph(), &activation.step, &route)
        .map_err(|_| invalid())?
        .clone();
    if target == TransitionTarget::End {
        compiled
            .validate_output(&output)
            .map_err(|e| AppError::BadRequest(e.message.into()))?;
    }
    let completed = CompletedStep::new(
        output.clone(),
        match &route {
            RouteSelection::Success => CompletionRoute::Success,
            RouteSelection::Choice(choice) => CompletionRoute::Choice(choice.clone()),
            RouteSelection::FinalError => return Err(invalid()),
        },
        compiled.graph().context_limits(),
    )
    .map_err(|_| invalid())?;
    let state = RunState::Running
        .apply(
            &EngineDisposition::Advance {
                completed: &completed,
                target: &target,
            },
            chrono::Utc::now(),
        )
        .map_err(|_| invalid())?;
    Ok(Some(PureCompletion {
        output,
        route,
        target,
        state,
    }))
}
fn invalid() -> AppError {
    AppError::Database("Invalid pure workflow activation".into())
}

#[async_trait]
pub trait WorkflowBatch: Send + Sync {
    /// Run-first atomic advancement of pending, due, unleased workflow jobs.
    /// Completed jobs replay their saved progression; they never advance again.
    async fn advance_pure(
        &self,
        request: ActivationRequest,
        budget: BatchBudget,
    ) -> AppResult<BatchResult>;
}
