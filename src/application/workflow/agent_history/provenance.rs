use crate::app_error::{AppError, AppResult};
use crate::application::workflow::{
    activation::{input_references, step_binding_references},
    publication::PublishedBundle,
};
use crate::domain::workflow::StepId;
use std::collections::BTreeSet;

/// Callers supply only actual earlier activation IDs and the frozen program.
/// Including executed control steps prevents a private rule from hiding behind
/// a later literal input; unexecuted future review steps do not taint an earlier draft.
pub fn thread_history_input_proven(
    bundle: &PublishedBundle,
    step: &StepId,
    earlier: &[StepId],
) -> AppResult<bool> {
    let definitions = &bundle.compiled().graph().definition().steps;
    if earlier.len() > definitions.len() {
        return Err(AppError::Internal(
            "History provenance exceeded its program bound".into(),
        ));
    }
    let mut pending = earlier.to_vec();
    pending.push(step.clone());
    let mut seen = BTreeSet::new();
    while let Some(step) = pending.pop() {
        if !seen.insert(step.clone()) {
            continue;
        }
        let definition = definitions
            .get(&step)
            .ok_or_else(|| AppError::Internal("Unknown history provenance step".into()))?;
        if !thread_data_step(definition.step_type.as_str()) {
            return Ok(false);
        }
        let references = if definition.step_type.as_str() == "agent.run" {
            // The resolved AgentId is the outer stream key. Choosing its alias
            // does not make the original context private; other inputs still can.
            step_binding_references(
                bundle,
                &step,
                definition
                    .inputs
                    .iter()
                    .filter(|(name, _)| name.as_str() != "agent")
                    .map(|(_, binding)| binding),
            )
        } else {
            input_references(bundle, &step)?
        };
        for reference in references {
            match reference.root_name() {
                "params" => return Ok(false),
                "steps" => pending.push(
                    reference
                        .step_id()
                        .ok_or_else(|| AppError::Internal("Invalid history dependency".into()))?
                        .clone(),
                ),
                "input" | "run.id" | "run.parent_id" | "run.deadline" => {}
                _ => return Err(AppError::Internal("Unknown history reference".into())),
            }
        }
    }
    Ok(true)
}

fn thread_data_step(kind: &str) -> bool {
    matches!(
        kind,
        "agent.run" | "context.history" | "data.map" | "decision.rule" | "message.ask"
    )
}
