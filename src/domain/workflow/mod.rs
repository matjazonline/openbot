//! Pure workflow definitions and decisions. Validation here covers structural graph
//! integrity and bounded context construction; schema, resource authorization,
//! expression compilation, and publication checks belong to later layers.

mod budget;
mod causality;
mod context;
mod definition;
mod evidence;
mod expression;
mod graph;
mod ids;
mod outcome;
mod recovery;
mod rule;
mod state;
mod transition;

pub use budget::{BudgetCharge, BudgetLimitError, BudgetResource, RootBudgetLimits};
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
pub use evidence::*;
pub use graph::{GraphError, ValidatedWorkflow, validate};
pub use ids::{
    ActionEvidenceId, ActionInvocationId, ActionReconciliationCommandId, ActionRemoteAttemptId,
    ActionRemoteEntryId, ActionRemoteMarkerId, BindingRevision, BindingStateRevision, ChoiceName,
    CompanyId, DraftRevision, ExecutionId, FailureCode, NameError, ResourceName, RunId,
    RuntimeResourceId, ScheduleId, ScheduleOccurrenceId, StepId, TemplateId, TemplateRevision,
    TriggerId, TypeName, VersionId, WaitId, WorkflowBindingId, WorkflowId,
};
pub use outcome::{
    CompletedStep, CompletionRoute, DurableWaitRequest, EngineDisposition, FailureClass,
    OutcomeError, RetryEligibility, StepFailure, StepOutcome, resolve_outcome,
};
pub use recovery::{
    AttemptBudget, RecoveryDecision, RecoveryLimit, RecoveryPolicy, RecoveryPolicyError,
    RecoverySnapshot, RetrySafety,
};
pub use rule::{OrderedRule, RuleCase, RuleError};
pub use state::{RunState, RunStateError, WaitingReason};
pub use transition::{RouteError, RouteSelection, TransitionTarget, select_route};
