//! Bound schema work before native validation, preserving nested resource roots.
//! JSON-valued annotations/const/enum are data, not schemas containing references.
use serde_json::Value;
use std::io::{self, Write};

struct Bytes(usize);
impl Write for Bytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("schema bytes"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn check_resources(schema: &Value) -> Result<(), ()> {
    // Bound the borrowed tree before serialization or recursive native compilation.
    visit_data(schema, 0, &mut 4096)?;
    serde_json::to_writer(Bytes(65_536), schema).map_err(|_| ())?;
    visit_schema(schema, schema, 0, &mut 4096, &mut |_| Ok(()))
}
fn visit_data(value: &Value, depth: usize, remaining: &mut usize) -> Result<(), ()> {
    debit(depth, remaining)?;
    match value {
        Value::Object(map) => {
            for child in map.values() {
                visit_data(child, depth + 1, remaining)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                visit_data(child, depth + 1, remaining)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn debit(depth: usize, remaining: &mut usize) -> Result<(), ()> {
    if depth > 32 || *remaining == 0 {
        return Err(());
    }
    *remaining -= 1;
    Ok(())
}
fn visit_schema<'a>(
    schema: &'a Value,
    root: &'a Value,
    depth: usize,
    remaining: &mut usize,
    inspect: &mut impl FnMut(&Value) -> Result<(), ()>,
) -> Result<(), ()> {
    debit(depth, remaining)?;
    inspect(schema)?;
    let Some(map) = schema.as_object() else {
        return Ok(());
    };
    let root = if map.contains_key("$id") {
        schema
    } else {
        root
    };
    for keyword in ["$ref", "$dynamicRef", "$recursiveRef"] {
        if let Some(reference) = map.get(keyword) {
            let pointer = reference
                .as_str()
                .and_then(|s| s.strip_prefix('#'))
                .ok_or(())?;
            let target = root.pointer(pointer).ok_or(())?;
            visit_schema(target, root, depth + 1, remaining, inspect)?;
        }
    }
    for child in schema_children(schema) {
        visit_schema(child, root, depth + 1, remaining, inspect)?;
    }
    Ok(())
}

pub(super) fn schema_children(schema: &Value) -> Vec<&Value> {
    let mut children = Vec::new();
    let Some(map) = schema.as_object() else {
        return children;
    };
    for (keyword, value) in map {
        match keyword.as_str() {
            "$defs" | "definitions" | "properties" | "patternProperties" | "dependentSchemas" => {
                if let Some(map) = value.as_object() {
                    children.extend(map.values());
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                if let Some(values) = value.as_array() {
                    children.extend(values);
                }
            }
            "additionalProperties"
            | "unevaluatedProperties"
            | "items"
            | "unevaluatedItems"
            | "contains"
            | "not"
            | "if"
            | "then"
            | "else"
            | "propertyNames" => children.push(value),
            _ => {}
        }
    }
    children
}

pub(super) fn check_dialects(schema: &Value) -> Result<(), ()> {
    visit_schema(schema, schema, 0, &mut 4096, &mut |value| {
        if value.get("$schema").is_some_and(|dialect| {
            dialect.as_str() != Some("https://json-schema.org/draft/2020-12/schema")
        }) {
            Err(())
        } else {
            Ok(())
        }
    })
}
