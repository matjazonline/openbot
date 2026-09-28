//! Bounded, span-aware YAML decoding at the adapter boundary.
use crate::application::workflow::compiler::{
    DecodedSource, Diagnostic, LocatedNode, NodeValue, SourceSpan,
};
use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use serde_saphyr::{Budget, Options, Spanned, from_str_with_options};
use std::fmt;

pub use crate::application::workflow::compiler::MAX_SOURCE_BYTES;

/// Bounded production YAML decoder supplied to application composition.
pub struct WorkflowSourceDecoder;
impl crate::application::workflow::compiler::SourceDecoder for WorkflowSourceDecoder {
    fn decode(&self, source: &str) -> Result<DecodedSource, Diagnostic> {
        decode(source)
    }
}
const MAX_NODES: usize = 8192;
const MAX_SCALAR_BYTES: usize = 262_144;
const MAX_DEPTH: usize = 48;

struct YamlNode(Spanned<YamlValue>);
enum YamlValue {
    Scalar(Value),
    Sequence(Vec<YamlNode>),
    Mapping(Vec<(YamlNode, YamlNode)>),
}

impl<'de> Deserialize<'de> for YamlNode {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Spanned::<YamlValue>::deserialize(deserializer).map(Self)
    }
}

impl<'de> Deserialize<'de> for YamlValue {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NodeVisitor;
        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = YamlValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON-compatible YAML node")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(YamlValue::Scalar(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                self.visit_unit()
            }
            fn visit_bool<E: de::Error>(self, x: bool) -> Result<Self::Value, E> {
                Ok(YamlValue::Scalar(Value::Bool(x)))
            }
            fn visit_i64<E: de::Error>(self, x: i64) -> Result<Self::Value, E> {
                Ok(YamlValue::Scalar(Value::from(x)))
            }
            fn visit_u64<E: de::Error>(self, x: u64) -> Result<Self::Value, E> {
                Ok(YamlValue::Scalar(Value::from(x)))
            }
            fn visit_f64<E: de::Error>(self, x: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(x)
                    .map(|n| YamlValue::Scalar(Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: de::Error>(self, x: &str) -> Result<Self::Value, E> {
                Ok(YamlValue::Scalar(Value::String(x.into())))
            }
            fn visit_string<E: de::Error>(self, x: String) -> Result<Self::Value, E> {
                Ok(YamlValue::Scalar(Value::String(x)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(YamlValue::Sequence(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut pairs = Vec::new();
                while let Some(pair) = map.next_entry()? {
                    pairs.push(pair);
                }
                Ok(YamlValue::Mapping(pairs))
            }
        }
        deserializer.deserialize_any(NodeVisitor)
    }
}

fn span(location: serde_saphyr::Location) -> SourceSpan {
    let source = location.span();
    let start = source.byte_offset().unwrap_or(0) as usize;
    SourceSpan {
        start,
        end: start.saturating_add(source.byte_len().unwrap_or(0) as usize),
        line: location.line() as usize,
        column: location.column() as usize,
    }
}

fn convert(node: YamlNode) -> LocatedNode {
    let value = match node.0.value {
        YamlValue::Scalar(value) => NodeValue::Scalar(value),
        YamlValue::Sequence(items) => NodeValue::Sequence(items.into_iter().map(convert).collect()),
        YamlValue::Mapping(pairs) => NodeValue::Mapping(
            pairs
                .into_iter()
                .map(|(k, v)| (convert(k), convert(v)))
                .collect(),
        ),
    };
    LocatedNode {
        value,
        span: span(node.0.referenced),
    }
}

pub fn decode(source: &str) -> Result<DecodedSource, Diagnostic> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Diagnostic::at(
            "source.limit",
            "YAML source exceeds 256 KiB",
            "",
            SourceSpan {
                start: 0,
                end: 0,
                line: 1,
                column: 1,
            },
        ));
    }
    let mut budget = Budget::default();
    budget.max_events = MAX_NODES * 4;
    budget.max_nodes = MAX_NODES;
    budget.max_depth = MAX_DEPTH;
    budget.max_total_scalar_bytes = MAX_SCALAR_BYTES;
    budget.max_documents = 1;
    budget.max_aliases = 0;
    budget.max_anchors = 0;
    budget.max_merge_keys = 0;
    budget.max_recorded_anchor_events = 0;
    budget.max_recorded_anchor_bytes = 0;
    let mut options = Options::default();
    options.budget = Some(budget);
    options.merge_keys = serde_saphyr::options::MergeKeyPolicy::Error;
    options.strict_booleans = true;
    options.reject_unsupported_tags = true;
    options.with_snippet = false;
    let root: YamlNode = from_str_with_options(source, options).map_err(|error| {
        let at = error.location().map(span).unwrap_or(SourceSpan {
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        });
        Diagnostic::at(
            "source.yaml",
            "Invalid or unsupported YAML; check syntax, duplicate keys, tags, aliases and limits",
            "",
            at,
        )
    })?;
    Ok(DecodedSource {
        source: source.into(),
        root: convert(root),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_yaml_locations_and_rejections() {
        let source = "title: héllo\nsteps: {\"quoted/key\": {ref: /input/name}}\n";
        let decoded = decode(source).unwrap();
        let NodeValue::Mapping(fields) = decoded.root.value else {
            panic!("mapping");
        };
        let NodeValue::Mapping(steps) = &fields[1].1.value else {
            panic!("steps");
        };
        let NodeValue::Mapping(binding) = &steps[0].1.value else {
            panic!("binding");
        };
        assert_eq!(binding[0].1.span.line, 2);
        assert_eq!(binding[0].1.span.column, 29);
        assert_eq!(
            &source[binding[0].1.span.start..binding[0].1.span.end],
            "/input/name"
        );
        assert!(decode("a: 1\na: 2\n").is_err());
        assert!(decode("left: {same: 1}\nright: {same: 2}\n").is_ok());
        assert!(decode("left: {same: 1, same: 2}\n").is_err());
        assert!(decode("x: !custom value\n").is_err());
        assert!(decode("x: &anchor 1\ny: *anchor\n").is_err());
        assert!(decode("x: 1\n---\ny: 2\n").is_err());
    }

    #[test]
    fn workflow_yaml_budgets_reject_at_boundary() {
        assert!(decode(&"x".repeat(MAX_SOURCE_BYTES + 1)).is_err());
        assert!(decode(&"x".repeat(MAX_SOURCE_BYTES)).is_ok());
        assert!(
            decode(&format!(
                "{}0{}",
                "[".repeat(MAX_DEPTH),
                "]".repeat(MAX_DEPTH)
            ))
            .is_ok()
        );
        assert!(decode(&format!("{}value", "[".repeat(MAX_DEPTH + 1))).is_err());
        let exact = format!("[{}]", vec!["0"; MAX_NODES - 1].join(","));
        assert!(decode(&exact).is_ok());
        let nodes = format!("items: [{}]", vec!["1"; MAX_NODES + 1].join(","));
        assert!(decode(&nodes).is_err());
    }
}
