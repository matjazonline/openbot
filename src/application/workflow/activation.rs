//! Freeze logical activation inputs once. This is not a worker ownership grant.
use super::{CompanyId, publication::PublishedBundle};
use crate::application::app_error::{AppError, AppResult};
use crate::domain::workflow::{Binding, Context, ExecutionId, RunId, StepId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowJobId(pub Uuid);

#[derive(Debug, Clone, Copy)]
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
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobWire {
    version: u8,
    execution_id: Uuid,
}

pub fn job_payload(execution: ExecutionId) -> Value {
    serde_json::json!({"version": 1, "execution_id": execution.as_uuid()})
}

pub fn decode_job_payload(value: Value) -> AppResult<ExecutionId> {
    let wire: JobWire = serde_json::from_value(value).map_err(|_| invalid_job())?;
    if wire.version != 1 {
        return Err(invalid_job());
    }
    Ok(ExecutionId::new(wire.execution_id))
}

fn invalid_job() -> AppError {
    AppError::Database("Invalid stored workflow job payload".into())
}

/// Include optional/defaulted references too; missing committed values retain
/// their normal binding semantics when the compiler resolves the inputs.
pub fn input_dependencies(bundle: &PublishedBundle, step: &StepId) -> AppResult<BTreeSet<StepId>> {
    let definition = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(step)
        .ok_or_else(|| AppError::Database("Unknown stored workflow step".into()))?;
    let mut pending: Vec<_> = definition.inputs.values().collect();
    let mut references = BTreeSet::new();
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
        if let Some(step) = reference.and_then(|reference| reference.step_id()) {
            references.insert(step.clone());
        }
    }
    Ok(references)
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

#[async_trait]
pub trait WorkflowActivation: Send + Sync {
    /// Atomically resolve/store once, or return the original snapshot. Lock run
    /// before execution. IDs must agree with the stored ID-only job. No I/O,
    /// claim, attempt creation, or permission to perform effects is implied.
    async fn activate(&self, request: ActivationRequest) -> AppResult<ActivatedExecution>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_activation_payload_is_strict_and_id_only() {
        let id = ExecutionId::new(Uuid::new_v4());
        assert_eq!(decode_job_payload(job_payload(id)).unwrap(), id);
        for value in [
            serde_json::json!({"version":2,"execution_id":id.as_uuid()}),
            serde_json::json!({"version":1,"execution_id":id.as_uuid(),"context":{}}),
            serde_json::json!({"execution_id":id.as_uuid()}),
            serde_json::json!({"version":1,"execution_id":"bad"}),
        ] {
            assert!(decode_job_payload(value).is_err());
        }
    }
}
