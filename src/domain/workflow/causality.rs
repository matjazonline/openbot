//! Immediate, company-scoped causal references. These describe provenance, not authority.
//! Admission must check them against authoritative stored records before committing a run.

use super::{
    ActionInvocationId, CompanyId, ExecutionId, RunId, ScheduleId, ScheduleOccurrenceId, StepId,
    ToolCallId, TriggerId,
};
use crate::domain::entities::{correlation::CorrelationId, message::CanonicalMessageId};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CausalError {
    #[error("causal references belong to different companies")]
    CompanyMismatch,
    #[error("causal references belong to different runs or executions")]
    ExecutionMismatch,
    #[error("a run cannot be its own parent")]
    SelfParent,
}

/// An execution instance identifies repeated visits to the same graph step separately.
/// Durable activation ordinals increase across the entire run, not separately per
/// step. A retry retains the execution ID and ordinal while changing its attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRef {
    company_id: CompanyId,
    run_id: RunId,
    execution_id: ExecutionId,
    step_id: StepId,
}

impl ExecutionRef {
    pub fn new(
        company_id: CompanyId,
        run_id: RunId,
        execution_id: ExecutionId,
        step_id: StepId,
    ) -> Self {
        Self {
            company_id,
            run_id,
            execution_id,
            step_id,
        }
    }
    pub fn company_id(&self) -> CompanyId {
        self.company_id
    }
    pub fn run_id(&self) -> RunId {
        self.run_id
    }
    pub fn execution_id(&self) -> ExecutionId {
        self.execution_id
    }
    pub fn step_id(&self) -> &StepId {
        &self.step_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRef {
    execution: ExecutionRef,
    action_id: ActionInvocationId,
}

impl ActionRef {
    pub fn new(execution: ExecutionRef, action_id: ActionInvocationId) -> Self {
        Self {
            execution,
            action_id,
        }
    }
    pub fn execution(&self) -> &ExecutionRef {
        &self.execution
    }
    pub fn action_id(&self) -> ActionInvocationId {
        self.action_id
    }
}

/// The round of one child step execution: `workflow.call` is round 1, and each `flow.repeat`
/// round is the next. Bounded by `MAX_ROOT_REPETITIONS`, the most rounds a root budget grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChildOrdinal(u16);

impl ChildOrdinal {
    pub const FIRST: Self = Self(1);
    pub fn new(value: u32) -> Option<Self> {
        (1..=super::budget::MAX_ROOT_REPETITIONS)
            .contains(&value)
            .then_some(Self(value as u16))
    }
    pub fn get(self) -> u32 {
        u32::from(self.0)
    }
    pub fn next(self) -> Option<Self> {
        Self::new(self.get() + 1)
    }
}

/// The runtime child of one parent step execution, created by that step (07-D1 §D1.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildStepRef {
    execution: ExecutionRef,
    ordinal: ChildOrdinal,
}

impl ChildStepRef {
    pub fn new(execution: ExecutionRef, ordinal: ChildOrdinal) -> Self {
        Self { execution, ordinal }
    }
    pub fn execution(&self) -> &ExecutionRef {
        &self.execution
    }
    pub fn ordinal(&self) -> ChildOrdinal {
        self.ordinal
    }
}

/// The runtime child of one saved model tool call of an agent execution: a workflow tool call
/// (07-D2 §D2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildToolCallRef {
    execution: ExecutionRef,
    call: ToolCallId,
}

impl ChildToolCallRef {
    pub fn new(execution: ExecutionRef, call: ToolCallId) -> Self {
        Self { execution, call }
    }
    pub fn execution(&self) -> &ExecutionRef {
        &self.execution
    }
    pub fn call(&self) -> ToolCallId {
        self.call
    }
}

/// A child has one immediate parent cause, which embeds its execution. Children are created
/// only by their parent's runtime transition, never through admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildCause {
    Step(ChildStepRef),
    ToolCall(ChildToolCallRef),
}

