//! Structured edits keep the unsaved source outside the selected inputs unchanged.
use super::{Diagnostic, SourceDecoder, SourceSpan, parse_workflow};
use crate::application::agent::AgentKey;
use crate::application::workflow::registry::ModelStep;
use crate::application::workflow::reviewers::{
    ReviewerSelector, approval_reviewers, literal_reviewers, parse_reviewers,
};
use crate::domain::workflow::{StepId, VersionId};
use crate::entities::department::DepartmentRole;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct ReviewerReferenceStep {
    pub id: StepId,
    pub selectors: Option<Vec<ReviewerSelector>>,
}

pub fn reviewer_reference_steps(
    decoder: &impl SourceDecoder,
    source: &str,
) -> Result<Vec<ReviewerReferenceStep>, Diagnostic> {
    let parsed = parse_workflow(&decoder.decode(source)?, VersionId::new(uuid::Uuid::nil()))?;
    Ok(parsed
        .definition
        .steps
        .iter()
        .filter_map(|(id, step)| {
            let reviewers = if step.step_type.as_str() == "decision.human" {
                step.inputs.get("reviewers").cloned()
            } else {
                step.inputs.get("approval").and_then(approval_reviewers)
            }?;
            let selectors =
                literal_reviewers(&reviewers).and_then(|value| parse_reviewers(&value).ok());
            Some(ReviewerReferenceStep {
                id: id.clone(),
                selectors,
            })
        })
        .collect())
}

pub fn select_role_reference(
    decoder: &impl SourceDecoder,
    source: &str,
    step: &StepId,
    role: DepartmentRole,
) -> Result<String, Diagnostic> {
    model_selector(
        decoder,
        source,
        step,
        "role",
        json!({"literal":role.as_str()}),
    )
}

pub(super) fn select_pinned_reference(
    decoder: &impl SourceDecoder,
    source: &str,
    step: &StepId,
    agent: &AgentKey,
) -> Result<String, Diagnostic> {
    model_selector(
        decoder,
        source,
        step,
        "agent",
        json!({"literal":agent.as_str()}),
    )
}

fn model_selector(
    decoder: &impl SourceDecoder,
    source: &str,
    step: &StepId,
    field: &str,
    selection: Value,
) -> Result<String, Diagnostic> {
    edit_inputs(decoder, source, step, |inputs, kind, span| {
        if ModelStep::of(kind).is_none() {
            return Err(error("Select a model step", span));
        }
        inputs.remove("agent");
        inputs.remove("role");
        inputs.insert(field.into(), selection);
        Ok(())
    })
}

pub fn select_reviewer_references(
    decoder: &impl SourceDecoder,
    source: &str,
    step: &StepId,
    reviewers: &[ReviewerSelector],
) -> Result<String, Diagnostic> {
    let values: Vec<_> = reviewers
        .iter()
        .map(|reviewer| match reviewer {
            ReviewerSelector::Principal(id) => json!({"kind":"principal","id":id.as_uuid()}),
            ReviewerSelector::Role(role) => json!(role.as_str()),
        })
        .collect();
    parse_reviewers(&json!(values)).map_err(|_| {
        error(
            "Choose a bounded reviewer selection",
            SourceSpan {
                start: 0,
                end: 0,
                line: 1,
                column: 1,
            },
        )
    })?;
    edit_inputs(decoder, source, step, |inputs, kind, span| {
        if kind == "decision.human" {
            inputs.insert("reviewers".into(), json!({"literal":values}));
        } else {
            let approval = inputs
                .get_mut("approval")
                .ok_or_else(|| error("This step has no approval reviewers", span))?;
            if let Some(fields) = approval.get_mut("literal").and_then(Value::as_object_mut) {
                fields.insert("reviewers".into(), json!(values));
            } else if let Some(fields) = approval.get_mut("object").and_then(Value::as_object_mut) {
                fields.insert("reviewers".into(), json!({"literal":values}));
            } else {
                return Err(error("Edit parameter-controlled approval in source", span));
            }
        }
        Ok(())
    })
}

fn edit_inputs(
    decoder: &impl SourceDecoder,
    source: &str,
    step: &StepId,
    edit: impl FnOnce(&mut serde_json::Map<String, Value>, &str, SourceSpan) -> Result<(), Diagnostic>,
) -> Result<String, Diagnostic> {
    let decoded = decoder.decode(source)?;
    let parsed = parse_workflow(&decoded, VersionId::new(uuid::Uuid::nil()))?;
    let path = format!("/steps/{step}/with");
    let span = super::selection_span::inputs_span(&decoded.root, step.as_str(), source)?;
    let definition = parsed
        .definition
        .steps
        .get(step)
        .ok_or_else(|| error("Unknown step", span))?;
    let mut root = super::syntax::json_value(&decoded.root, "")?;
    let inputs = root
        .pointer_mut(&path)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| error("Invalid step inputs", span))?;
    edit(inputs, definition.step_type.as_str(), span)?;
    let mut replacement =
        serde_json::to_string(inputs).map_err(|_| error("Cannot encode step inputs", span))?;
    if source[span.start..span.end].ends_with('\n') {
        replacement.push('\n');
    }
    let size = source.len() - (span.end - span.start) + replacement.len();
    if size > super::MAX_SOURCE_BYTES {
        return Err(error("Source exceeds 256 KiB", span));
    }
    let result = format!(
        "{}{}{}",
        &source[..span.start],
        replacement,
        &source[span.end..]
    );
    let checked = decoder.decode(&result)?;
    parse_workflow(&checked, VersionId::new(uuid::Uuid::nil()))?;
    if super::syntax::json_value(&checked.root, "")? != root {
        return Err(error("Selection changed unrelated source values", span));
    }
    Ok(result)
}

fn error(message: &str, span: SourceSpan) -> Diagnostic {
    Diagnostic::at("workflow.selection", message, "/steps", span)
}
