//! Read and retarget model-step references through the same bounded, source-preserving decoder.
use super::{Diagnostic, SourceDecoder, parse_workflow, retarget_agent};
use crate::application::{agent::AgentKey, workflow::registry::ModelStep};
use crate::domain::workflow::{Binding, StepId, VersionId};

#[derive(Debug, Clone)]
pub struct AgentReferenceStep {
    pub id: StepId,
    pub model_step: ModelStep,
    pub key: Option<AgentKey>,
}

pub fn agent_reference_steps(
    decoder: &impl SourceDecoder,
    source: &str,
) -> Result<Vec<AgentReferenceStep>, Diagnostic> {
    let decoded = decoder.decode(source)?;
    let parsed = parse_workflow(&decoded, VersionId::new(uuid::Uuid::nil()))?;
    Ok(parsed
        .definition
        .steps
        .iter()
        .filter_map(|(id, step)| {
            let model_step = ModelStep::of(step.step_type.as_str())?;
            let key = match step.inputs.get("agent") {
                Some(Binding::Literal(value)) => value.as_str().and_then(AgentKey::parse),
                _ => None,
            };
            Some(AgentReferenceStep {
                id: id.clone(),
                model_step,
                key,
            })
        })
        .collect())
}

pub fn select_agent_reference(
    decoder: &impl SourceDecoder,
    source: &str,
    step: &StepId,
    agent: &AgentKey,
) -> Result<String, Diagnostic> {
    let decoded = decoder.decode(source)?;
    let parsed = parse_workflow(&decoded, VersionId::new(uuid::Uuid::nil()))?;
    if parsed
        .definition
        .steps
        .get(step)
        .is_none_or(|definition| ModelStep::of(definition.step_type.as_str()).is_none())
    {
        return Err(Diagnostic::at(
            "workflow.agent_reference",
            "Select a model step in the current source",
            &format!("/steps/{}", step.as_str()),
            parsed.locations[""],
        ));
    }
    retarget_agent(decoder, source, step.as_str(), agent.as_str())
}
