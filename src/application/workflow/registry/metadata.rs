use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    Pure,
    BoundedRead,
    ModelInvocation,
    AgentActions,
    DurableWrite,
    SharedAction,
    DurableSuspension,
    ChildControl,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryMode {
    Recompute,
    ReuseCommittedResult,
    LogicalIdempotencyReceiptReconciliation,
    ResumeDurableIdentity,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRequirement {
    ContextRead,
    MemoryRead,
    MemoryWrite,
    ModelInvoke,
    AgentInvoke,
    ResourceUse,
    SharedAction,
    MessageSend,
    HumanReview,
    EventWait,
    TimerWait,
    ChildInvoke,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSelection {
    AgentDefaults,
    ExplicitSelection,
    NotApplicable,
}
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionConstraints {
    pub effect: EffectClass,
    pub recovery: RecoveryMode,
    pub capabilities: Vec<CapabilityRequirement>,
    pub may_suspend: bool,
    pub shared_action_required: bool,
    pub sequential_repeat: bool,
    pub inherited_run_budgets: bool,
    pub frozen_inputs_required: bool,
    pub runtime_deadline_required: bool,
    pub profile_selection: ProfileSelection,
}

pub(super) fn constraints(name: &str, profile_selection: ProfileSelection) -> ExecutionConstraints {
    use CapabilityRequirement::*;
    use EffectClass::*;
    let (effect, capabilities) = match name {
        "data.map" | "decision.rule" => (Pure, vec![]),
        "context.load" => (BoundedRead, vec![ContextRead]),
        "memory.load" => (BoundedRead, vec![MemoryRead]),
        "memory.save" => (DurableWrite, vec![MemoryWrite]),
        "ai.classify" | "decision.agent" => (ModelInvocation, vec![ModelInvoke]),
        "agent.run" => (AgentActions, vec![AgentInvoke]),
        "http.request" | "mcp.call" => (
            EffectClass::SharedAction,
            vec![ResourceUse, CapabilityRequirement::SharedAction],
        ),
        "tool.call" => (
            EffectClass::SharedAction,
            vec![CapabilityRequirement::SharedAction],
        ),
        "message.send" | "message.reply" => (
            EffectClass::SharedAction,
            vec![MessageSend, CapabilityRequirement::SharedAction],
        ),
        "decision.human" => (DurableSuspension, vec![HumanReview]),
        "wait.event" => (DurableSuspension, vec![EventWait]),
        "wait.timer" => (DurableSuspension, vec![TimerWait]),
        "workflow.call" | "flow.repeat" => (ChildControl, vec![ChildInvoke]),
        _ => unreachable!("checked registration"),
    };
    let recovery = match effect {
        Pure => RecoveryMode::Recompute,
        BoundedRead | ModelInvocation => RecoveryMode::ReuseCommittedResult,
        AgentActions | DurableWrite | EffectClass::SharedAction => {
            RecoveryMode::LogicalIdempotencyReceiptReconciliation
        }
        DurableSuspension | ChildControl => RecoveryMode::ResumeDurableIdentity,
    };
    ExecutionConstraints {
        effect,
        recovery,
        capabilities,
        may_suspend: matches!(
            effect,
            DurableSuspension | ChildControl | AgentActions | EffectClass::SharedAction
        ),
        shared_action_required: matches!(effect, EffectClass::SharedAction | AgentActions),
        sequential_repeat: name == "flow.repeat",
        inherited_run_budgets: true,
        frozen_inputs_required: true,
        runtime_deadline_required: effect != Pure,
        profile_selection,
    }
}
