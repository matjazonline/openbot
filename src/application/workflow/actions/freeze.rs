use super::contracts::*;
use super::service::{ActionRequest, FrozenAction};
use crate::application::app_error::AppResult;
use crate::application::workflow::compiler::{Schema, SourceSpan};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    Ok(ArgumentDigest(format!("{:x}", Sha256::digest(bytes))))
}

pub(super) fn canonical(value: &Value, bytes: usize) -> AppResult<Value> {
    let mut remaining = 4096;
    let mut remaining_bytes = bytes;
    bound(value, 0, &mut remaining, &mut remaining_bytes)?;
    let mut remaining = 4096;
    let value = sorted(value, 0, &mut remaining)?;
    if serde_json::to_vec(&value).map_err(|_| invalid())?.len() > bytes {
        return Err(invalid());
    }
    Ok(value)
}

// Bound allocations before cloning/serializing caller-controlled values.
fn bound(value: &Value, depth: usize, items: &mut usize, bytes: &mut usize) -> AppResult<()> {
    if depth > 32 {
        return Err(invalid());
    }
    *items = items.checked_sub(1).ok_or_else(invalid)?;
    *bytes = bytes.checked_sub(2).ok_or_else(invalid)?;
    match value {
        Value::String(text) => *bytes = bytes.checked_sub(text.len()).ok_or_else(invalid)?,
        Value::Object(object) => {
            for (key, item) in object {
                *bytes = bytes.checked_sub(key.len() + 3).ok_or_else(invalid)?;
                bound(item, depth + 1, items, bytes)?;
            }
        }
        Value::Array(array) => {
            for item in array {
                bound(item, depth + 1, items, bytes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn sorted(value: &Value, depth: usize, remaining: &mut usize) -> AppResult<Value> {
    if depth > 32 {
        return Err(invalid());
    }
    *remaining = remaining.checked_sub(1).ok_or_else(invalid)?;
    match value {
        Value::Number(number) => Ok(Value::Number(canonical_number(number)?)),
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            let mut result = serde_json::Map::new();
            for key in keys {
                result.insert(key.clone(), sorted(&object[key], depth + 1, remaining)?);
            }
            Ok(Value::Object(result))
        }
        Value::Array(array) => array
            .iter()
            .map(|item| sorted(item, depth + 1, remaining))
            .collect(),
        _ => Ok(value.clone()),
    }
}

fn canonical_number(number: &serde_json::Number) -> AppResult<serde_json::Number> {
    if !number.is_f64() {
        return Ok(number.clone());
    }
    let value = number.as_f64().ok_or_else(invalid)?;
    // JSONB erases signed zero and exponent/integral spelling. Normalize before hashing,
    // including schema numbers, so decoding the committed operation preserves identity.
    if value.fract() == 0.0 {
        if value >= i64::MIN as f64 && value < i64::MAX as f64 {
            return Ok(serde_json::Number::from(value as i64));
        }
        if value >= 0.0 && value < u64::MAX as f64 {
            return Ok(serde_json::Number::from(value as u64));
        }
    }
    serde_json::Number::from_f64(value).ok_or_else(invalid)
}
