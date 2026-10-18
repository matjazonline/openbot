//! Bounded JSON canonicalization shared by action and decision identities.
use crate::app_error::{AppError, AppResult};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) fn digest(value: &Value) -> AppResult<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn invalid() -> AppError {
    AppError::BadRequest("Invalid canonical workflow value".into())
}

pub(super) fn canonical(value: &Value, bytes: usize) -> AppResult<Value> {
    let mut remaining = 4096;
    let mut remaining_bytes = bytes;
    bound(value, 0, &mut remaining, &mut remaining_bytes)?;
    let mut remaining = 4096;
    let value = sorted(value, 0, &mut remaining, 32)?;
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

fn sorted(
    value: &Value,
    depth: usize,
    remaining: &mut usize,
    max_depth: usize,
) -> AppResult<Value> {
    if depth > max_depth {
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
                result.insert(
                    key.clone(),
                    sorted(&object[key], depth + 1, remaining, max_depth)?,
                );
            }
            Ok(Value::Object(result))
        }
        Value::Array(array) => array
            .iter()
            .map(|item| sorted(item, depth + 1, remaining, max_depth))
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

/// Decision artifacts follow the workflow context bounds, rather than action argument bounds.
pub(super) fn context(value: &Value) -> AppResult<Value> {
    use crate::domain::workflow::{ContextLimits, validate_context_value};
    validate_context_value(value, ContextLimits::default()).map_err(|_| invalid())?;
    let mut remaining = ContextLimits::default().work_nodes;
    // The domain validator already enforced its depth bound before this traversal.
    sorted(value, 0, &mut remaining, usize::MAX)
}
