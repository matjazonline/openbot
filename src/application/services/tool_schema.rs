//! Compile bounded, self-contained schemas with no network or filesystem reference retrieval.
use crate::app_error::{AppError, AppResult};
use serde_json::Value;

pub(crate) fn compile(schema: &Value) -> AppResult<jsonschema::Validator> {
    check_resources(schema)?;
    jsonschema::options()
        .with_retriever(NoExternalReferences)
        .with_pattern_options(jsonschema::PatternOptions::regex().size_limit(65_536))
        .build(schema)
        .map_err(|_| invalid())
}

pub(crate) fn check_resources(schema: &Value) -> AppResult<()> {
    if schema.to_string().len() > 65_536 {
        return Err(invalid());
    }
    let mut remaining = 4096;
    visit(schema, schema, 0, &mut remaining)?;
    Ok(())
}

fn visit(value: &Value, root: &Value, depth: usize, remaining: &mut usize) -> AppResult<()> {
    if depth > 32 || *remaining == 0 {
        return Err(invalid());
    }
    *remaining -= 1;
    match value {
        Value::Object(object) => {
            for keyword in ["$ref", "$dynamicRef", "$recursiveRef"] {
                if let Some(reference) = object.get(keyword) {
                    let pointer = reference
                        .as_str()
                        .and_then(|reference| reference.strip_prefix('#'))
                        .ok_or_else(invalid)?;
                    let target = root.pointer(pointer).ok_or_else(invalid)?;
                    visit(target, root, depth + 1, remaining)?;
                }
            }
            for child in object.values() {
                visit(child, root, depth + 1, remaining)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                visit(child, root, depth + 1, remaining)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) struct NoExternalReferences;
impl jsonschema::Retrieve for NoExternalReferences {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("External schema references are prohibited".into())
    }
}

fn invalid() -> AppError {
    AppError::BadRequest("Invalid, external, recursive or oversized tool schema".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn schema_resolution_cannot_read_files_fetch_urls_or_recurse_without_progress() {
        for reference in ["file:///etc/passwd", "https://example.com/schema", "#"] {
            assert!(compile(&json!({"$ref":reference})).is_err());
        }
        let validator = compile(&json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"],"additionalProperties":false})).unwrap();
        assert!(validator.is_valid(&json!({"value":3})));
        assert!(!validator.is_valid(&json!({"value":"3"})));
        assert!(!validator.is_valid(&json!({"value":3,"extra":true})));
    }
}
