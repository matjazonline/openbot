//! Frozen agent capabilities over the same escalation commands as the authenticated API.
use super::{EscalationChoice, EscalationDecisionId};
use crate::app_error::{AppError, AppResult};
use crate::application::agent::{
    ActionEffect, ActionRecovery, ApprovedActionPolicy, ToolCallOrigin,
};
use crate::application::workflow::compiler::{Schema, SourceSpan};
use crate::application::workflow::{
    actions::*,
    agent::AgentToolObservation,
    lease::{LeasePolicy, WorkflowFence},
    publication::ToolSnapshot,
    registry::ToolContract,
    tools::tool_error,
};
use crate::domain::workflow::{CompanyId, TypeName};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const RESOURCE_KIND: &str = "escalation";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationTool {
    Read,
    Respond,
}

impl EscalationTool {
    pub const ALL: [Self; 2] = [Self::Read, Self::Respond];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "read_escalation",
            Self::Respond => "respond_to_escalation",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }
    pub const fn description(self) -> &'static str {
        match self {
            Self::Read => {
                "Read an escalation decision and the current account's allowed actions. A successful handling run does not acknowledge an escalation."
            }
            Self::Respond => {
                "Explicitly acknowledge an escalation, reassign it to a principal, or cancel its source run. Reassignment requires principal; cancellation requires expected_run_revision and actual run-control authority. Assignment never grants authority."
            }
        }
    }
}

pub fn schemas(tool: EscalationTool) -> (Value, Value) {
    let mut input = json!({"type":"object","properties":{"decision_id":{"type":"string","format":"uuid"}},"required":["decision_id"],"additionalProperties":false});
    if tool == EscalationTool::Respond {
        input["properties"]["choice"] = json!({"type":"object","properties":{
            "kind":{"type":"string","enum":["acknowledge","reassign","cancel"]},
            "principal":{"type":"string","format":"uuid"},
            "expected_run_revision":{"type":"integer","minimum":0,"maximum":9223372036854775807u64}
        },"required":["kind"],"additionalProperties":false});
        input["required"] = json!(["decision_id", "choice"]);
    }
    let output = json!({"type":"object","properties":{
        "ok":{"type":"boolean"},"decision":{"type":"object","additionalProperties":true},
        "replayed":{"type":"boolean"},"error":{"type":"object","additionalProperties":true}
    },"required":["ok"],"additionalProperties":false});
    (input, output)
}

pub fn snapshot(company: CompanyId, tool: EscalationTool) -> AppResult<ToolSnapshot> {
    let name = TypeName::parse(tool.name()).map_err(|_| invalid())?;
    let (input_schema, output_schema) = schemas(tool);
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
            effect: match tool {
                EscalationTool::Read => ActionEffect::Read,
                EscalationTool::Respond => ActionEffect::Write,
            },
            recovery: ActionRecovery::Reconcile,
        },
    })
}

pub fn trusted(value: &ToolSnapshot) -> AppResult<bool> {
    let Some(tool) = EscalationTool::parse(value.contract.name.as_str()) else {
        return Ok(false);
    };
    Ok(serde_json::to_value(value).map_err(|_| invalid())?
        == serde_json::to_value(snapshot(value.company_id, tool)?).map_err(|_| invalid())?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    pub decision_id: EscalationDecisionId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RespondInput {
    pub decision_id: EscalationDecisionId,
    pub choice: EscalationChoice,
}

#[derive(Clone)]
pub struct EscalationToolCall {
    pub origin: ToolCallOrigin,
    pub kind: EscalationTool,
    pub arguments: Value,
    pub call: ModelToolCallId,
}

#[async_trait]
pub trait EscalationPreparation: Send + Sync {
    async fn escalation_tool_request(
        &self,
        scope: ActionScope,
        call: &EscalationToolCall,
    ) -> AppResult<ActionRequest>;
}

pub struct EscalationActions<P, D> {
    persistence: P,
    dispatch: ActionService<D>,
    lease: LeasePolicy,
}
impl<P, D> EscalationActions<P, D> {
    pub fn new(persistence: P, dispatch: D, lease: LeasePolicy) -> Self {
        Self {
            persistence,
            dispatch: ActionService::new(dispatch),
            lease,
        }
    }
}
impl<P, D> EscalationActions<P, D>
where
    P: EscalationPreparation + ActionIntents + Clone,
    D: ActionDispatch,
{
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: EscalationToolCall,
    ) -> AppResult<AgentToolObservation> {
        // Conditional choice fields are checked by the same typed command decoder as the API.
        let valid = match call.kind {
            EscalationTool::Read => {
                serde_json::from_value::<ReadInput>(call.arguments.clone()).is_ok()
            }
            EscalationTool::Respond => {
                serde_json::from_value::<RespondInput>(call.arguments.clone()).is_ok()
            }
        };
        let span = SourceSpan {
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        };
        let schema =
            Schema::compile(schemas(call.kind).0, "/input", span).map_err(|_| invalid())?;
        if !valid || schema.validate(&call.arguments, "/input", span).is_err() {
            return Ok(AgentToolObservation::Completed(refusal(
                "workflow.invalid_input",
                "Invalid escalation command",
            )));
        }
        let request = self
            .persistence
            .escalation_tool_request(
                crate::application::workflow::message::tool_scope(fence),
                &call,
            )
            .await?;
        let intent = ActionService::new(self.persistence.clone())
            .prepare_tool(request, call.call)
            .await?;
        let dispatched = self
            .dispatch
            .dispatch_local(&ActionDispatchRequest {
                subject: intent.approval_subject(),
                fence,
                lease: self.lease,
            })
            .await?;
        match dispatched {
            LocalDispatchResult::Committed(receipt) => {
                Ok(AgentToolObservation::Completed(receipt.result))
            }
            LocalDispatchResult::ApprovalParked(wait) => Ok(AgentToolObservation::Parked(wait)),
            LocalDispatchResult::ApprovalRequired => Err(AppError::Internal(
                "Escalation builtin has an unexpected approval policy".into(),
            )),
            LocalDispatchResult::PossibleDispatchExists => Ok(AgentToolObservation::Uncertain),
        }
    }
}

pub fn refusal(code: &str, message: &str) -> Value {
    // The shared tool error envelope is the canonical model-facing refusal.
    let error = tool_error(code, message);
    json!({"ok":false,"error":error})
}
fn invalid() -> AppError {
    AppError::Internal("Invalid trusted escalation capability".into())
}

#[cfg(test)]
#[path = "escalation_tool_tests.rs"]
mod tests;
