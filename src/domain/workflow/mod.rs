//! Pure workflow definitions and decisions. Validation here covers structural graph
//! integrity and bounded context construction; schema, resource authorization,
//! expression compilation, and publication checks belong to later layers.

mod causality;
mod context;
mod definition;
mod expression;
mod graph;
mod ids;
mod outcome;
mod rule;
mod state;
mod transition;

pub use causality::{
    ActionRef, CausalError, ChildCause, ExecutionRef, MessageEffectLink, RunCausality,
    StepCausality, TriggerRef, TriggerSource,
};
pub use context::{
    Binding, Comparison, Context, ContextError, ContextLimits, ContextReference, RunMetadata,
    canonical_array_index, preflight_binding, resolve, resolve_inputs, validate_context_value,
};
pub use definition::{
    ExecutionLimits, ResourceRequirement, Routes, StepDefinition, WorkflowDefinition,
};
pub use graph::{GraphError, ValidatedWorkflow, validate};
pub use ids::{
    ActionInvocationId, BindingRevision, BindingStateRevision, ChoiceName, CompanyId,
    DraftRevision, ExecutionId, FailureCode, NameError, ResourceName, RunId, RuntimeResourceId,
    ScheduleId, ScheduleOccurrenceId, StepId, TemplateId, TemplateRevision, TriggerId, TypeName,
    VersionId, WaitId, WorkflowBindingId, WorkflowId,
};
pub use outcome::{
    CompletedStep, CompletionRoute, DurableWaitRequest, EngineDisposition, FailureClass,
    OutcomeError, RetryEligibility, StepFailure, StepOutcome, resolve_outcome,
};
pub use rule::{OrderedRule, RuleCase, RuleError};
pub use state::{RunState, RunStateError, WaitingReason};
pub use transition::{RouteError, RouteSelection, TransitionTarget, select_route};
