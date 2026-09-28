use super::schema_budget::check_resources;
use super::{Diagnostic, SourceSpan};
use crate::domain::workflow::canonical_array_index;
use crate::services::tool_schema::NoExternalReferences;
use jsonschema::{Draft, Validator};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) struct Schema {
    raw: Value,
    validator: Validator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PathGuarantee {
    Impossible,
    UnsafeTraversal,
    Optional,
    Guaranteed,
}

impl Schema {
    pub fn compile(raw: Value, path: &str, span: SourceSpan) -> Result<Self, Diagnostic> {
        check_resources(&raw).map_err(|_| {
            Diagnostic::at(
                "schema.resource",
                "Schema exceeds limits or has an external or recursive reference",
                path,
                span,
            )
        })?;
        check_dialect(&raw)
            .map_err(|message| Diagnostic::at("schema.dialect", message, path, span))?;
        if !jsonschema::draft202012::meta::is_valid(&raw) {
            return Err(Diagnostic::at(
                "schema.meta",
                "Schema is not valid Draft 2020-12",
                path,
                span,
            ));
        }
        let validator = jsonschema::options()
            .with_draft(Draft::Draft202012)
            .with_retriever(NoExternalReferences)
            .with_pattern_options(jsonschema::PatternOptions::regex().size_limit(65_536))
            .build(&raw)
            .map_err(|_| {
                Diagnostic::at("schema.compile", "Unable to compile schema", path, span)
            })?;
        Ok(Self { raw, validator })
    }

    pub fn raw(&self) -> &Value {
        &self.raw
    }

    pub fn validate(&self, value: &Value, path: &str, span: SourceSpan) -> Result<(), Diagnostic> {
        let Err(error) = self.validator.validate(value) else {
            return Ok(());
        };
        let mut diagnostic =
            Diagnostic::at("schema.value", "Value does not match schema", path, span);
        diagnostic.instance_path = Some(
            error
                .instance_path
                .to_string()
                .chars()
                .take(512)
                .collect::<String>()
                .into_boxed_str(),
        );
        diagnostic.schema_path = Some(
            error
                .schema_path
                .to_string()
                .chars()
                .take(512)
                .collect::<String>()
                .into_boxed_str(),
        );
        Err(diagnostic)
    }

    pub fn guarantees(&self, path: &[String]) -> PathGuarantee {
        project(&self.raw, &self.raw, path, 0, &mut BTreeSet::new())
    }

    pub fn root_constraints(&self) -> Vec<&Value> {
        local_constraints(&self.raw, &self.raw)
    }

    pub fn object_possible(&self) -> bool {
        self.root_constraints().iter().all(|schema| {
            !known_empty(schema, &self.raw, 0, &mut BTreeSet::new())
                && type_allows(schema, "object")
        })
    }
}

pub(crate) fn local_constraints<'a>(schema: &'a Value, root: &'a Value) -> Vec<&'a Value> {
    let mut current = schema;
    let mut seen = BTreeSet::new();
    let mut constraints = vec![current];
    for _ in 0..32 {
        let Some(reference) = current.get("$ref").and_then(Value::as_str) else {
            break;
        };
        if !seen.insert(reference) {
            break;
        }
        let Some(target) = reference
            .strip_prefix('#')
            .and_then(|path| root.pointer(path))
        else {
            break;
        };
        current = target;
        constraints.push(current);
    }
    constraints
}

pub(crate) fn validate_known_literal(
    schema: &Value,
    root: &Value,
    value: &Value,
    path: &str,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    let location = schema_location(root, schema).ok_or_else(|| {
        Diagnostic::at(
            "schema.compile",
            "Unable to locate destination schema",
            path,
            span,
        )
    })?;
    let selector = json!({"$ref": format!("json-schema:///#{location}")});
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_retriever(NoExternalReferences)
        .with_pattern_options(jsonschema::PatternOptions::regex().size_limit(65_536))
        .with_resource(
            "json-schema:///",
            Draft::Draft202012.create_resource(root.clone()),
        )
        .with_base_uri("urn:workflow:selection")
        .build(&selector)
        .map_err(|_| {
            Diagnostic::at(
                "schema.compile",
                "Unable to compile scoped schema",
                path,
                span,
            )
        })?;
    if validator.is_valid(value) {
        Ok(())
    } else {
        Err(Diagnostic::at(
            "binding.type",
            "Literal does not match destination schema",
            path,
            span,
        ))
    }
}

// The accepted root is bounded by check_resources (depth 32, 4096 visits).
// Identity matters: equal JSON fragments may have different enclosing $id scopes.
fn schema_location(root: &Value, target: &Value) -> Option<String> {
    fn find(value: &Value, target: &Value, path: &mut String, remaining: &mut usize) -> bool {
        if *remaining == 0 {
            return false;
        }
        *remaining -= 1;
        if std::ptr::eq(value, target) {
            return true;
        }
        match value {
            Value::Object(fields) => {
                for (name, child) in fields {
                    let end = path.len();
                    path.push('/');
                    encode_pointer_token(name, path);
                    if find(child, target, path, remaining) {
                        return true;
                    }
                    path.truncate(end);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    let end = path.len();
                    path.push('/');
                    path.push_str(&index.to_string());
                    if find(child, target, path, remaining) {
                        return true;
                    }
                    path.truncate(end);
                }
            }
            _ => {}
        }
        false
    }

    let mut location = String::new();
    find(root, target, &mut location, &mut 4096).then_some(location)
}

fn encode_pointer_token(token: &str, output: &mut String) {
    for byte in token.bytes() {
        match byte {
            b'~' => output.push_str("~0"),
            b'/' => output.push_str("~1"),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => {
                output.push(char::from(byte));
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                output.push('%');
                output.push(char::from(HEX[(byte >> 4) as usize]));
                output.push(char::from(HEX[(byte & 0x0f) as usize]));
            }
        }
    }
}

pub(crate) fn is_known_empty(schema: &Value, root: &Value) -> bool {
    known_empty(schema, root, 0, &mut BTreeSet::new())
}

fn known_empty(schema: &Value, root: &Value, depth: usize, seen: &mut BTreeSet<String>) -> bool {
    if schema == &Value::Bool(false) {
        return true;
    }
    if depth > 32 {
        return false;
    }
    let Value::Object(map) = schema else {
        return false;
    };
    if let Some(reference) = map.get("$ref").and_then(Value::as_str)
        && seen.insert(reference.to_owned())
    {
        let empty = reference
            .strip_prefix('#')
            .and_then(|path| root.pointer(path))
            .is_some_and(|target| known_empty(target, root, depth + 1, seen));
        seen.remove(reference);
        if empty {
            return true;
        }
    }
    map.get("allOf")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| known_empty(item, root, depth + 1, seen))
        })
        || ["anyOf", "oneOf"].iter().any(|keyword| {
            map.get(*keyword)
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    !items.is_empty()
                        && items
                            .iter()
                            .all(|item| known_empty(item, root, depth + 1, seen))
                })
        })
}

