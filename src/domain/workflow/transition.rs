use super::{ChoiceName, Routes, StepId, ValidatedWorkflow};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionTarget {
    Step(StepId),
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteSelection {
    Success,
    Choice(ChoiceName),
    FinalError,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RouteError {
    #[error("unknown step {0}")]
    UnknownStep(StepId),
    #[error("step {step} does not declare a {route} route")]
    WrongRoute { step: StepId, route: &'static str },
    #[error("step {step} has no choice {choice}")]
    UnknownChoice { step: StepId, choice: ChoiceName },
    #[error("step {0} has no final-error route")]
    MissingFinalError(StepId),
}

pub fn select_route<'a>(
    workflow: &'a ValidatedWorkflow,
    step: &StepId,
    selection: &RouteSelection,
) -> Result<&'a TransitionTarget, RouteError> {
    let definition = workflow
        .definition()
        .steps
        .get(step)
        .ok_or_else(|| RouteError::UnknownStep(step.clone()))?;
    match (selection, &definition.routes) {
        (RouteSelection::Success, Routes::Success(target)) => Ok(target),
        (RouteSelection::Choice(choice), Routes::Choices(routes)) => routes
            .iter()
            .find(|(name, _)| name == choice)
            .map(|(_, target)| target)
            .ok_or_else(|| RouteError::UnknownChoice {
                step: step.clone(),
                choice: choice.clone(),
            }),
        (RouteSelection::FinalError, _) => definition
            .final_error
            .as_ref()
            .ok_or_else(|| RouteError::MissingFinalError(step.clone())),
        (RouteSelection::Success, _) => Err(RouteError::WrongRoute {
            step: step.clone(),
            route: "success",
        }),
        (RouteSelection::Choice(_), _) => Err(RouteError::WrongRoute {
            step: step.clone(),
            route: "choice",
        }),
    }
}
