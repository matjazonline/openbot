use super::{
    ChoiceName, ContextError, ContextLimits, FailureCode, RouteError, RouteSelection, StepId,
    TransitionTarget, ValidatedWorkflow, WaitId, WaitingReason, select_route,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use thiserror::Error;

const MAX_DIAGNOSTIC_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionRoute {
    Success,
    Choice(ChoiceName),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompletedStep {
    output: Value,
    route: CompletionRoute,
}

impl CompletedStep {
    pub fn new(
        output: Value,
        route: CompletionRoute,
        limits: ContextLimits,
    ) -> Result<Self, OutcomeError> {
        super::context::validate_output(&output, limits)?;
        Ok(Self { output, route })
    }

    pub fn output(&self) -> &Value {
        &self.output
    }

    pub fn route(&self) -> &CompletionRoute {
        &self.route
    }
}

/// Requests durable registration of a logical wake for the current execution.
/// The stable ID must be scoped by the caller's company and run; it grants no
/// authorization and must never be used for an unscoped lookup. Constructing
/// this value does not persist the wait or arrange its notification.
/// The reason is classified here; its associated durable record, run-deadline
/// cap, recovery, and deadline policy belong to later phases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableWaitRequest {
    wait_id: WaitId,
    reason: WaitingReason,
    deadline: DateTime<Utc>,
}

impl DurableWaitRequest {
    pub fn new(
        wait_id: WaitId,
        reason: WaitingReason,
        deadline: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Self, OutcomeError> {
        if deadline <= now {
            return Err(OutcomeError::ExpiredDeadline);
        }
        Ok(Self {
            wait_id,
            reason,
            deadline,
        })
    }

    pub fn wait_id(&self) -> WaitId {
        self.wait_id
    }

    pub fn reason(&self) -> WaitingReason {
        self.reason
    }

    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    Retryable,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepFailure {
    class: FailureClass,
    code: FailureCode,
    /// Internal diagnostic; callers must decide separately whether it is safe to log.
    diagnostic: Option<String>,
}

impl StepFailure {
    pub fn new(
        class: FailureClass,
        code: FailureCode,
        diagnostic: Option<String>,
    ) -> Result<Self, OutcomeError> {
        if diagnostic
            .as_ref()
            .is_some_and(|text| text.len() > MAX_DIAGNOSTIC_BYTES)
        {
            return Err(OutcomeError::DiagnosticTooLong);
        }
        Ok(Self {
            class,
            code,
            diagnostic,
        })
    }

    pub fn class(&self) -> FailureClass {
        self.class
    }

    pub fn code(&self) -> &FailureCode {
        &self.code
    }

    pub fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepOutcome {
    Completed(Box<CompletedStep>),
    Waiting(DurableWaitRequest),
    Failed(StepFailure),
}

/// Retry eligibility is decided by the engine from its durable attempt budget.
/// A step handler reports failure class but cannot grant itself another attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryEligibility {
    Available,
    Exhausted,
}

/// Pure transition decision; no execution result, wait, or successor is stored
/// or scheduled by constructing this value.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineDisposition<'a> {
    Advance {
        completed: &'a CompletedStep,
        target: &'a TransitionTarget,
    },
    Park {
        request: &'a DurableWaitRequest,
    },
    Retry {
        failure: &'a StepFailure,
    },
    /// The current execution failed. A declared error route can continue the
    /// workflow, so `Some(target)` does not mean the run itself has failed.
    Terminal {
        failure: &'a StepFailure,
        target: Option<&'a TransitionTarget>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OutcomeError {
    #[error(transparent)]
    Context(#[from] ContextError),
    #[error(transparent)]
    Route(#[from] RouteError),
    #[error("wait deadline must be in the future")]
    ExpiredDeadline,
    #[error("failure diagnostic exceeds {MAX_DIAGNOSTIC_BYTES} bytes")]
    DiagnosticTooLong,
}

/// Resolve a checked handler outcome using the engine's retry eligibility.
/// The caller must atomically commit the execution result, run progress, audit,
/// and either wait plus notification or the selected successor. `Park` is only
/// a request for registration, not evidence that a wait has been committed.
/// Later runtime work must enforce the run-deadline cap and persist/recover the
/// associated record for the already classified waiting reason.
pub fn resolve_outcome<'a>(
    workflow: &'a ValidatedWorkflow,
    step: &StepId,
    outcome: &'a StepOutcome,
    retry: RetryEligibility,
    now: DateTime<Utc>,
) -> Result<EngineDisposition<'a>, OutcomeError> {
    if !workflow.definition().steps.contains_key(step) {
        return Err(RouteError::UnknownStep(step.clone()).into());
    }
    match outcome {
        StepOutcome::Completed(completed) => {
            super::context::validate_output(completed.output(), workflow.context_limits())?;
            let selection = match completed.route() {
                CompletionRoute::Success => RouteSelection::Success,
                CompletionRoute::Choice(choice) => RouteSelection::Choice(choice.clone()),
            };
            let target = select_route(workflow, step, &selection)?;
            Ok(EngineDisposition::Advance { completed, target })
        }
        StepOutcome::Waiting(request) => {
            if request.deadline() <= now {
                return Err(OutcomeError::ExpiredDeadline);
            }
            Ok(EngineDisposition::Park { request })
        }
        StepOutcome::Failed(failure)
            if failure.class() == FailureClass::Retryable
                && retry == RetryEligibility::Available =>
        {
            Ok(EngineDisposition::Retry { failure })
        }
        StepOutcome::Failed(failure) => {
            let target = match select_route(workflow, step, &RouteSelection::FinalError) {
                Ok(target) => Some(target),
                Err(RouteError::MissingFinalError(_)) => None,
                Err(error) => return Err(error.into()),
            };
            Ok(EngineDisposition::Terminal { failure, target })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::workflow::{
        Binding, Context, ContextReference, ExecutionLimits, Routes, RunId, RunMetadata,
        StepDefinition, TypeName, VersionId, WorkflowDefinition, WorkflowId, resolve, validate,
    };
    use chrono::Duration;
    use serde_json::json;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn id(name: &str) -> StepId {
        StepId::parse(name).unwrap()
    }

    fn graph() -> ValidatedWorkflow {
        let mut branch = StepDefinition {
            step_type: TypeName::parse("agent.run").unwrap(),
            inputs: BTreeMap::new(),
            routes: Routes::Choices(vec![
                (
                    ChoiceName::parse("accepted").unwrap(),
                    TransitionTarget::Step(id("b")),
                ),
                (
                    ChoiceName::parse("rejected").unwrap(),
                    TransitionTarget::End,
                ),
            ]),
            final_error: Some(TransitionTarget::End),
        };
        let success = StepDefinition {
            routes: Routes::Success(TransitionTarget::End),
            final_error: None,
            ..branch.clone()
        };
        branch.final_error = Some(TransitionTarget::End);
        validate(WorkflowDefinition {
            format_version: 1,
            workflow_id: WorkflowId::new(Uuid::nil()),
            version_id: VersionId::new(Uuid::nil()),
            input_schema: None,
            parameter_schema: None,
            output_schema: None,
            resources: vec![],
            entry: id("a"),
            steps: [(id("a"), branch), (id("b"), success)].into(),
            limits: ExecutionLimits {
                max_steps: 10,
                max_context_bytes: 1024,
            },
        })
        .unwrap()
    }

    fn completed(value: Value, route: CompletionRoute) -> StepOutcome {
        StepOutcome::Completed(Box::new(
            CompletedStep::new(value, route, ContextLimits::default()).unwrap(),
        ))
    }

    fn failure(class: FailureClass) -> StepOutcome {
        StepOutcome::Failed(
            StepFailure::new(
                class,
                FailureCode::parse("provider.timeout").unwrap(),
                Some("internal".into()),
            )
            .unwrap(),
        )
    }

    #[test]
    fn completion_preserves_output_and_routes_only_declared_success_or_choice() {
        let graph = graph();
        let now = Utc::now();
        let outcome = completed(
            json!({"decision": "accepted"}),
            CompletionRoute::Choice(ChoiceName::parse("accepted").unwrap()),
        );
        assert!(
            matches!(resolve_outcome(&graph, &id("a"), &outcome, RetryEligibility::Exhausted, now).unwrap(),
            EngineDisposition::Advance { completed, target: TransitionTarget::Step(next) }
            if next == &id("b") && completed.output() == &json!({"decision": "accepted"}))
        );
        let rejected = completed(
            json!({"decision": "rejected"}),
            CompletionRoute::Choice(ChoiceName::parse("rejected").unwrap()),
        );
        assert!(matches!(
            resolve_outcome(
                &graph,
                &id("a"),
                &rejected,
                RetryEligibility::Available,
                now
            )
            .unwrap(),
            EngineDisposition::Advance {
                target: TransitionTarget::End,
                ..
            }
        ));
        let success = completed(json!(true), CompletionRoute::Success);
        assert!(matches!(
            resolve_outcome(&graph, &id("b"), &success, RetryEligibility::Available, now).unwrap(),
            EngineDisposition::Advance {
                target: TransitionTarget::End,
                ..
            }
        ));
        assert!(matches!(
            resolve_outcome(&graph, &id("a"), &success, RetryEligibility::Available, now),
            Err(OutcomeError::Route(RouteError::WrongRoute { .. }))
        ));
        let missing = completed(
            json!(null),
            CompletionRoute::Choice(ChoiceName::parse("missing").unwrap()),
        );
        assert!(matches!(
            resolve_outcome(&graph, &id("a"), &missing, RetryEligibility::Available, now),
            Err(OutcomeError::Route(RouteError::UnknownChoice { .. }))
        ));
    }

    #[test]
    fn wait_requires_future_deadline_and_has_no_successor() {
        let graph = graph();
        let now = Utc::now();
        let wait_id = WaitId::new(Uuid::new_v4());
        for deadline in [now - Duration::seconds(1), now] {
            assert_eq!(
                DurableWaitRequest::new(wait_id, WaitingReason::Timer, deadline, now),
                Err(OutcomeError::ExpiredDeadline)
            );
        }
        let deadline = now + Duration::seconds(1);
        let wait = StepOutcome::Waiting(
            DurableWaitRequest::new(wait_id, WaitingReason::Timer, deadline, now).unwrap(),
        );
        assert!(
            matches!(resolve_outcome(&graph, &id("a"), &wait, RetryEligibility::Available, now).unwrap(),
            EngineDisposition::Park { request } if request.wait_id() == wait_id && request.deadline() == deadline)
        );
        assert_eq!(
            resolve_outcome(
                &graph,
                &id("a"),
                &wait,
                RetryEligibility::Available,
                deadline
            ),
            Err(OutcomeError::ExpiredDeadline)
        );
    }

    #[test]
    fn retry_policy_is_engine_owned_and_final_error_is_optional() {
        let graph = graph();
        let now = Utc::now();
        let retryable = failure(FailureClass::Retryable);
        assert!(
            matches!(resolve_outcome(&graph, &id("a"), &retryable, RetryEligibility::Available, now).unwrap(),
            EngineDisposition::Retry { failure } if failure.code().as_str() == "provider.timeout")
        );
        for outcome in [&retryable, &failure(FailureClass::Terminal)] {
            assert!(matches!(
                resolve_outcome(&graph, &id("a"), outcome, RetryEligibility::Exhausted, now)
                    .unwrap(),
                EngineDisposition::Terminal {
                    target: Some(TransitionTarget::End),
                    ..
                }
            ));
            assert!(matches!(
                resolve_outcome(&graph, &id("b"), outcome, RetryEligibility::Exhausted, now)
                    .unwrap(),
                EngineDisposition::Terminal { target: None, .. }
            ));
        }
        let terminal = failure(FailureClass::Terminal);
        assert!(matches!(
            resolve_outcome(
                &graph,
                &id("a"),
                &terminal,
                RetryEligibility::Available,
                now
            )
            .unwrap(),
            EngineDisposition::Terminal {
                target: Some(TransitionTarget::End),
                ..
            }
        ));
        assert!(matches!(
            resolve_outcome(
                &graph,
                &id("b"),
                &terminal,
                RetryEligibility::Available,
                now
            )
            .unwrap(),
            EngineDisposition::Terminal { target: None, .. }
        ));
    }

    #[test]
    fn unknown_step_is_rejected_for_every_outcome() {
        let graph = graph();
        let now = Utc::now();
        let wait = StepOutcome::Waiting(
            DurableWaitRequest::new(
                WaitId::new(Uuid::nil()),
                WaitingReason::Timer,
                now + Duration::seconds(1),
                now,
            )
            .unwrap(),
        );
        for outcome in [
            completed(json!(1), CompletionRoute::Success),
            wait,
            failure(FailureClass::Retryable),
        ] {
            assert!(matches!(
                resolve_outcome(
                    &graph,
                    &id("missing"),
                    &outcome,
                    RetryEligibility::Available,
                    now
                ),
                Err(OutcomeError::Route(RouteError::UnknownStep(_)))
            ));
        }
    }

    #[test]
    fn completed_output_is_bounded_before_clone_and_at_resolution() {
        let exact = json!("abcd"); // Six serialized bytes.
        assert!(
            CompletedStep::new(
                exact.clone(),
                CompletionRoute::Success,
                ContextLimits {
                    output_bytes: 6,
                    work_nodes: 1
                }
            )
            .is_ok()
        );
        assert!(matches!(
            CompletedStep::new(
                exact,
                CompletionRoute::Success,
                ContextLimits {
                    output_bytes: 5,
                    work_nodes: 1
                }
            ),
            Err(OutcomeError::Context(ContextError::Limit("output bytes")))
        ));
        let array = json!([1, 2]);
        assert!(
            CompletedStep::new(
                array.clone(),
                CompletionRoute::Success,
                ContextLimits {
                    output_bytes: 64,
                    work_nodes: 3
                }
            )
            .is_ok()
        );
        assert!(matches!(
            CompletedStep::new(
                array,
                CompletionRoute::Success,
                ContextLimits {
                    output_bytes: 64,
                    work_nodes: 2
                }
            ),
            Err(OutcomeError::Context(ContextError::Limit("work nodes")))
        ));
        let mut deep = json!(null);
        for _ in 0..64 {
            deep = json!([deep]);
        }
        assert!(
            CompletedStep::new(
                deep.clone(),
                CompletionRoute::Success,
                ContextLimits::default()
            )
            .is_ok()
        );
        deep = json!([deep]);
        assert!(matches!(
            CompletedStep::new(deep, CompletionRoute::Success, ContextLimits::default()),
            Err(OutcomeError::Context(ContextError::Limit("JSON depth")))
        ));
        let graph = graph();
        let large = completed(json!("x".repeat(1024)), CompletionRoute::Success);
        assert!(matches!(
            resolve_outcome(
                &graph,
                &id("b"),
                &large,
                RetryEligibility::Available,
                Utc::now()
            ),
            Err(OutcomeError::Context(ContextError::Limit("output bytes")))
        ));
    }

    #[test]
    fn failure_identity_and_diagnostic_are_bounded() {
        for bad in ["", "bad code", "é", &"x".repeat(129)] {
            assert!(FailureCode::parse(bad).is_err());
        }
        assert!(FailureCode::parse("x".repeat(128)).is_ok());
        let code = FailureCode::parse("provider.timeout").unwrap();
        assert!(
            StepFailure::new(FailureClass::Terminal, code.clone(), Some("x".repeat(4096))).is_ok()
        );
        assert_eq!(
            StepFailure::new(FailureClass::Terminal, code, Some("x".repeat(4097))),
            Err(OutcomeError::DiagnosticTooLong)
        );
    }

    #[test]
    fn routing_does_not_mutate_previous_committed_context() {
        let graph = graph();
        let prior = json!({"receipt": 7});
        let outputs = BTreeMap::from([(id("earlier"), prior.clone())]);
        let input = json!({});
        let context = Context {
            input: &input,
            params: &input,
            step_outputs: &outputs,
            run: RunMetadata {
                run_id: RunId::new(Uuid::nil()),
                parent_run_id: None,
            },
        };
        let outcome = completed(
            json!({"decision": "rejected"}),
            CompletionRoute::Choice(ChoiceName::parse("rejected").unwrap()),
        );
        let _ = resolve_outcome(
            &graph,
            &id("a"),
            &outcome,
            RetryEligibility::Available,
            Utc::now(),
        )
        .unwrap();
        let read = resolve(
            &Binding::Reference(ContextReference::parse("/steps/earlier/output/receipt").unwrap()),
            &context,
            graph.context_limits(),
        )
        .unwrap();
        assert_eq!(read, json!(7));
        assert_eq!(outputs[&id("earlier")], prior);
    }
}
