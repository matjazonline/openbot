//! Conservative static checks for bindings with known result shapes.
use super::schema::{is_known_empty, local_constraints, validate_known_literal};
use super::syntax::child;
use super::{Diagnostic, SourceSpan};
use crate::domain::workflow::{Binding, Comparison};
use bigdecimal::BigDecimal;
use serde_json::Value;
use std::str::FromStr;

fn known_kind(binding: &Binding) -> Option<&'static str> {
    match binding {
        Binding::Literal(Value::Null) => Some("null"),
        Binding::Literal(Value::Bool(_)) => Some("boolean"),
        Binding::Literal(Value::String(_)) | Binding::Concat(_) => Some("string"),
        Binding::Literal(Value::Number(n)) => Some(
            if BigDecimal::from_str(&n.to_string())
                .expect("finite JSON decimal")
                .is_integer()
            {
                "integer"
            } else {
                "number"
            },
        ),
        Binding::Literal(Value::Array(_)) | Binding::Array(_) => Some("array"),
        Binding::Literal(Value::Object(_)) | Binding::Object(_) => Some("object"),
        Binding::Compare { .. }
        | Binding::In { .. }
        | Binding::Exists(_)
        | Binding::And(_)
        | Binding::Or(_)
        | Binding::Not(_) => Some("boolean"),
        Binding::Reference(_) | Binding::Default { .. } => None,
    }
}

fn child_schema<'a>(schema: &'a Value, key: &str) -> Option<&'a Value> {
    schema
        .get("properties")
        .and_then(|properties| properties.get(key))
        .or_else(|| schema.get("additionalProperties"))
}

fn array_schema(schema: &Value, index: usize) -> Option<&Value> {
    schema
        .get("prefixItems")
        .and_then(Value::as_array)
        .and_then(|items| items.get(index))
        .or_else(|| schema.get("items"))
}

fn require_kind(
    binding: &Binding,
    expected: &str,
    path: &str,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    if known_kind(binding).is_some_and(|kind| kind != expected) {
        return Err(Diagnostic::at(
            "binding.operand",
            "Operator operand has an incompatible known type",
            path,
            span,
        ));
    }
    Ok(())
}

fn check_ordering(
    left: &Binding,
    right: &Binding,
    path: &str,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    let kind = |binding| match known_kind(binding) {
        Some("integer" | "number") => Some("number"),
        Some("string") => Some("string"),
        Some(_) => Some("invalid"),
        None => None,
    };
    let left_kind = kind(left);
    let right_kind = kind(right);
    if left_kind == Some("invalid")
        || right_kind == Some("invalid")
        || left_kind.is_some() && right_kind.is_some() && left_kind != right_kind
    {
        return Err(Diagnostic::at(
            "binding.operand",
            "Ordering needs two numbers or two strings",
            path,
            span,
        ));
    }
    Ok(())
}

pub(crate) fn check_operator_operands(
    binding: &Binding,
    path: &str,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    match binding {
        Binding::Concat(items) => {
            for item in items {
                require_kind(item, "string", path, span)?;
                check_operator_operands(item, path, span)?;
            }
        }
        Binding::And(items) | Binding::Or(items) => {
            for item in items {
                require_kind(item, "boolean", path, span)?;
                check_operator_operands(item, path, span)?;
            }
        }
        Binding::Not(item) => {
            require_kind(item, "boolean", path, span)?;
            check_operator_operands(item, path, span)?;
        }
        Binding::In { value, items } => {
            require_kind(items, "array", path, span)?;
            check_operator_operands(value, path, span)?;
            check_operator_operands(items, path, span)?;
        }
        Binding::Compare { op, left, right } => {
            if !matches!(op, Comparison::Eq | Comparison::Ne) {
                check_ordering(left, right, path, span)?;
            }
            check_operator_operands(left, path, span)?;
            check_operator_operands(right, path, span)?;
        }
        Binding::Object(fields) => {
            for child in fields.values() {
                check_operator_operands(child, path, span)?;
            }
        }
        Binding::Array(items) => {
            for child in items {
                check_operator_operands(child, path, span)?;
            }
        }
        Binding::Default { fallback, .. } => check_operator_operands(fallback, path, span)?,
        Binding::Literal(_) | Binding::Reference(_) | Binding::Exists(_) => {}
    }
    Ok(())
}

pub(crate) fn check_binding_shape(
    binding: &Binding,
    schema: &Value,
    root_schema: &Value,
    path: &str,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    check_operator_operands(binding, path, span)?;
    if let Binding::Literal(value) = binding {
        return validate_known_literal(schema, root_schema, value, path, span);
    }
    // Peel fallbacks before traversing schema conjunctions so nested defaults
    // and allOf cannot revisit the same binding/schema pairs combinatorially.
    if let Binding::Default { fallback, .. } = binding {
        return check_binding_shape(fallback, schema, root_schema, path, span);
    }
    if is_known_empty(schema, root_schema) {
        return Err(Diagnostic::at(
            "binding.type",
            "Destination schema rejects every value",
            path,
            span,
        ));
    }
    let constraints = local_constraints(schema, root_schema);
    if let Some(kind) = known_kind(binding) {
        for constraint in &constraints {
            if !super::schema::type_allows(constraint, kind) {
                return Err(Diagnostic::at(
                    "binding.type",
                    "Binding type cannot satisfy destination schema",
                    path,
                    span,
                ));
            }
        }
    }
    // Conjunctive schemas all constrain this binding, including embedded tool
    // contracts. Check constructed children against each branch as well.
    for constraint in &constraints {
        if let Some(branches) = constraint.get("allOf").and_then(Value::as_array) {
            for branch in branches {
                check_binding_shape(binding, branch, root_schema, path, span)?;
            }
        }
    }
    match binding {
        Binding::Object(fields) => {
            for constraint in constraints {
                check_object_shape(fields, constraint, root_schema, path, span)?;
            }
            Ok(())
        }
        Binding::Array(items) => {
            for constraint in constraints {
                check_array_shape(items, constraint, root_schema, path, span)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_object_shape(
    fields: &std::collections::BTreeMap<String, Binding>,
    schema: &Value,
    root_schema: &Value,
    path: &str,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    for (name, child_binding) in fields {
        let child_path = child(path, name);
        if let Some(child_schema) = child_schema(schema, name) {
            if child_schema == &Value::Bool(false) {
                return Err(Diagnostic::at(
                    "binding.property",
                    "Destination schema forbids this property",
                    &child_path,
                    span,
                ));
            }
            check_binding_shape(child_binding, child_schema, root_schema, &child_path, span)?;
        }
    }
    for name in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !fields.contains_key(name) {
            return Err(Diagnostic::at(
                "binding.required",
                "Required constructed property is missing",
                path,
                span,
            ));
        }
    }
    Ok(())
}

fn check_array_shape(
    items: &[Binding],
    schema: &Value,
    root_schema: &Value,
    path: &str,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    for (index, binding) in items.iter().enumerate() {
        let child_path = child(path, &index.to_string());
        if let Some(child_schema) = array_schema(schema, index) {
            if child_schema == &Value::Bool(false) {
                return Err(Diagnostic::at(
                    "binding.item",
                    "Destination schema forbids this array item",
                    &child_path,
                    span,
                ));
            }
            check_binding_shape(binding, child_schema, root_schema, &child_path, span)?;
        }
    }
    if schema
        .get("minItems")
        .and_then(Value::as_u64)
        .is_some_and(|minimum| items.len() < minimum as usize)
    {
        return Err(Diagnostic::at(
            "binding.items",
            "Constructed array is shorter than required",
            path,
            span,
        ));
    }
    Ok(())
}
