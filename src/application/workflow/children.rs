//! Runtime child calls and bounded repetition (07-D1). The parent parks on a durable
//! `child_run` wait while its child runs; deciding the next round is pure.
use super::{
    activation::{ActivatedExecution, ActivationRequest},
    batch::{PureCompletion, validate_completion},
    compiler::RepeatBindings,
    publication::PublishedBundle,
    waits::ParkedWorkflow,
};
use crate::application::app_error::AppResult;
use crate::domain::workflow::{ChildOrdinal, Context, ContextLimits, RouteSelection, resolve};
use async_trait::async_trait;
use serde_json::{Value, json};

#[async_trait]
pub trait WorkflowChildren: Send + Sync {
    /// One transaction (07-D1 §D1.2): activate the step, create the pinned child run and the
    /// parent's `child_run` wait together, and park the parent without holding a worker. A
    /// replay returns the existing wait. `None`: not runnable, or a deterministic failure was
    /// settled in the same transaction.
    async fn park_child(&self, scope: ActivationRequest) -> AppResult<Option<ParkedWorkflow>>;
}

/// A `flow.repeat` step's output after a round: the child's result and the rounds so far. It is
/// also what the step's own `repeat` bindings read at `/steps/<self>/output`.
pub fn provisional_output(result: Value, rounds: ChildOrdinal) -> Value {
    json!({"result": result, "rounds": rounds.get()})
}

#[derive(Debug, Clone, PartialEq)]
pub enum RoundDecision {
    /// `exit_when` held: leave through `done`, also on the last permitted round.
    Done,
    /// The bound was reached without an exit: leave through `exhausted`, never as approval.
    Exhausted,
    /// Start the next round with this child input.
    Next(Value),
}

/// A deferred binding could not decide the round; the step fails with Terminal
/// `workflow.invalid_input` and takes its declared error route (07-D1 §D1.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidRound;

/// The repeat bindings and bound of one step, with the pinned child whose input they produce.
pub struct RepeatRule<'a> {
    pub bindings: &'a RepeatBindings,
    pub max_iterations: u32,
    pub child: &'a PublishedBundle,
}

/// Exit is evaluated before the bound, so an acceptance in the last round is `Done`.
/// `context` must already hold the provisional output at `/steps/<self>/output`.
pub fn decide_round(
    rule: &RepeatRule<'_>,
    context: &Context<'_>,
    limits: ContextLimits,
    round: ChildOrdinal,
) -> Result<RoundDecision, InvalidRound> {
    match resolve(&rule.bindings.exit_when, context, limits) {
        Ok(Value::Bool(true)) => return Ok(RoundDecision::Done),
        Ok(Value::Bool(false)) => {}
        _ => return Err(InvalidRound),
    }
    if round.get() >= rule.max_iterations {
        return Ok(RoundDecision::Exhausted);
    }
    let input = resolve(&rule.bindings.next_input, context, limits).map_err(|_| InvalidRound)?;
    rule.child
        .compiled()
        .validate_input(&input)
        .map_err(|_| InvalidRound)?;
    Ok(RoundDecision::Next(input))
}

/// The parent step's completion from its child's output (`workflow.call`) or its final
/// provisional output and `done`/`exhausted` choice (`flow.repeat`), checked like any step's.
pub fn child_completion(
    bundle: &PublishedBundle,
    activation: &ActivatedExecution,
    output: Value,
    route: RouteSelection,
) -> AppResult<PureCompletion> {
    validate_completion(bundle, activation, output, route)
}
