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
    Concat(Vec<Binding>),
    Compare {
        op: Comparison,
        left: Box<Binding>,
        right: Box<Binding>,
    },
    In {
        value: Box<Binding>,
        items: Box<Binding>,
    },
    Exists(ContextReference),
    And(Vec<Binding>),
    Or(Vec<Binding>),
    Not(Box<Binding>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
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

    pub fn step_id(&self) -> Option<&StepId> {
        match &self.root {
            ReferenceRoot::Step(id) => Some(id),
            _ => None,
        }
    }

    pub fn tail(&self) -> &[String] {
        match self.root {
            ReferenceRoot::Input | ReferenceRoot::Params => &self.path[1..],
            ReferenceRoot::Step(_) => &self.path[3..],
            ReferenceRoot::RunId | ReferenceRoot::ParentRunId => &[],
        }
    }

    pub fn root_name(&self) -> &'static str {
        match self.root {
            ReferenceRoot::Input => "input",
            ReferenceRoot::Params => "params",
            ReferenceRoot::Step(_) => "steps",
            ReferenceRoot::RunId => "run.id",
            ReferenceRoot::ParentRunId => "run.parent_id",
        }
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
    #[error("binding operand type mismatch: {0}")]
    Operand(&'static str),
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
                let index = canonical_array_index(part)
                    .ok_or_else(|| ContextError::InvalidReference(pointer.into()))?;
                items.get(index)
            }
            _ => return Err(ContextError::InvalidReference(pointer.into())),
        }
        .ok_or_else(|| ContextError::Missing(pointer.into()))?;
    }
    Ok(value)
}

pub fn canonical_array_index(segment: &str) -> Option<usize> {
    if segment.is_empty()
        || segment.len() > 1 && segment.starts_with('0')
        || !segment.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    segment.parse().ok()
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

pub fn validate_context_value(value: &Value, limits: ContextLimits) -> Result<(), ContextError> {
    let mut work_budget = limits.work_nodes.min(MAX_WORK_NODES);
    check_value(value, &mut work_budget, MAX_JSON_DEPTH)?;
    let mut byte_budget = limits.output_bytes.min(MAX_OUTPUT_BYTES);
    check_size(value, &mut byte_budget)
}

pub(super) fn validate_output(value: &Value, limits: ContextLimits) -> Result<(), ContextError> {
    validate_context_value(value, limits)
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
            Binding::Literal(value) => {
                // The binding node was already charged; count literal descendants.
                let mut value_budget = budget.saturating_add(1);
                check_value(
                    value,
                    &mut value_budget,
                    MAX_JSON_DEPTH.saturating_sub(depth),
                )?;
                *budget = value_budget;
                // Each literal must fit the remaining space, but operators may
                // short circuit or combine parts into one result. Evaluation
                // charges the shared output budget for values actually used.
                let mut literal_budget = *key_bytes;
                check_size(value, &mut literal_budget)?;
            }
            Binding::Object(map) => {
                ensure_pending_capacity(stack.len(), map.len(), *budget, "binding nodes")?;
                for (key, child) in map {
                    *key_bytes = key_bytes
                        .checked_sub(key.len())
                        .ok_or(ContextError::Limit("output bytes"))?;
                    stack.push((child, depth + 1));
                }
            }
            Binding::Array(items)
            | Binding::Concat(items)
            | Binding::And(items)
            | Binding::Or(items) => {
                ensure_pending_capacity(stack.len(), items.len(), *budget, "binding nodes")?;
                stack.extend(items.iter().map(|child| (child, depth + 1)));
            }
            Binding::Default { fallback, .. } => stack.push((fallback, depth + 1)),
            Binding::Compare { left, right, .. } => {
                ensure_pending_capacity(stack.len(), 2, *budget, "binding nodes")?;
                stack.push((left, depth + 1));
                stack.push((right, depth + 1));
            }
            Binding::In { value, items } => {
                ensure_pending_capacity(stack.len(), 2, *budget, "binding nodes")?;
                stack.push((value, depth + 1));
                stack.push((items, depth + 1));
            }
            Binding::Not(child) => stack.push((child, depth + 1)),
            _ => {}
        }
    }
    Ok(())
}

pub fn preflight_binding(binding: &Binding, limits: ContextLimits) -> Result<(), ContextError> {
    let mut nodes = MAX_BINDING_NODES;
    let mut key_bytes = limits.output_bytes.min(MAX_OUTPUT_BYTES);
    check_binding(binding, &mut nodes, &mut key_bytes)
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
        Binding::Exists(pointer) => match reference(pointer, context) {
            Ok(_) => Ok(Value::Bool(true)),
            Err(ContextError::Missing(_)) => Ok(Value::Bool(false)),
            Err(error) => Err(error),
        },
        Binding::Concat(items) => eval_concat(items, context, budget, bytes, depth),
        Binding::Compare { op, left, right } => {
            let left = evaluate(left, context, budget, bytes, depth + 1)?;
            let right = evaluate(right, context, budget, bytes, depth + 1)?;
            Ok(Value::Bool(super::expression::compare(*op, &left, &right)?))
        }
        Binding::In { value, items } => {
            eval_membership(value, items, context, budget, bytes, depth)
        }
        Binding::And(items) | Binding::Or(items) => eval_boolean(
            items,
            matches!(binding, Binding::And(_)),
            context,
            budget,
            bytes,
            depth,
        ),
        Binding::Not(item) => {
            let Value::Bool(value) = evaluate(item, context, budget, bytes, depth + 1)? else {
                return Err(ContextError::Operand("not expects a boolean"));
            };
            Ok(Value::Bool(!value))
        }
    }
}

