use super::{Diagnostic, ParsedWorkflow};
use crate::application::workflow::publication::{RoleReference, RoleUseMode};
use crate::application::workflow::registry::ModelStep;
use crate::application::workflow::reviewers::{
    ReviewerSelector, approval_reviewers, literal_reviewers, parse_reviewers,
};
use crate::domain::workflow::{Binding, StepId};

pub(super) fn references(parsed: &ParsedWorkflow) -> Result<Vec<RoleReference>, Diagnostic> {
    let mut roles = Vec::new();
    for (step, definition) in &parsed.definition.steps {
        if let Some(follow) = definition.inputs.get("reviewers_follow_role")
            && (definition.step_type.as_str() != "decision.human"
                || !matches!(follow, Binding::Literal(serde_json::Value::Bool(_))))
        {
            return Err(error(
                parsed,
                step,
                "reviewers_follow_role",
                "reviewers_follow_role must be a literal boolean on decision.human",
            ));
        }
        if let Some(kind) = ModelStep::of(definition.step_type.as_str())
            && let Some(binding) = definition.inputs.get("role")
        {
            let role = match binding {
                Binding::Literal(value) => value.as_str().and_then(|value| value.parse().ok()),
                _ => None,
            }
            .ok_or_else(|| {
                error(
                    parsed,
                    step,
                    "role",
                    "Choose a literal department role: lead or reviewer",
                )
            })?;
            roles.push(RoleReference {
                step: step.clone(),
                role,
                mode: if kind.uses_capabilities() {
                    RoleUseMode::Tools
                } else {
                    RoleUseMode::ModelOnly
                },
            });
        }
        let reviewers = if definition.step_type.as_str() == "decision.human" {
            definition.inputs.get("reviewers").cloned()
        } else if let Some(approval) = definition.inputs.get("approval") {
            if default_approval_contains_role(approval) {
                return Err(error(
                    parsed,
                    step,
                    "approval",
                    "Role reviewers require an unconditional literal approval",
                ));
            }
            approval_reviewers(approval)
        } else {
            None
        };
        if let Some(reviewers) = reviewers {
            collect_people(parsed, step, &reviewers, &mut roles)?;
        }
    }
    Ok(roles)
}

fn default_approval_contains_role(binding: &Binding) -> bool {
    match binding {
        Binding::Default { fallback, .. } => {
            approval_reviewers(fallback)
                .as_ref()
                .is_some_and(contains_role)
                || default_approval_contains_role(fallback)
        }
        _ => false,
    }
}

fn collect_people(
    parsed: &ParsedWorkflow,
    step: &StepId,
    binding: &Binding,
    roles: &mut Vec<RoleReference>,
) -> Result<(), Diagnostic> {
    let Some(value) = literal_reviewers(binding) else {
        if contains_role(binding) {
            return Err(error(
                parsed,
                step,
                "reviewers",
                "Role reviewers require an unconditional literal reviewer array",
            ));
        }
        return Ok(());
    };
    let selectors = parse_reviewers(&value).map_err(|_| {
        error(
            parsed,
            step,
            "reviewers",
            "Choose person principals or literal department roles as reviewers",
        )
    })?;
    for selector in selectors {
        let ReviewerSelector::Role(role) = selector else {
            continue;
        };
        if roles
            .iter()
            .any(|reference| reference.step == *step && reference.role == role)
        {
            return Err(error(
                parsed,
                step,
                "reviewers",
                "A role cannot select both an agent and people on the same step",
            ));
        }
        roles.push(RoleReference {
            step: step.clone(),
            role,
            mode: RoleUseMode::People,
        });
    }
    Ok(())
}

fn contains_role(binding: &Binding) -> bool {
    match binding {
        Binding::Literal(value) => {
            value.as_str().is_some_and(|value| {
                value
                    .parse::<crate::entities::department::DepartmentRole>()
                    .is_ok()
            }) || value.as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item.as_str().is_some_and(|role| {
                        role.parse::<crate::entities::department::DepartmentRole>()
                            .is_ok()
                    })
                })
            })
        }
        Binding::Array(items) => items.iter().any(contains_role),
        Binding::Default { fallback, .. } => contains_role(fallback),
        _ => false,
    }
}

fn error(parsed: &ParsedWorkflow, step: &StepId, field: &str, message: &str) -> Diagnostic {
    let path = format!("/steps/{step}/with/{field}");
    Diagnostic::at(
        "registry.role",
        message,
        &path,
        super::input_span(&parsed.locations, &path),
    )
}

pub(super) fn validate_inputs(
    references: &[RoleReference],
    step: &StepId,
    inputs: &serde_json::Value,
    root: &str,
    span: super::SourceSpan,
) -> Result<(), Diagnostic> {
    let Some(reviewers) = inputs.get("reviewers").or_else(|| {
        inputs
            .get("approval")
            .and_then(|approval| approval.get("reviewers"))
    }) else {
        return Ok(());
    };
    let invalid = || {
        Diagnostic::at(
            "registry.reviewers",
            "Choose person principals; department roles must be literal in this step's source",
            &format!("{root}/reviewers"),
            span,
        )
    };
    for selected in parse_reviewers(reviewers).map_err(|_| invalid())? {
        if let ReviewerSelector::Role(role) = selected
            && !references.iter().any(|reference| {
                reference.step == *step
                    && reference.role == role
                    && reference.mode == RoleUseMode::People
            })
        {
            return Err(invalid());
        }
    }
    Ok(())
}
