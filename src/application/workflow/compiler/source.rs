use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub enum NodeValue {
    Scalar(Value),
    Sequence(Vec<LocatedNode>),
    Mapping(Vec<(LocatedNode, LocatedNode)>),
}

#[derive(Debug, Clone)]
pub struct LocatedNode {
    pub value: NodeValue,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct DecodedSource {
    pub(crate) source: String,
    pub(crate) root: LocatedNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: Box<str>,
    pub field_path: Box<str>,
    pub span: SourceSpan,
    pub instance_path: Option<Box<str>>,
    pub schema_path: Option<Box<str>>,
}

impl Diagnostic {
    pub fn at(code: &'static str, message: &str, field_path: &str, span: SourceSpan) -> Self {
        Self {
            code,
            message: message
                .chars()
                .take(240)
                .collect::<String>()
                .into_boxed_str(),
            field_path: field_path
                .chars()
                .take(512)
                .collect::<String>()
                .into_boxed_str(),
            span,
            instance_path: None,
            schema_path: None,
        }
    }
}

/// Locate a semantic input field in literal data, or its enclosing dynamic binding.
pub(crate) fn input_span(
    locations: &std::collections::BTreeMap<String, SourceSpan>,
    path: &str,
) -> SourceSpan {
    if let Some(span) = locations.get(path) {
        return *span;
    }
    if let Some((prefix, tail)) = path.split_once("/with/")
        && let Some((field, nested)) = tail.split_once('/')
        && let Some(span) = locations.get(&format!("{prefix}/with/{field}/literal/{nested}"))
    {
        return *span;
    }
    let mut ancestor = path;
    while let Some((parent, _)) = ancestor.rsplit_once('/') {
        if let Some(span) = locations.get(parent) {
            return *span;
        }
        ancestor = parent;
    }
    locations[""]
}