fn eval_concat(
    items: &[Binding],
    context: &Context<'_>,
    budget: &mut usize,
    bytes: &mut usize,
    depth: usize,
) -> Result<Value, ContextError> {
    let mut result = String::new();
    for item in items {
        let mut part_budget = *bytes;
        let Value::String(part) = evaluate(item, context, budget, &mut part_budget, depth + 1)?
        else {
            return Err(ContextError::Operand("concat expects strings"));
        };
        if result.len().saturating_add(part.len()) > *bytes {
            return Err(ContextError::Limit("output bytes"));
        }
        result.push_str(&part);
    }
    let value = Value::String(result);
    check_size(&value, bytes)?;
    Ok(value)
}

fn eval_membership(
    value: &Binding,
    items: &Binding,
    context: &Context<'_>,
    budget: &mut usize,
    bytes: &mut usize,
    depth: usize,
) -> Result<Value, ContextError> {
    let value = evaluate(value, context, budget, bytes, depth + 1)?;
    let Value::Array(items) = evaluate(items, context, budget, bytes, depth + 1)? else {
        return Err(ContextError::Operand("in expects an array"));
    };
    Ok(Value::Bool(
        items
            .iter()
            .any(|item| super::expression::equal(item, &value)),
    ))
}

fn eval_boolean(
    items: &[Binding],
    and: bool,
    context: &Context<'_>,
    budget: &mut usize,
    bytes: &mut usize,
    depth: usize,
) -> Result<Value, ContextError> {
    for item in items {
        let Value::Bool(value) = evaluate(item, context, budget, bytes, depth + 1)? else {
            return Err(ContextError::Operand("boolean operator expects booleans"));
        };
        if value != and {
            return Ok(Value::Bool(!and));
        }
    }
    Ok(Value::Bool(and))
}

pub fn resolve(
    binding: &Binding,
    context: &Context<'_>,
    limits: ContextLimits,
) -> Result<Value, ContextError> {
    let output_limit = limits.output_bytes.min(MAX_OUTPUT_BYTES);
    preflight_binding(binding, limits)?;
    let mut work_budget = limits.work_nodes.min(MAX_WORK_NODES);
    let mut byte_budget = output_limit;
    let value = evaluate(binding, context, &mut work_budget, &mut byte_budget, 0)?;
    check_value(&value, &mut work_budget, MAX_JSON_DEPTH)?;
    let mut final_budget = output_limit;
    check_size(&value, &mut final_budget)?;
    Ok(value)
}

/// Resolve all named step inputs against one budget before the caller persists the snapshot.
/// The optional name identifies the failing field; None denotes the aggregate object.
pub fn resolve_inputs<'a>(
    bindings: &'a BTreeMap<String, Binding>,
    context: &Context<'_>,
    limits: ContextLimits,
) -> Result<Value, (Option<&'a str>, ContextError)> {
    let output_limit = limits.output_bytes.min(MAX_OUTPUT_BYTES);
    let mut preflight_nodes = MAX_BINDING_NODES;
    let mut preflight_bytes = output_limit
        .checked_sub(2)
        .ok_or((None, ContextError::Limit("output bytes")))?;
    for (index, (name, binding)) in bindings.iter().enumerate() {
        let punctuation = usize::from(index > 0) + 1;
        preflight_bytes = preflight_bytes
            .checked_sub(punctuation)
            .ok_or((Some(name.as_str()), ContextError::Limit("output bytes")))?;
        let mut writer = CappedWriter {
            remaining: preflight_bytes,
        };
        serde_json::to_writer(&mut writer, name)
            .map_err(|_| (Some(name.as_str()), ContextError::Limit("output bytes")))?;
        preflight_bytes = writer.remaining;
        check_binding(binding, &mut preflight_nodes, &mut preflight_bytes)
            .map_err(|error| (Some(name.as_str()), error))?;
    }

    let mut work_budget = limits.work_nodes.min(MAX_WORK_NODES);
    let mut byte_budget = output_limit - 2;
    let mut values = Map::new();
    for (index, (name, binding)) in bindings.iter().enumerate() {
        byte_budget = byte_budget
            .checked_sub(usize::from(index > 0) + 1)
            .ok_or((Some(name.as_str()), ContextError::Limit("output bytes")))?;
        let mut writer = CappedWriter {
            remaining: byte_budget,
        };
        serde_json::to_writer(&mut writer, name)
            .map_err(|_| (Some(name.as_str()), ContextError::Limit("output bytes")))?;
        byte_budget = writer.remaining;
        let value = evaluate(binding, context, &mut work_budget, &mut byte_budget, 0)
            .map_err(|error| (Some(name.as_str()), error))?;
        values.insert(name.clone(), value);
    }
    let value = Value::Object(values);
    validate_context_value(&value, limits).map_err(|error| (None, error))?;
    Ok(value)
}

#[cfg(test)]
mod tests;