impl ChildCause {
    pub fn execution(&self) -> &ExecutionRef {
        match self {
            Self::Step(reference) => reference.execution(),
            Self::ToolCall(reference) => reference.execution(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerSource {
    Message {
        message_id: CanonicalMessageId,
    },
    Schedule {
        schedule_id: ScheduleId,
        occurrence_id: ScheduleOccurrenceId,
    },
    Manual,
    Child {
        parent: ChildCause,
    },
}

/// Trigger identity and source are stable across retries; correlation is deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerRef {
    company_id: CompanyId,
    trigger_id: TriggerId,
    source: TriggerSource,
}

impl TriggerRef {
    pub fn new(
        company_id: CompanyId,
        trigger_id: TriggerId,
        source: TriggerSource,
    ) -> Result<Self, CausalError> {
        if let TriggerSource::Child { parent } = &source
            && parent.execution().company_id() != company_id
        {
            return Err(CausalError::CompanyMismatch);
        }
        Ok(Self {
            company_id,
            trigger_id,
            source,
        })
    }
    pub fn company_id(&self) -> CompanyId {
        self.company_id
    }
    pub fn trigger_id(&self) -> TriggerId {
        self.trigger_id
    }
    pub fn source(&self) -> &TriggerSource {
        &self.source
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCausality {
    run_id: RunId,
    trigger: TriggerRef,
    correlation_id: CorrelationId,
}

impl RunCausality {
    pub fn new(
        run_id: RunId,
        trigger: TriggerRef,
        correlation_id: CorrelationId,
    ) -> Result<Self, CausalError> {
        if let TriggerSource::Child { parent } = trigger.source()
            && parent.execution().run_id() == run_id
        {
            return Err(CausalError::SelfParent);
        }
        Ok(Self {
            run_id,
            trigger,
            correlation_id,
        })
    }
    pub fn company_id(&self) -> CompanyId {
        self.trigger.company_id()
    }
    pub fn run_id(&self) -> RunId {
        self.run_id
    }
    pub fn trigger(&self) -> &TriggerRef {
        &self.trigger
    }
    /// Trace only: never use for authorization, deduplication, or fencing.
    pub fn correlation_id(&self) -> CorrelationId {
        self.correlation_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepCausality {
    execution: ExecutionRef,
}

impl StepCausality {
    pub fn new(run: &RunCausality, execution: ExecutionRef) -> Result<Self, CausalError> {
        if execution.company_id() != run.company_id() {
            return Err(CausalError::CompanyMismatch);
        }
        if execution.run_id() != run.run_id() {
            return Err(CausalError::ExecutionMismatch);
        }
        Ok(Self { execution })
    }
    pub fn execution(&self) -> &ExecutionRef {
        &self.execution
    }
}

/// The canonical message is caused by exactly one action invocation. Phase 04
/// persistence must commit this link with the action receipt and message, not
/// infer it later from a shared correlation ID or nearby timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEffectLink {
    message_id: CanonicalMessageId,
    action: ActionRef,
}

impl MessageEffectLink {
    pub fn new(message_id: CanonicalMessageId, action: ActionRef) -> Self {
        Self { message_id, action }
    }
    pub fn message_id(&self) -> CanonicalMessageId {
        self.message_id
    }
    pub fn action(&self) -> &ActionRef {
        &self.action
    }
    pub fn company_id(&self) -> CompanyId {
        self.action.execution().company_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn checked_links_keep_immediate_identity_and_repeated_executions_distinct() {
        let company = CompanyId::new(Uuid::new_v4());
        let run_id = RunId::new(Uuid::new_v4());
        let step = StepId::parse("repeat").unwrap();
        let first = ExecutionRef::new(
            company,
            run_id,
            ExecutionId::new(Uuid::new_v4()),
            step.clone(),
        );
        let second = ExecutionRef::new(company, run_id, ExecutionId::new(Uuid::new_v4()), step);
        assert_ne!(first, second);
        let action = ActionRef::new(first.clone(), ActionInvocationId::new(Uuid::new_v4()));
        let call = ChildToolCallRef::new(first.clone(), ToolCallId::new(Uuid::new_v4()));
        let trigger = TriggerRef::new(
            company,
            TriggerId::new(Uuid::new_v4()),
            TriggerSource::Child {
                parent: ChildCause::ToolCall(call.clone()),
            },
        )
        .unwrap();
        assert_eq!(
            RunCausality::new(run_id, trigger.clone(), CorrelationId::new()),
            Err(CausalError::SelfParent)
        );
        let child =
            RunCausality::new(RunId::new(Uuid::new_v4()), trigger, CorrelationId::new()).unwrap();
        assert_eq!(
            child.trigger().source(),
            &TriggerSource::Child {
                parent: ChildCause::ToolCall(call)
            }
        );
        assert_eq!(
            StepCausality::new(&child, second),
            Err(CausalError::ExecutionMismatch)
        );
        assert_eq!(
            MessageEffectLink::new(CanonicalMessageId::random(), action.clone()).action(),
            &action
        );
        let foreign = ExecutionRef::new(
            CompanyId::new(Uuid::new_v4()),
            run_id,
            ExecutionId::new(Uuid::new_v4()),
            StepId::parse("x").unwrap(),
        );
        assert_eq!(
            TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Child {
                    parent: ChildCause::Step(ChildStepRef::new(foreign, ChildOrdinal::FIRST))
                }
            ),
            Err(CausalError::CompanyMismatch)
        );
    }

    #[test]
    fn child_ordinals_are_bounded_by_root_repetitions() {
        assert_eq!(ChildOrdinal::new(0), None);
        assert_eq!(ChildOrdinal::new(1), Some(ChildOrdinal::FIRST));
        let last = ChildOrdinal::new(super::super::budget::MAX_ROOT_REPETITIONS).unwrap();
        assert_eq!(last.get(), 1000);
        assert_eq!(last.next(), None);
        assert_eq!(ChildOrdinal::FIRST.next().unwrap().get(), 2);
        assert_eq!(ChildOrdinal::new(1001), None);
    }
}
