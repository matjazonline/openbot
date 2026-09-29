use super::{
    Binding, ChoiceName, ResourceName, StepId, TransitionTarget, TypeName, VersionId, WorkflowId,
};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowDefinition {
    pub format_version: u32,
    pub workflow_id: WorkflowId,
    pub version_id: VersionId,
    /// Descriptive until the phase 02 compiler validates schemas.
    pub input_schema: Option<Value>,
    pub parameter_schema: Option<Value>,
    pub output_schema: Option<Value>,
    /// Descriptive until resource resolution and execution authorization exist.
    /// Lifecycle admission/cancellation authorization is already enforced by
    /// the application service, which rejects nonempty unresolved requirements.
    pub resources: Vec<ResourceRequirement>,
    pub entry: StepId,
    pub steps: BTreeMap<StepId, StepDefinition>,
    pub limits: ExecutionLimits,
}

/// A named resource slot and the compatible type/contract required by a step.
/// Lookup, compatibility checks, and resource/execution authorization are
/// deferred to later phases. Lifecycle authorization does not grant this slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRequirement {
    pub slot: ResourceName,
    pub kind: TypeName,
    pub contract: Option<TypeName>,
}

pub const MAX_EXECUTION_STEPS: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionLimits {
    /// Shared across descendants; admission freezes the root's allowance.
    pub root_budget: super::RootBudgetLimits,
    /// The runtime must debit this on each step execution, including repeats.
    pub max_steps: u32,
    /// The runtime passes this bound to every context resolution.
    pub max_context_bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepDefinition {
    pub step_type: TypeName,
    pub inputs: BTreeMap<String, Binding>,
    pub routes: Routes,
    /// Used only after retry eligibility is exhausted by the runtime.
    pub final_error: Option<TransitionTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routes {
    Success(TransitionTarget),
    /// Raw ordered routes retain duplicate names for structural validation.
    Choices(Vec<(ChoiceName, TransitionTarget)>),
}
