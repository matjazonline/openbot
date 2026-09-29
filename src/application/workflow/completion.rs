//! Validated successful progression. Failure recovery and parking have separate owners.
use super::{activation::*, batch::*, lease::FencedWorkflowResult, publication::PublishedBundle};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::workflow::{ChoiceName, RouteSelection, TransitionTarget};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitDisposition {
    Committed,
    Replayed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommittedWorkflowStep {
    pub disposition: CommitDisposition,
    pub output: Value,
    pub route: RouteSelection,
    pub target: TransitionTarget,
    pub successor: Option<ActivationRequest>,
}

#[async_trait]
pub trait WorkflowCompletion: Send + Sync {
    /// Some returns a committed successful result or its exact replay. None means
    /// no successful result: either read-only fence/replay refusal, or durable
    /// validation/budget failure retirement (possibly requiring reconciliation).
    /// Storage failures propagate and roll back; callers must reread durable state
    /// to distinguish refusal from settled failure and must not redispatch on None.
    async fn complete_io(
        &self,
        result: FencedWorkflowResult,
    ) -> AppResult<Option<CommittedWorkflowStep>>;
}

pub fn is_io_kind(kind: &str) -> bool {
    matches!(
        kind,
        "context.load"
            | "memory.load"
            | "memory.save"
            | "ai.classify"
            | "agent.run"
            | "decision.agent"
            | "http.request"
            | "tool.call"
            | "mcp.call"
            | "message.send"
            | "message.reply"
    )
}

pub fn prepare_io(
    bundle: &PublishedBundle,
    activation: &ActivatedExecution,
    output: Value,
) -> AppResult<PureCompletion> {
    let step = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&activation.step)
        .ok_or_else(|| AppError::BadRequest("Unknown workflow step".into()))?;
    if !is_io_kind(step.step_type.as_str()) {
        return Err(AppError::Conflict(
            "Workflow step is not an I/O completion".into(),
        ));
    }
    let route = if step.step_type.as_str() == "decision.agent" {
        RouteSelection::Choice(
            ChoiceName::parse(
                output
                    .get("choice")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AppError::BadRequest("Missing workflow choice".into()))?,
            )
            .map_err(|_| AppError::BadRequest("Invalid workflow choice".into()))?,
        )
    } else {
        RouteSelection::Success
    };
    validate_completion(bundle, activation, output, route)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::workflow::{publication::*, registry};
    use crate::domain::workflow::{CompanyId, ExecutionId, StepId, TypeName, VersionId};
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn workflow_completion_declared_choice_and_terminal_schema() {
        let company = CompanyId::new(Uuid::new_v4());
        let mut source: Value =
            serde_json::from_str(&registry::example("decision.agent").unwrap().source).unwrap();
        source["output_schema"] = json!({"type":"object","properties":{"choice":{"const":"continue"},"data":{"type":"string"}},"required":["choice","data"]});
        let bundle = freeze(
            crate::adapters::workflow_source::decode(&source.to_string()).unwrap(),
            company,
            VersionId::new(Uuid::new_v4()),
            DependencySnapshots {
                agents: vec![AgentSnapshot {
                    company_id: company,
                    key: AgentKey::parse("reviewer").unwrap(),
                    instructions: "Review".into(),
                    model: ModelSettings {
                        provider: TypeName::parse("test").unwrap(),
                        model: "scripted".into(),
                        max_output_tokens: 100,
                    },
                    tools: vec![],
                    skills: vec![],
                }],
                ..Default::default()
            },
            vec![],
        )
        .unwrap();
        let activation = ActivatedExecution {
            execution: ExecutionId::new(Uuid::new_v4()),
            step: StepId::parse("start").unwrap(),
            ordinal: 1,
            inputs: json!({}),
            choice: None,
        };
        let saved = prepare_io(
            &bundle,
            &activation,
            json!({"choice":"continue","data":"accepted"}),
        )
        .unwrap();
        assert_eq!(
            saved.route,
            RouteSelection::Choice(ChoiceName::parse("continue").unwrap())
        );
        assert_eq!(saved.target, TransitionTarget::End);
        for output in [
            json!({"choice":"unknown","data":"x"}),
            json!({"choice":"continue","data":7}),
            json!({"choice":"revise","data":"x"}),
            json!({"data":"x"}),
        ] {
            assert!(prepare_io(&bundle, &activation, output).is_err());
        }
    }
}
