//! `goal.status` (Plan08 03D.4, gate 08G-P03-D2 r2 D2.8): a read-only built-in that returns the
//! rollup of the goal the calling run serves. It takes no arguments, so a run cannot ask about
//! another goal; the calling agent must hold `lead` or `sponsor` on that goal, checked at call
//! time.
use crate::app_error::{AppError, AppResult};
use crate::application::agent::{
    ActionEffect, ActionRecovery, AgentToolOrigin, ApprovedActionPolicy,
};
use crate::application::workflow::{
    agent::AgentToolObservation, lease::WorkflowFence, publication::ToolKind,
    publication::ToolSnapshot, registry::ToolContract,
};
use crate::domain::workflow::{CompanyId, TypeName};
use serde_json::{Value, json};

pub const GOAL_STATUS: &str = "goal.status";
/// The run serves no goal, or its goal is no longer one.
pub const GOAL_NONE: &str = "goal.none";
/// The calling agent holds neither `lead` nor `sponsor` on the run's goal.
pub const GOAL_NOT_AUTHORIZED: &str = "goal.not_authorized";

pub const DESCRIPTION: &str = "Read the status of the goal this run serves: its runs by state and department, decisions waiting on people, open escalations, how desk calls ended, spend against its total cap, and times. Counts and names only. Takes no arguments. Only the goal's lead or a sponsor may read it.";

pub fn is_builtin_goal_status_tool(name: &str) -> bool {
    name == GOAL_STATUS
}

fn invalid() -> AppError {
    AppError::Internal("Invalid goal.status contract".into())
}

pub fn goal_status_schemas() -> (Value, Value) {
    let input = json!({"type":"object","properties":{},"additionalProperties":false});
    let output = json!({"type":"object","properties":{
        "status":{"type":"object","additionalProperties":true},
        "error":{"type":"object","additionalProperties":true}
    },"additionalProperties":false});
    (input, output)
}

pub fn builtin_goal_status_snapshot(company: CompanyId) -> AppResult<ToolSnapshot> {
    let name = TypeName::parse(GOAL_STATUS).map_err(|_| invalid())?;
    let (input_schema, output_schema) = goal_status_schemas();
    Ok(ToolSnapshot {
        company_id: company,
        workflow: None,
        contract: ToolContract {
            name: name.clone(),
            connection: None,
            input_schema,
            output_schema,
        },
        policy: ApprovedActionPolicy {
            capability: name,
            policy_revision: 1,
            effect: ActionEffect::Read,
            recovery: ActionRecovery::Reconcile,
        },
    })
}

/// Whether a snapshot is exactly the trusted built-in.
pub fn is_trusted_goal_status_snapshot(snapshot: &ToolSnapshot) -> AppResult<bool> {
    if snapshot.kind() != ToolKind::GoalStatus {
        return Ok(false);
    }
    let trusted = builtin_goal_status_snapshot(snapshot.company_id)?;
    Ok(serde_json::to_value(snapshot).map_err(|_| invalid())?
        == serde_json::to_value(trusted).map_err(|_| invalid())?)
}

/// One `goal.status` call by an agent.
#[derive(Debug, Clone)]
pub struct GoalStatusToolCall {
    pub origin: AgentToolOrigin,
    pub arguments: Value,
}

/// Reads the calling run's goal status under its fence.
#[async_trait::async_trait]
pub trait GoalStatusDispatch: Send + Sync {
    async fn goal_status(
        &self,
        fence: WorkflowFence,
        call: GoalStatusToolCall,
    ) -> AppResult<AgentToolObservation>;
}