fn check_dialect(value: &Value) -> Result<(), &'static str> {
    super::schema_budget::check_dialects(value)
        .map_err(|_| "Only Draft 2020-12 schemas are supported")
}

pub(crate) fn type_allows(schema: &Value, kind: &str) -> bool {
    let Some(types) = schema.get("type") else {
        return true;
    };
    match types {
        Value::String(value) => value == kind || kind == "integer" && value == "number",
        Value::Array(items) => items.iter().any(|item| {
            item.as_str() == Some(kind) || kind == "integer" && item.as_str() == Some("number")
        }),
        _ => true,
    }
}

fn merge_alternatives(
    items: &[Value],
    root: &Value,
    path: &[String],
    depth: usize,
    seen: &mut BTreeSet<String>,
) -> PathGuarantee {
    let mut possible = false;
    let mut all_guaranteed = true;
    let mut unsafe_branch = false;
    for branch in items {
        if branch == &Value::Bool(false) {
            continue;
        }
        let result = project(branch, root, path, depth + 1, seen);
        possible |= matches!(result, PathGuarantee::Optional | PathGuarantee::Guaranteed);
        all_guaranteed &= result == PathGuarantee::Guaranteed;
        unsafe_branch |= result == PathGuarantee::UnsafeTraversal;
    }
    if unsafe_branch {
        PathGuarantee::UnsafeTraversal
    } else if !possible {
        PathGuarantee::Impossible
    } else if all_guaranteed {
        PathGuarantee::Guaranteed
    } else {
        PathGuarantee::Optional
    }
}

fn project(
    schema: &Value,
    root: &Value,
    path: &[String],
    depth: usize,
    seen: &mut BTreeSet<String>,
) -> PathGuarantee {
    if depth > 32 || schema == &Value::Bool(false) {
        return PathGuarantee::Impossible;
    }
    if path.is_empty() {
        return PathGuarantee::Guaranteed;
    }
    let Value::Object(map) = schema else {
        return PathGuarantee::Optional;
    };
    if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
        if !seen.insert(reference.to_owned()) {
            return PathGuarantee::Optional;
        }
        let result = reference
            .strip_prefix('#')
            .and_then(|p| root.pointer(p))
            .map_or(PathGuarantee::Optional, |target| {
                project(target, root, path, depth + 1, seen)
            });
        seen.remove(reference);
        return result;
    }
    for keyword in ["anyOf", "oneOf"] {
        if let Some(items) = map.get(keyword).and_then(Value::as_array) {
            return merge_alternatives(items, root, path, depth, seen);
        }
    }
    if let Some(items) = map.get("allOf").and_then(Value::as_array) {
        // Sibling properties/required constrain the same instance as allOf.
        // In particular, conditional human feedback must not erase required data.
        let siblings = project_container(schema, root, path, depth, seen);
        return merge_all(items, root, path, depth, seen, siblings);
    }
    project_container(schema, root, path, depth, seen)
}

