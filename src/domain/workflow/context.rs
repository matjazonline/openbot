use super::{RunId, StepId};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::{self, Write};
use thiserror::Error;

pub const MAX_BINDING_DEPTH: usize = 64;
pub const MAX_BINDING_NODES: usize = 4096;
pub const MAX_POINTER_BYTES: usize = 1024;
pub const MAX_POINTER_SEGMENTS: usize = 64;
pub const MAX_JSON_DEPTH: usize = 64;
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_WORK_NODES: usize = 65536;

#[derive(Debug, Clone, PartialEq)]
pub enum Binding {
    Literal(Value),
    Reference(ContextReference),
    Object(BTreeMap<String, Binding>),
    Array(Vec<Binding>),
    Default {
        reference: ContextReference,
        fallback: Box<Binding>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextReference {
    raw: String,
    root: ReferenceRoot,
    path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReferenceRoot {
    Input,
    Params,
    Step(StepId),
    RunId,
    ParentRunId,
}

impl ContextReference {
    pub fn parse(pointer: impl AsRef<str>) -> Result<Self, ContextError> {
        let pointer = pointer.as_ref();
        let path = segments(pointer)?;
        let root = match path.first().map(String::as_str) {
            Some("input") => ReferenceRoot::Input,
            Some("params") => ReferenceRoot::Params,
            Some("steps") if path.len() >= 3 && path[2] == "output" => ReferenceRoot::Step(
                StepId::parse(&path[1])
                    .map_err(|_| ContextError::InvalidReference(pointer.into()))?,
            ),
            Some("run") if path.len() == 2 && path[1] == "id" => ReferenceRoot::RunId,
            Some("run") if path.len() == 2 && path[1] == "parent_id" => ReferenceRoot::ParentRunId,
            _ => return Err(ContextError::InvalidReference(pointer.into())),
        };
        Ok(Self {
            raw: pointer.into(),
            root,
            path,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ContextLimits {
    pub output_bytes: usize,
    pub work_nodes: usize,
}

impl Default for ContextLimits {
    fn default() -> Self {
        Self {
            output_bytes: MAX_OUTPUT_BYTES,
            work_nodes: MAX_WORK_NODES,
        }
    }
}

/// Metadata exposed to bindings; operational state and authorization data are absent.
#[derive(Debug, Clone, Copy)]
pub struct RunMetadata {
    pub run_id: RunId,
    pub parent_run_id: Option<RunId>,
}

pub struct Context<'a> {
    pub input: &'a Value,
    pub params: &'a Value,
    /// Only committed outputs may be supplied here.
    pub step_outputs: &'a BTreeMap<StepId, Value>,
    pub run: RunMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextError {
    #[error("missing reference {0}")]
    Missing(String),
    #[error("invalid reference {0}")]
    InvalidReference(String),
    #[error("context limit exceeded: {0}")]
    Limit(&'static str),
}

fn segments(pointer: &str) -> Result<Vec<String>, ContextError> {
    if pointer.len() > MAX_POINTER_BYTES {
        return Err(ContextError::Limit("pointer bytes"));
    }
    if !pointer.starts_with('/') {
        return Err(ContextError::InvalidReference(pointer.into()));
    }
    let raw: Vec<_> = pointer[1..].split('/').collect();
    if raw.len() > MAX_POINTER_SEGMENTS {
        return Err(ContextError::Limit("pointer segments"));
    }
    raw.into_iter()
        .map(|part| {
            let mut decoded = String::with_capacity(part.len());
            let mut chars = part.chars();
            while let Some(ch) = chars.next() {
                if ch == '~' {
                    match chars.next() {
                        Some('0') => decoded.push('~'),
                        Some('1') => decoded.push('/'),
                        _ => return Err(ContextError::InvalidReference(pointer.into())),
                    }
                } else {
                    decoded.push(ch);
                }
            }
            Ok(decoded)
        })
        .collect()
}

fn lookup<'a>(value: &'a Value, path: &[String], pointer: &str) -> Result<&'a Value, ContextError> {
    let mut value = value;
    for part in path {
        value = match value {
            Value::Object(map) => map.get(part),
            Value::Array(items) => {
                if part.is_empty()
                    || (part.len() > 1 && part.starts_with('0'))
                    || !part.bytes().all(|c| c.is_ascii_digit())
                {
                    return Err(ContextError::InvalidReference(pointer.into()));
                }
                let index = part
                    .parse::<usize>()
                    .map_err(|_| ContextError::InvalidReference(pointer.into()))?;
                items.get(index)
            }
            _ => return Err(ContextError::InvalidReference(pointer.into())),
        }
        .ok_or_else(|| ContextError::Missing(pointer.into()))?;
    }
    Ok(value)
}

fn reference<'a>(
    pointer: &ContextReference,
    context: &'a Context<'_>,
) -> Result<Cow<'a, Value>, ContextError> {
    let path = &pointer.path;
    let value = match &pointer.root {
        ReferenceRoot::Input => lookup(context.input, &path[1..], pointer.as_str())?,
        ReferenceRoot::Params => lookup(context.params, &path[1..], pointer.as_str())?,
        ReferenceRoot::Step(id) => {
            let output = context
                .step_outputs
                .get(id)
                .ok_or_else(|| ContextError::Missing(pointer.as_str().into()))?;
            lookup(output, &path[3..], pointer.as_str())?
        }
        ReferenceRoot::RunId => {
            return Ok(Cow::Owned(Value::String(context.run.run_id.to_string())));
        }
        ReferenceRoot::ParentRunId => {
            return Ok(Cow::Owned(
                context
                    .run
                    .parent_run_id
                    .map_or(Value::Null, |id| Value::String(id.to_string())),
            ));
        }
    };
    Ok(Cow::Borrowed(value))
}

struct CappedWriter {
    remaining: usize,
}
impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("output limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn check_size(value: &Value, remaining: &mut usize) -> Result<(), ContextError> {
    let mut writer = CappedWriter {
        remaining: *remaining,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| ContextError::Limit("output bytes"))?;
    *remaining = writer.remaining;
    Ok(())
}

fn ensure_pending_capacity(
    pending: usize,
    children: usize,
    remaining: usize,
    limit: &'static str,
) -> Result<(), ContextError> {
    if pending.saturating_add(children) > remaining {
        return Err(ContextError::Limit(limit));
    }
    Ok(())
}

fn check_value(value: &Value, budget: &mut usize, max_depth: usize) -> Result<(), ContextError> {
    let mut stack = vec![(value, 0)];
    while let Some((value, depth)) = stack.pop() {
        *budget = budget
            .checked_sub(1)
            .ok_or(ContextError::Limit("work nodes"))?;
        if depth > max_depth {
            return Err(ContextError::Limit("JSON depth"));
        }
        match value {
            Value::Object(map) => {
                ensure_pending_capacity(stack.len(), map.len(), *budget, "work nodes")?;
                stack.extend(map.values().map(|child| (child, depth + 1)));
            }
            Value::Array(items) => {
                ensure_pending_capacity(stack.len(), items.len(), *budget, "work nodes")?;
                stack.extend(items.iter().map(|child| (child, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn validate_output(value: &Value, limits: ContextLimits) -> Result<(), ContextError> {
    let mut work_budget = limits.work_nodes.min(MAX_WORK_NODES);
    check_value(value, &mut work_budget, MAX_JSON_DEPTH)?;
    let mut byte_budget = limits.output_bytes.min(MAX_OUTPUT_BYTES);
    check_size(value, &mut byte_budget)
}

fn check_binding(
    binding: &Binding,
    budget: &mut usize,
    key_bytes: &mut usize,
) -> Result<(), ContextError> {
    let mut stack = vec![(binding, 0)];
    while let Some((binding, depth)) = stack.pop() {
        *budget = budget
            .checked_sub(1)
            .ok_or(ContextError::Limit("binding nodes"))?;
        if depth > MAX_BINDING_DEPTH {
            return Err(ContextError::Limit("binding depth"));
        }
        match binding {
            Binding::Object(map) => {
                ensure_pending_capacity(stack.len(), map.len(), *budget, "binding nodes")?;
                for (key, child) in map {
                    *key_bytes = key_bytes
                        .checked_sub(key.len())
                        .ok_or(ContextError::Limit("output bytes"))?;
                    stack.push((child, depth + 1));
                }
            }
            Binding::Array(items) => {
                ensure_pending_capacity(stack.len(), items.len(), *budget, "binding nodes")?;
                stack.extend(items.iter().map(|child| (child, depth + 1)));
            }
            Binding::Default { fallback, .. } => stack.push((fallback, depth + 1)),
            _ => {}
        }
    }
    Ok(())
}

fn evaluate(
    binding: &Binding,
    context: &Context<'_>,
    budget: &mut usize,
    bytes: &mut usize,
    depth: usize,
) -> Result<Value, ContextError> {
    if depth > MAX_BINDING_DEPTH {
        return Err(ContextError::Limit("binding depth"));
    }
    match binding {
        Binding::Literal(value) => {
            check_value(value, budget, MAX_JSON_DEPTH.saturating_sub(depth))?;
            check_size(value, bytes)?;
            Ok(value.clone())
        }
        Binding::Reference(pointer) => {
            let value = reference(pointer, context)?;
            check_value(&value, budget, MAX_JSON_DEPTH.saturating_sub(depth))?;
            check_size(&value, bytes)?;
            Ok(value.into_owned())
        }
        Binding::Default {
            reference: pointer,
            fallback,
        } => match reference(pointer, context) {
            Ok(value) => {
                check_value(&value, budget, MAX_JSON_DEPTH.saturating_sub(depth))?;
                check_size(&value, bytes)?;
                Ok(value.into_owned())
            }
            Err(ContextError::Missing(_)) => evaluate(fallback, context, budget, bytes, depth + 1),
            Err(error) => Err(error),
        },
        Binding::Object(fields) => {
            let mut output = Map::new();
            for (name, child) in fields {
                output.insert(
                    name.clone(),
                    evaluate(child, context, budget, bytes, depth + 1)?,
                );
            }
            Ok(Value::Object(output))
        }
        Binding::Array(items) => items
            .iter()
            .map(|child| evaluate(child, context, budget, bytes, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
    }
}

pub fn resolve(
    binding: &Binding,
    context: &Context<'_>,
    limits: ContextLimits,
) -> Result<Value, ContextError> {
    let output_limit = limits.output_bytes.min(MAX_OUTPUT_BYTES);
    let mut binding_budget = MAX_BINDING_NODES;
    let mut key_budget = output_limit;
    check_binding(binding, &mut binding_budget, &mut key_budget)?;
    let mut work_budget = limits.work_nodes.min(MAX_WORK_NODES);
    let mut byte_budget = output_limit;
    let value = evaluate(binding, context, &mut work_budget, &mut byte_budget, 0)?;
    check_value(&value, &mut work_budget, MAX_JSON_DEPTH)?;
    let mut final_budget = output_limit;
    check_size(&value, &mut final_budget)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;
    fn id(s: &str) -> StepId {
        StepId::parse(s).unwrap()
    }
    fn context<'a>(
        input: &'a Value,
        params: &'a Value,
        outputs: &'a BTreeMap<StepId, Value>,
    ) -> Context<'a> {
        Context {
            input,
            params,
            step_outputs: outputs,
            run: RunMetadata {
                run_id: RunId::new(Uuid::nil()),
                parent_run_id: None,
            },
        }
    }
    fn reference(path: &str) -> Binding {
        Binding::Reference(ContextReference::parse(path).unwrap())
    }

    #[test]
    fn typed_values_defaults_and_immutability() {
        let input = json!({"null":null,"number":3,"escaped/key":true});
        let params = json!(["a", "b"]);
        let outputs = BTreeMap::from([(id("first"), json!({"done":[1,2]}))]);
        let original = outputs.clone();
        let ctx = context(&input, &params, &outputs);
        let binding = Binding::Object(BTreeMap::from([
            (
                "null".into(),
                Binding::Default {
                    reference: ContextReference::parse("/input/null").unwrap(),
                    fallback: Box::new(Binding::Literal(json!("wrong"))),
                },
            ),
            (
                "missing".into(),
                Binding::Default {
                    reference: ContextReference::parse("/input/absent").unwrap(),
                    fallback: Box::new(reference("/params/1")),
                },
            ),
            ("value".into(), reference("/steps/first/output/done/0")),
            ("escaped".into(), reference("/input/escaped~1key")),
        ]));
        assert_eq!(
            resolve(&binding, &ctx, ContextLimits::default()).unwrap(),
            json!({"null":null,"missing":"b","value":1,"escaped":true})
        );
        assert_eq!(outputs, original);
    }

    #[test]
    fn malformed_paths_do_not_default() {
        let input = json!([1]);
        let params = json!({});
        let outputs = BTreeMap::new();
        let ctx = context(&input, &params, &outputs);
        for path in [
            "input/0",
            "/unknown/x",
            "/input/~2",
            "/steps/x/other",
            "/steps/1bad/output",
            "/run/private",
        ] {
            assert!(
                matches!(
                    ContextReference::parse(path),
                    Err(ContextError::InvalidReference(_))
                ),
                "{path}"
            );
        }
        for path in [
            "/input/00",
            "/input/9999999999999999999999999999999999999999",
        ] {
            let fallback = Binding::Default {
                reference: ContextReference::parse(path).unwrap(),
                fallback: Box::new(Binding::Literal(json!(1))),
            };
            assert!(
                matches!(
                    resolve(&fallback, &ctx, ContextLimits::default()),
                    Err(ContextError::InvalidReference(_))
                ),
                "{path}"
            );
        }
    }

    #[test]
    fn scalar_traversal_is_invalid_and_null_is_a_value() {
        let input = json!({"number": 4, "boolean": true, "string": "text", "null": null});
        let params = json!({});
        let outputs = BTreeMap::new();
        let ctx = context(&input, &params, &outputs);
        assert_eq!(
            resolve(&reference("/input/null"), &ctx, ContextLimits::default()).unwrap(),
            Value::Null
        );
        for field in ["number", "boolean", "string", "null"] {
            let path = format!("/input/{field}/child");
            let binding = Binding::Default {
                reference: ContextReference::parse(&path).unwrap(),
                fallback: Box::new(Binding::Literal(json!("fallback"))),
            };
            assert!(
                matches!(
                    resolve(&binding, &ctx, ContextLimits::default()),
                    Err(ContextError::InvalidReference(_))
                ),
                "{field}"
            );
        }
    }

    #[test]
    fn wide_inputs_are_rejected_before_pending_stack_growth() {
        let input = Value::Array(vec![Value::Null; MAX_WORK_NODES + 1]);
        let params = json!({});
        let outputs = BTreeMap::new();
        let ctx = context(&input, &params, &outputs);
        assert!(matches!(
            resolve(&reference("/input"), &ctx, ContextLimits::default()),
            Err(ContextError::Limit("work nodes"))
        ));
        let input = Value::Object(
            (0..MAX_WORK_NODES + 1)
                .map(|n| (format!("k{n}"), Value::Null))
                .collect(),
        );
        let ctx = context(&input, &params, &outputs);
        assert!(matches!(
            resolve(&reference("/input"), &ctx, ContextLimits::default()),
            Err(ContextError::Limit("work nodes"))
        ));
        let wide_array = Binding::Array(vec![Binding::Literal(Value::Null); MAX_BINDING_NODES]);
        assert!(matches!(
            resolve(&wide_array, &ctx, ContextLimits::default()),
            Err(ContextError::Limit("binding nodes"))
        ));
        let wide_object = Binding::Object(
            (0..MAX_BINDING_NODES)
                .map(|n| (format!("k{n}"), Binding::Literal(Value::Null)))
                .collect(),
        );
        assert!(matches!(
            resolve(&wide_object, &ctx, ContextLimits::default()),
            Err(ContextError::Limit("binding nodes"))
        ));
    }

    #[test]
    fn depth_work_and_output_budgets() {
        let mut deep = json!(0);
        for _ in 0..65 {
            deep = Value::Array(vec![deep]);
        }
        let params = json!({});
        let outputs = BTreeMap::new();
        let ctx = context(&deep, &params, &outputs);
        assert!(matches!(
            resolve(&reference("/input"), &ctx, ContextLimits::default()),
            Err(ContextError::Limit("JSON depth"))
        ));
        let input = json!({"value":[1,2,3]});
        let ctx = context(&input, &params, &outputs);
        let many = Binding::Array((0..100).map(|_| reference("/input/value")).collect());
        assert!(matches!(
            resolve(
                &many,
                &ctx,
                ContextLimits {
                    output_bytes: MAX_OUTPUT_BYTES,
                    work_nodes: 100
                }
            ),
            Err(ContextError::Limit("work nodes"))
        ));
        assert!(matches!(
            resolve(
                &many,
                &ctx,
                ContextLimits {
                    output_bytes: 10,
                    work_nodes: MAX_WORK_NODES
                }
            ),
            Err(ContextError::Limit("output bytes"))
        ));
    }

    #[test]
    fn exact_binding_and_pointer_limits() {
        let input = json!(0);
        let params = json!({});
        let outputs = BTreeMap::new();
        let ctx = context(&input, &params, &outputs);
        let mut nested = Binding::Literal(json!(1));
        for _ in 0..MAX_BINDING_DEPTH {
            nested = Binding::Array(vec![nested]);
        }
        assert!(resolve(&nested, &ctx, ContextLimits::default()).is_ok());
        let too_deep = Binding::Array(vec![nested]);
        assert!(matches!(
            resolve(&too_deep, &ctx, ContextLimits::default()),
            Err(ContextError::Limit("binding depth"))
        ));
        let exact = Binding::Array(
            (0..MAX_BINDING_NODES - 1)
                .map(|_| Binding::Literal(Value::Null))
                .collect(),
        );
        assert!(resolve(&exact, &ctx, ContextLimits::default()).is_ok());
        let excessive = Binding::Array(
            (0..MAX_BINDING_NODES)
                .map(|_| Binding::Literal(Value::Null))
                .collect(),
        );
        assert!(matches!(
            resolve(&excessive, &ctx, ContextLimits::default()),
            Err(ContextError::Limit("binding nodes"))
        ));
        let many_segments = format!("/input{}", "/x".repeat(MAX_POINTER_SEGMENTS));
        assert!(matches!(
            ContextReference::parse(&many_segments),
            Err(ContextError::Limit("pointer segments"))
        ));
        let long_pointer = format!("/input/{}", "x".repeat(MAX_POINTER_BYTES));
        assert!(matches!(
            ContextReference::parse(&long_pointer),
            Err(ContextError::Limit("pointer bytes"))
        ));
        let exact_output = Binding::Literal(Value::String("x".repeat(MAX_OUTPUT_BYTES - 2)));
        assert!(resolve(&exact_output, &ctx, ContextLimits::default()).is_ok());
        let oversized_key = Binding::Object(BTreeMap::from([(
            "x".repeat(MAX_OUTPUT_BYTES + 1),
            Binding::Literal(Value::Null),
        )]));
        assert!(matches!(
            resolve(&oversized_key, &ctx, ContextLimits::default()),
            Err(ContextError::Limit("output bytes"))
        ));
    }
}
