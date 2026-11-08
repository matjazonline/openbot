//! Freeze logical activation inputs once. This is not a worker ownership grant.
use super::{CompanyId, publication::PublishedBundle};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::workflow::{
    Binding, ChoiceName, Context, ContextReference, ExecutionId, RunId, StepId,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowJobId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationRequest {
    pub company: CompanyId,
    pub run: RunId,
    pub execution: ExecutionId,
    pub job: WorkflowJobId,
}

/// `ordinal` is run-wide, increasing across every step (including repeats).
/// Attempts reuse this ordinal and execution ID; successors receive a new one.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivatedExecution {
    pub execution: ExecutionId,
    pub step: StepId,
    pub ordinal: u64,
    pub inputs: Value,
    pub choice: Option<ChoiceName>,
}

/// Include optional/defaulted references too; missing committed values retain
/// their normal binding semantics when the compiler resolves the inputs.
pub fn input_dependencies(bundle: &PublishedBundle, step: &StepId) -> AppResult<BTreeSet<StepId>> {
    Ok(input_references(bundle, step)?
        .into_iter()
        .filter_map(|reference| reference.step_id().cloned())
        .collect())
}

/// The same bounded compiled expression walk serves activation and audience provenance.
pub fn input_references(
    bundle: &PublishedBundle,
    step: &StepId,
) -> AppResult<Vec<ContextReference>> {
    let definition = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(step)
        .ok_or_else(|| AppError::Database("Unknown stored workflow step".into()))?;
    Ok(step_binding_references(
        bundle,
        step,
        definition.inputs.values(),
    ))
}

pub(crate) fn step_binding_references<'a>(
    bundle: &'a PublishedBundle,
    step: &StepId,
    bindings: impl IntoIterator<Item = &'a Binding>,
) -> Vec<ContextReference> {
    let mut pending: Vec<_> = bindings.into_iter().collect();
    if let Some(rule) = bundle.compiled().rule(step) {
        pending.extend(rule.cases.iter().map(|case| &case.when));
    }
    if let Some(repeat) = bundle
        .compiled()
        .child_control(step)
        .and_then(|control| control.repeat.as_ref())
    {
        pending.extend([&repeat.next_input, &repeat.exit_when]);
    }
    binding_references(pending)
}

pub fn binding_references<'a>(
    bindings: impl IntoIterator<Item = &'a Binding>,
) -> Vec<ContextReference> {
    let mut pending: Vec<_> = bindings.into_iter().collect();
    let mut references = Vec::new();
    while let Some(binding) = pending.pop() {
        let reference = match binding {
            Binding::Reference(reference) | Binding::Exists(reference) => Some(reference),
            Binding::Default {
                reference,
                fallback,
            } => {
                pending.push(fallback);
                Some(reference)
            }
            Binding::Object(fields) => {
                pending.extend(fields.values());
                None
            }
            Binding::Array(items)
            | Binding::Concat(items)
            | Binding::And(items)
            | Binding::Or(items) => {
                pending.extend(items);
                None
            }
            Binding::Compare { left, right, .. } => {
                pending.extend([left.as_ref(), right.as_ref()]);
                None
            }
            Binding::In { value, items } => {
                pending.extend([value.as_ref(), items.as_ref()]);
                None
            }
            Binding::Not(inner) => {
                pending.push(inner);
                None
            }
            Binding::Literal(_) => None,
        };
        if let Some(reference) = reference {
            references.push(reference.clone());
        }
    }
    references
}

/// Pure preparation uses only a frozen bundle and committed run context.
pub fn prepare_inputs(
    bundle: &PublishedBundle,
    step: &StepId,
    context: &Context<'_>,
) -> AppResult<Value> {
    bundle
        .prepare_step_inputs(step, context)
        .map(|prepared| prepared.value().clone())
        .map_err(|diagnostic| AppError::BadRequest(diagnostic.message.into()))
}

/// Shared pure preparation for durable activation and isolated samples.
pub struct PreparedActivation {
    pub inputs: Value,
    pub choice: Option<ChoiceName>,
}

pub fn prepare_activation(
    bundle: &PublishedBundle,
    step: &StepId,
    context: &Context<'_>,
) -> AppResult<PreparedActivation> {
    let inputs = prepare_inputs(bundle, step, context)?;
    let choice = bundle
        .compiled()
        .rule(step)
        .map(|rule| rule.decide(context, bundle.compiled().graph().context_limits()))
        .transpose()
        .map_err(|_| AppError::BadRequest("Invalid rule predicate".into()))?;
    Ok(PreparedActivation { inputs, choice })
}

#[async_trait]
pub trait WorkflowActivation: Send + Sync {
    /// Atomically resolve/store once, or return the original snapshot. Lock run
    /// before execution. IDs must agree with the stored ID-only job. No I/O,
    /// claim, attempt creation, or permission to perform effects is implied.
    async fn activate(&self, request: ActivationRequest) -> AppResult<ActivatedExecution>;
}
