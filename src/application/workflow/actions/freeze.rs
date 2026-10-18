use super::contracts::*;
use super::service::{ActionRequest, FrozenAction};
use crate::application::app_error::AppResult;
pub(super) use crate::application::workflow::canonical::canonical;
use crate::application::workflow::compiler::{Schema, SourceSpan};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) const MAX_ARGUMENT_BYTES: usize = 65_536;
pub(super) const MAX_OPERATION_BYTES: usize = 262_144;

pub(super) fn freeze(mut request: ActionRequest) -> AppResult<FrozenAction> {
    validate_scope(&mut request)?;
    canonical(&request.arguments, MAX_ARGUMENT_BYTES)?;
    canonical(&request.contract.contract.input_schema, MAX_ARGUMENT_BYTES)?;
    canonical(&request.contract.contract.output_schema, MAX_ARGUMENT_BYTES)?;
    canonical(
        &serde_json::to_value(&request).map_err(|_| invalid())?,
        MAX_OPERATION_BYTES,
    )?;
    let arguments = canonical(&request.arguments, MAX_ARGUMENT_BYTES)?;
    validate_arguments(&request, &arguments)?;
    request.arguments = arguments;
    let argument_digest = digest(&request.arguments)?;
    // Bind effect identity to the actual operation/target and approved contract, never attempts
    // or use-time policy lookups. Context changes conflict with the existing frozen intent.
    let operation = json!({"scope":request.scope,"key":request.operation_key,
        "target":request.target,"contract":request.contract,"arguments":request.arguments});
    let operation = canonical(&operation, MAX_OPERATION_BYTES)?;
    let operation_digest = digest(&operation)?;
    let saved = canonical(
        &serde_json::to_value(&request).map_err(|_| invalid())?,
        MAX_OPERATION_BYTES,
    )?;
    let decision = if request.context.approval_required {
        ActionPolicyDecision::ApprovalRequired
    } else {
        ActionPolicyDecision::Unevaluated
    };
    Ok(FrozenAction {
        request,
        saved,
        argument_digest,
        operation_digest,
        decision,
    })
}

pub(super) fn restore(saved: Value) -> AppResult<FrozenAction> {
    canonical(&saved, MAX_OPERATION_BYTES)?;
    freeze(serde_json::from_value(saved).map_err(|_| invalid())?)
}

fn validate_scope(request: &mut ActionRequest) -> AppResult<()> {
    if request.scope.company != request.contract.company_id
        || request.contract.policy.policy_revision == 0
        || request.context.actor.is_nil()
        || request.context.capability_ceiling.len() > 256
        || request.scope.company.as_uuid().is_nil()
        || request.scope.run.as_uuid().is_nil()
        || request.scope.execution.as_uuid().is_nil()
    {
        return Err(invalid());
    }
    let resource = match request.target {
        ActionTarget::Connection { resource, .. } | ActionTarget::Local { resource, .. } => {
            resource
        }
    };
    if resource.is_nil() {
        return Err(invalid());
    }
    let mut ceiling = BTreeSet::new();
    if request
        .context
        .capability_ceiling
        .iter()
        .any(|item| !ceiling.insert(item.clone()))
    {
        return Err(invalid());
    }
    request.context.capability_ceiling.sort();
    Ok(())
}

fn validate_arguments(request: &ActionRequest, arguments: &Value) -> AppResult<()> {
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
    let schema = Schema::compile(
        request.contract.contract.input_schema.clone(),
        "/arguments",
        span,
    )
    .map_err(|_| invalid())?;
    schema
        .validate(arguments, "/arguments", span)
        .map_err(|_| invalid())?;
    Schema::compile(
        request.contract.contract.output_schema.clone(),
        "/output",
        span,
    )
    .map_err(|_| invalid())?;
    Ok(())
}

pub(super) fn digest(value: &Value) -> AppResult<ArgumentDigest> {
    crate::application::workflow::canonical::digest(value).map(ArgumentDigest)
}
