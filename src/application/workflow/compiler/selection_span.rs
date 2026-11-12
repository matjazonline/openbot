//! Container locations identify their opening token; scalar locations cover their contents.
//! Selection edits recover the complete container extent and revalidate the entire result.
use super::{Diagnostic, LocatedNode, NodeValue, SourceSpan};

pub(super) fn inputs_span(
    root: &LocatedNode,
    step: &str,
    source: &str,
) -> Result<SourceSpan, Diagnostic> {
    let mut node = root;
    for segment in ["steps", step, "with"] {
        let NodeValue::Mapping(fields) = &node.value else {
            return Err(error(node.span));
        };
        node = fields
            .iter()
            .find_map(|(key, value)| match &key.value {
                NodeValue::Scalar(serde_json::Value::String(key)) if key == segment => Some(value),
                _ => None,
            })
            .ok_or_else(|| error(node.span))?;
    }
    Ok(SourceSpan {
        end: node_end(node, source)?,
        ..node.span
    })
}

fn node_end(node: &LocatedNode, source: &str) -> Result<usize, Diagnostic> {
    match &node.value {
        NodeValue::Scalar(_) => Ok(node.span.end),
        NodeValue::Mapping(fields) => {
            if source.as_bytes().get(node.span.start) == Some(&b'{') {
                return flow_end(node.span, source);
            }
            fields.iter().try_fold(node.span.end, |end, (key, value)| {
                Ok(end
                    .max(node_end(key, source)?)
                    .max(node_end(value, source)?))
            })
        }
        NodeValue::Sequence(items) => {
            if source.as_bytes().get(node.span.start) == Some(&b'[') {
                return flow_end(node.span, source);
            }
            items.iter().try_fold(node.span.end, |end, item| {
                Ok(end.max(node_end(item, source)?))
            })
        }
    }
}

fn flow_end(span: SourceSpan, source: &str) -> Result<usize, Diagnostic> {
    let bytes = source.as_bytes();
    let mut stack = Vec::new();
    let mut quote = None;
    let mut comment = false;
    let mut index = span.start;
    while let Some(&byte) = bytes.get(index) {
        if comment {
            comment = byte != b'\n';
            index += 1;
            continue;
        }
        if let Some(delimiter) = quote {
            if delimiter == b'"' && byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == delimiter {
                if delimiter == b'\'' && bytes.get(index + 1) == Some(&b'\'') {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'#' if index == 0 || bytes[index - 1].is_ascii_whitespace() => comment = true,
            b'{' | b'[' => stack.push(byte),
            b'}' | b']' => {
                if stack.pop() != Some(if byte == b'}' { b'{' } else { b'[' }) {
                    return Err(error(span));
                }
                if stack.is_empty() {
                    return Ok(index + 1);
                }
            }
            _ => {}
        }
        index += 1;
    }
    Err(error(span))
}
fn error(span: SourceSpan) -> Diagnostic {
    Diagnostic::at(
        "workflow.selection_span",
        "Cannot locate the complete input mapping; edit this step in source",
        "/steps",
        span,
    )
}
