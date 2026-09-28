use super::syntax::child;
use super::{
    Diagnostic, LocatedNode, NodeValue, fields, json_value, required, scalar_string, sequence,
};
use crate::domain::workflow::{Binding, Comparison, ContextReference};
use std::collections::BTreeMap;

const OPERATORS: &[&str] = &[
    "literal", "ref", "object", "array", "concat", "default", "eq", "ne", "lt", "le", "gt", "ge",
    "in", "exists", "and", "or", "not",
];

fn pointer(node: &LocatedNode, path: &str) -> Result<ContextReference, Diagnostic> {
    let value = scalar_string(node, path)?;
    ContextReference::parse(value).map_err(|_| {
        Diagnostic::at(
            "binding.reference",
            "Invalid context pointer",
            path,
            node.span,
        )
    })
}

fn bindings(node: &LocatedNode, path: &str) -> Result<Vec<Binding>, Diagnostic> {
    sequence(node, path)?
        .iter()
        .enumerate()
        .map(|(i, item)| parse_binding(item, &child(path, &i.to_string())))
        .collect()
}

fn pair(node: &LocatedNode, path: &str) -> Result<(Box<Binding>, Box<Binding>), Diagnostic> {
    let items = sequence(node, path)?;
    if items.len() != 2 {
        return Err(Diagnostic::at(
            "binding.arity",
            "Expected exactly two bindings",
            path,
            node.span,
        ));
    }
    Ok((
        Box::new(parse_binding(&items[0], &child(path, "0"))?),
        Box::new(parse_binding(&items[1], &child(path, "1"))?),
    ))
}

fn object(node: &LocatedNode, path: &str) -> Result<BTreeMap<String, Binding>, Diagnostic> {
    fields(node, path, &[])?
        .into_iter()
        .map(|(key, value)| Ok((key.clone(), parse_binding(value, &child(path, &key))?)))
        .collect()
}

fn fallback(node: &LocatedNode, path: &str) -> Result<Binding, Diagnostic> {
    let map = fields(node, path, &["ref", "value"])?;
    Ok(Binding::Default {
        reference: pointer(required(&map, "ref", path, node)?, &child(path, "ref"))?,
        fallback: Box::new(parse_binding(
            required(&map, "value", path, node)?,
            &child(path, "value"),
        )?),
    })
}

pub(crate) fn parse_binding(node: &LocatedNode, path: &str) -> Result<Binding, Diagnostic> {
    let NodeValue::Mapping(_) = &node.value else {
        return Err(Diagnostic::at(
            "binding.operator",
            "Expected a one-operator binding mapping",
            path,
            node.span,
        ));
    };
    let map = fields(node, path, OPERATORS)?;
    if map.len() != 1 {
        return Err(Diagnostic::at(
            "binding.operator",
            "Binding must have exactly one operator",
            path,
            node.span,
        ));
    }
    let (name, operand) = map.into_iter().next().expect("one operator");
    let operand_path = child(path, &name);
    Ok(match name.as_str() {
        "literal" => Binding::Literal(json_value(operand, &operand_path)?),
        "ref" => Binding::Reference(pointer(operand, &operand_path)?),
        "object" => Binding::Object(object(operand, &operand_path)?),
        "array" => Binding::Array(bindings(operand, &operand_path)?),
        "concat" => Binding::Concat(bindings(operand, &operand_path)?),
        "default" => fallback(operand, &operand_path)?,
        "exists" => Binding::Exists(pointer(operand, &operand_path)?),
        "and" => Binding::And(bindings(operand, &operand_path)?),
        "or" => Binding::Or(bindings(operand, &operand_path)?),
        "not" => Binding::Not(Box::new(parse_binding(operand, &operand_path)?)),
        "in" => {
            let (value, items) = pair(operand, &operand_path)?;
            Binding::In { value, items }
        }
        "eq" | "ne" | "lt" | "le" | "gt" | "ge" => {
            let (left, right) = pair(operand, &operand_path)?;
            let op = match name.as_str() {
                "eq" => Comparison::Eq,
                "ne" => Comparison::Ne,
                "lt" => Comparison::Lt,
                "le" => Comparison::Le,
                "gt" => Comparison::Gt,
                "ge" => Comparison::Ge,
                _ => unreachable!(),
            };
            Binding::Compare { op, left, right }
        }
        _ => unreachable!(),
    })
}