fn merge_all(
    items: &[Value],
    root: &Value,
    path: &[String],
    depth: usize,
    seen: &mut BTreeSet<String>,
    siblings: PathGuarantee,
) -> PathGuarantee {
    if matches!(
        siblings,
        PathGuarantee::Impossible | PathGuarantee::UnsafeTraversal
    ) {
        return siblings;
    }
    let mut guaranteed = siblings == PathGuarantee::Guaranteed;
    for branch in items {
        match project(branch, root, path, depth + 1, seen) {
            PathGuarantee::Impossible => return PathGuarantee::Impossible,
            PathGuarantee::UnsafeTraversal => return PathGuarantee::UnsafeTraversal,
            PathGuarantee::Guaranteed => guaranteed = true,
            PathGuarantee::Optional => {}
        }
    }
    if guaranteed {
        PathGuarantee::Guaranteed
    } else {
        PathGuarantee::Optional
    }
}

fn project_container(
    schema: &Value,
    root: &Value,
    path: &[String],
    depth: usize,
    seen: &mut BTreeSet<String>,
) -> PathGuarantee {
    let segment = &path[0];
    let object = type_allows(schema, "object");
    let array = type_allows(schema, "array");
    let scalar_possible = schema.get("type").is_some_and(|types| match types {
        Value::String(value) => value != "object" && value != "array",
        Value::Array(items) => items
            .iter()
            .any(|item| !matches!(item.as_str(), Some("object" | "array"))),
        _ => false,
    });
    let possible = if object {
        project_object(schema, root, segment, &path[1..], depth, seen)
    } else {
        PathGuarantee::Impossible
    };
    let indexed = if array {
        project_array(schema, root, segment, &path[1..], depth, seen)
    } else {
        PathGuarantee::Impossible
    };
    let combined = if possible == PathGuarantee::Impossible {
        indexed
    } else if indexed == PathGuarantee::Impossible {
        possible
    } else if possible == PathGuarantee::Guaranteed && indexed == PathGuarantee::Guaranteed {
        PathGuarantee::Guaranteed
    } else {
        PathGuarantee::Optional
    };
    if scalar_possible {
        PathGuarantee::UnsafeTraversal
    } else {
        combined
    }
}

fn project_object(
    schema: &Value,
    root: &Value,
    segment: &str,
    rest: &[String],
    depth: usize,
    seen: &mut BTreeSet<String>,
) -> PathGuarantee {
    let property = schema.get("properties").and_then(|p| p.get(segment));
    let additional = schema.get("additionalProperties");
    let Some(child) = property.or(additional) else {
        return PathGuarantee::Optional;
    };
    if child == &Value::Bool(false) {
        return PathGuarantee::Impossible;
    }
    let sub = project(child, root, rest, depth + 1, seen);
    if matches!(
        sub,
        PathGuarantee::Impossible | PathGuarantee::UnsafeTraversal
    ) {
        return sub;
    }
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(segment)));
    if required && type_only(schema, "object") && sub == PathGuarantee::Guaranteed {
        PathGuarantee::Guaranteed
    } else {
        PathGuarantee::Optional
    }
}

fn project_array(
    schema: &Value,
    root: &Value,
    segment: &str,
    rest: &[String],
    depth: usize,
    seen: &mut BTreeSet<String>,
) -> PathGuarantee {
    let Some(index) = canonical_array_index(segment) else {
        return PathGuarantee::Impossible;
    };
    if schema
        .get("maxItems")
        .and_then(Value::as_u64)
        .is_some_and(|max| index >= max as usize)
    {
        return PathGuarantee::Impossible;
    }
    let prefix = schema.get("prefixItems").and_then(Value::as_array);
    let child = prefix
        .and_then(|items| items.get(index))
        .or_else(|| schema.get("items"));
    let Some(child) = child else {
        return PathGuarantee::Optional;
    };
    if child == &Value::Bool(false) {
        return PathGuarantee::Impossible;
    }
    let sub = project(child, root, rest, depth + 1, seen);
    if matches!(
        sub,
        PathGuarantee::Impossible | PathGuarantee::UnsafeTraversal
    ) {
        return sub;
    }
    let min = schema.get("minItems").and_then(Value::as_u64).unwrap_or(0);
    if type_only(schema, "array") && min > index as u64 && sub == PathGuarantee::Guaranteed {
        PathGuarantee::Guaranteed
    } else {
        PathGuarantee::Optional
    }
}

fn type_only(schema: &Value, kind: &str) -> bool {
    schema.get("type").and_then(Value::as_str) == Some(kind)
}

#[cfg(test)]
#[path = "schema_projection_tests.rs"]
mod projection_tests;
