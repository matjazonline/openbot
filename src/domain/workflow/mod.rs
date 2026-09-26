//! Pure workflow definitions and decisions. Validation here covers structural graph
//! integrity and bounded context construction; schema, resource authorization,
//! expression compilation, and publication checks belong to later layers.

mod causality;
mod context;
mod definition;
mod graph;
mod ids;
mod outcome;
mod state;
mod transition;

pub use causality::{
    ActionRef, CausalError, ChildCause, ExecutionRef, MessageEffectLink, RunCausality,
    StepCausality, TriggerRef, TriggerSource,
};
pub use context::{
    Binding, Context, ContextError, ContextLimits, ContextReference, RunMetadata, resolve,
};
pub use definition::{
    ExecutionLimits, ResourceRequirement, Routes, StepDefinition, WorkflowDefinition,
};
pub use graph::{GraphError, ValidatedWorkflow, validate};
pub use ids::{
    ActionInvocationId, ChoiceName, CompanyId, ExecutionId, FailureCode, NameError, ResourceName,
    RunId, ScheduleId, ScheduleOccurrenceId, StepId, TriggerId, TypeName, VersionId, WaitId,
    WorkflowId,
};
pub use outcome::{
    CompletedStep, CompletionRoute, DurableWaitRequest, EngineDisposition, FailureClass,
    OutcomeError, RetryEligibility, StepFailure, StepOutcome, resolve_outcome,
};
pub use state::{RunState, RunStateError, WaitingReason};
pub use transition::{RouteError, RouteSelection, TransitionTarget, select_route};
