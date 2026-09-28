use super::{Diagnostic, LocatedNode, NodeValue};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(crate) fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

pub(crate) fn child(path: &str, segment: &str) -> String {
    format!("{path}/{}", escape(segment))
}

pub(crate) fn fields<'a>(
    node: &'a LocatedNode,
    path: &str,
    allowed: &[&str],
) -> Result<BTreeMap<String, &'a LocatedNode>, Diagnostic> {
    let NodeValue::Mapping(entries) = &node.value else {
        return Err(Diagnostic::at(
            "syntax.mapping",
            "Expected a mapping",
            path,
            node.span,
        ));
    };
    let mut result = BTreeMap::new();
    for (key, value) in entries {
        let name = scalar_string(key, path)?;
        let field = child(path, name);
        if !allowed.is_empty() && !allowed.contains(&name) {
            return Err(Diagnostic::at(
                "syntax.unknown_field",
                "Unknown field",
                &field,
                key.span,
            ));
        }
        if result.insert(name.to_owned(), value).is_some() {
            return Err(Diagnostic::at(
                "syntax.duplicate",
                "Duplicate mapping key",
                &field,
                key.span,
            ));
        }
    }
    Ok(result)
}

pub(crate) fn required<'a>(
    fields: &'a BTreeMap<String, &'a LocatedNode>,
    key: &str,
    path: &str,
    enclosing: &LocatedNode,
) -> Result<&'a LocatedNode, Diagnostic> {
    fields.get(key).copied().ok_or_else(|| {
        Diagnostic::at(
            "syntax.required",
            &format!("Missing required field {key}"),
            path,
            enclosing.span,
        )
    })
}

pub(crate) fn scalar_string<'a>(node: &'a LocatedNode, path: &str) -> Result<&'a str, Diagnostic> {
    match &node.value {
        NodeValue::Scalar(Value::String(value)) => Ok(value),
        _ => Err(Diagnostic::at(
            "syntax.string",
            "Expected a string",
            path,
            node.span,
        )),
    }
}

pub(crate) fn sequence<'a>(
    node: &'a LocatedNode,
    path: &str,
) -> Result<&'a [LocatedNode], Diagnostic> {
    match &node.value {
        NodeValue::Sequence(items) => Ok(items),
        _ => Err(Diagnostic::at(
            "syntax.sequence",
            "Expected a sequence",
            path,
            node.span,
        )),
    }
}

pub(crate) fn json_value(node: &LocatedNode, path: &str) -> Result<Value, Diagnostic> {
    match &node.value {
        NodeValue::Scalar(value) => Ok(value.clone()),
        NodeValue::Sequence(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| json_value(item, &child(path, &i.to_string())))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        NodeValue::Mapping(_) => {
            let mut result = Map::new();
            for (key, value) in fields(node, path, &[])? {
                result.insert(key.clone(), json_value(value, &child(path, &key))?);
            }
            Ok(Value::Object(result))
        }
    }
}
