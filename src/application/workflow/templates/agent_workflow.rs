//! The workflow every agent channel runs: three base steps, and the slots where one workflow adds
//! its own. The base is the autonomous-response template as it ships: `context` (the thread
//! history) → `draft` (the agent) → `reply`. A reviewed channel is the base with a `review` gate
//! before the reply. Custom steps go into the same slots; the base's own steps are never edited.
//! Another history implementation is a change to the base's `context` step, which the composer
//! takes as it finds it.
use super::REPLY_AGENT_STEP;
use crate::application::workflow::compiler::{
    Diagnostic, LocatedNode, NodeValue, SourceDecoder, SourceNode, json_value, scalar_string,
};
use crate::domain::workflow::StepId;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The base step whose output reaches the agent as `untrusted_history`.
pub const HISTORY_STEP: &str = "context";
pub const REPLY_STEP: &str = "reply";
/// The gate a reviewed channel adds before its reply.
pub const REVIEW_STEP: &str = "review";
const REVIEW_REJECTED_STEP: &str = "review_rejected";
const FINISH_RESPONSE_STEP: &str = "finish_response";
/// The route target a custom step names for whichever step follows it.
pub const NEXT_STEP: &str = "$next";
const END_STEP: &str = "$end";
/// The three base steps, the review gate and these fit the base's `max_steps` of 10.
pub const MAX_CUSTOM_STEPS: usize = 6;

/// Where a custom step runs. A step before the draft also hands its output to the agent, as
/// `untrusted_<id>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepSlot {
    BeforeDraft,
    BeforeReply,
    AfterReply,
}

/// One step a workflow adds to the base: its definition (`type`, `with`, `routes` and any other
/// step field, such as a rule). Its routes name only `$next` or `$end`, so the base keeps its
/// order; they default to `{success: $next}`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CustomStep {
    pub id: String,
    pub slot: StepSlot,
    #[serde(flatten)]
    pub definition: Map<String, Value>,
}

/// The base and what one workflow adds to it, composed against the base source.
#[derive(Debug, Clone, Default)]
pub struct AgentWorkflow {
    reviewed: bool,
    steps: Vec<CustomStep>,
}

impl AgentWorkflow {
    pub fn base() -> Self {
        Self::default()
    }

    /// A `review` gate before the reply: the run's `reviewer` approves, edits or rejects the
    /// draft, and the reply sends what they approved.
    pub fn reviewed(mut self) -> Self {
        self.reviewed = true;
        self
    }

    pub fn with_steps(mut self, steps: Vec<CustomStep>) -> Self {
        self.steps = steps;
        self
    }

    pub fn compose(
        &self,
        decoder: &impl SourceDecoder,
        base: &str,
    ) -> Result<SourceNode, Diagnostic> {
        let decoded = decoder.decode(base)?;
        let root = entries(&decoded.root, "")?;
        let schema = self.response_schema(&root)?;
        let mut base_steps = BaseSteps::read(&root)?;
        self.check_steps()?;
        let custom = |slot| self.steps.iter().filter(move |step| step.slot == slot);

        let history = base_steps.take(HISTORY_STEP)?;
        let mut draft = base_steps.take(REPLY_AGENT_STEP)?;
        let mut reply = base_steps.take(REPLY_STEP)?;
        base_steps.finish()?;

        let mut chain = vec![Link::base(HISTORY_STEP, history)];
        for step in custom(StepSlot::BeforeDraft) {
            hand_to_agent(&mut draft, &step.id)?;
            chain.push(Link::custom(step));
        }
        let mut answer = format!("/steps/{REPLY_AGENT_STEP}/output");
        let gate = self
            .reviewed
            .then(|| review_gate(&draft, &answer))
            .transpose()?;
        chain.push(Link::base(REPLY_AGENT_STEP, draft));
        chain.extend(custom(StepSlot::BeforeReply).map(Link::custom));
        if let Some(gate) = gate {
            chain.push(Link {
                id: REVIEW_STEP.into(),
                step: gate,
                routes: Routes::Review,
            });
            answer = format!("/steps/{REVIEW_STEP}/output/data");
        }
        reply_with(&mut reply, &answer)?;
        chain.push(Link::base(REPLY_STEP, reply));
        let steps = self.complete_chain(chain, &schema, &root)?;
        let entry = steps.first().map(|(id, _)| id.clone()).unwrap_or_default();
        root.into_iter()
            .map(|(key, node)| {
                let value = match key.as_str() {
                    "entry" => SourceNode::Value(Value::String(entry.clone())),
                    "steps" => SourceNode::Mapping(steps.clone()),
                    "output_schema" => SourceNode::Value(schema.clone()),
                    "parameter_schema" if self.reviewed => {
                        SourceNode::Value(with_reviewer(json_value(node, "/parameter_schema")?)?)
                    }
                    _ => SourceNode::Value(json_value(node, &format!("/{key}"))?),
                };
                Ok((key, value))
            })
            .collect::<Result<_, _>>()
            .map(SourceNode::Mapping)
    }

    fn complete_chain(
        &self,
        mut chain: Vec<Link>,
        schema: &Value,
        root: &[(String, &LocatedNode)],
    ) -> Result<Vec<(String, SourceNode)>, Diagnostic> {
        let tail: Vec<_> = self
            .steps
            .iter()
            .filter(|step| step.slot == StepSlot::AfterReply)
            .collect();
        chain.extend(tail.iter().map(|step| Link::custom(step)));
        if !tail.is_empty()
            && !tail
                .last()
                .is_some_and(|step| step.definition["type"] == "channel.no_reply")
        {
            chain.push(Link::base(
                FINISH_RESPONSE_STEP,
                response_map(&format!("/steps/{REPLY_STEP}/output"), schema),
            ));
        }
        let limit = root
            .iter()
            .find(|(key, _)| key == "limits")
            .map(|(_, node)| json_value(node, "/limits"))
            .transpose()?
            .and_then(|limits| limits.get("max_steps").and_then(Value::as_u64))
            .ok_or_else(|| {
                invalid(
                    "/limits/max_steps",
                    "The base needs an explicit step budget",
                )
            })?;
        if chain.len() as u64 > limit {
            return Err(invalid(
                "/steps",
                "The composed response and custom steps exceed the base step budget; reduce custom steps",
            ));
        }
        let mut steps = route(chain)?;
        if self.reviewed {
            let rejected = json!({"type":"channel.no_reply","with":{
                "value":{"object":{"body":{"literal":""},"review":{"ref":"/steps/review/output"}}},
                "output_schema":{"literal":schema}},"routes":{"success":"$end"}});
            steps.push((
                REVIEW_REJECTED_STEP.into(),
                ordered(rejected.as_object().cloned().expect("object")),
            ));
        }
        Ok(steps)
    }

    fn response_schema(&self, root: &[(String, &LocatedNode)]) -> Result<Value, Diagnostic> {
        let node = root
            .iter()
            .find(|(key, _)| key == "output_schema")
            .map(|(_, node)| *node)
            .ok_or_else(|| invalid("/output_schema", "The base needs a response schema"))?;
        let mut schema = json_value(node, "/output_schema")?;
        if self.reviewed {
            schema
                .get_mut("properties")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    invalid(
                        "/output_schema",
                        "The base response schema needs object properties",
                    )
                })?
                .insert("review".into(), json!({"type":"object"}));
        }
        Ok(schema)
    }

    fn check_steps(&self) -> Result<(), Diagnostic> {
        if self.steps.len() > MAX_CUSTOM_STEPS {
            return Err(invalid(
                "/steps",
                &format!("At most {MAX_CUSTOM_STEPS} custom steps are allowed"),
            ));
        }
        let mut seen = BTreeSet::from([
            HISTORY_STEP,
            REPLY_AGENT_STEP,
            REPLY_STEP,
            REVIEW_STEP,
            REVIEW_REJECTED_STEP,
            FINISH_RESPONSE_STEP,
        ]);
        for step in &self.steps {
            let at = format!("/steps/{}", step.id);
            if StepId::parse(&step.id).is_err() {
                return Err(invalid(&at, "A custom step id is not a valid step id"));
            }
            if !seen.insert(&step.id) {
                return Err(invalid(
                    &at,
                    "A custom step id repeats a step of the workflow",
                ));
            }
            if !step.definition.get("type").is_some_and(Value::is_string) {
                return Err(invalid(&at, "A custom step needs a type"));
            }
            if step.definition["type"] != "channel.no_reply"
                && let Some(routes) = step.definition.get("routes")
            {
                reject_custom_end(routes, &format!("{at}/routes"))?;
            }
        }
        Ok(())
    }
}

/// The base source's steps by id, each taken once; any left over is not a base step.
struct BaseSteps(BTreeMap<String, Map<String, Value>>);
impl BaseSteps {
    fn read(root: &[(String, &LocatedNode)]) -> Result<Self, Diagnostic> {
        let (_, steps) = root
            .iter()
            .find(|(key, _)| key == "steps")
            .ok_or_else(|| invalid("/steps", "The base has no steps"))?;
        entries(steps, "/steps")?
            .into_iter()
            .map(|(id, node)| {
                let at = format!("/steps/{id}");
                match json_value(node, &at)? {
                    Value::Object(step) => Ok((id, step)),
                    _ => Err(invalid(&at, "A base step is not a mapping")),
                }
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    fn take(&mut self, id: &str) -> Result<Map<String, Value>, Diagnostic> {
        self.0
            .remove(id)
            .ok_or_else(|| invalid(&format!("/steps/{id}"), "The base lacks this step"))
    }

    fn finish(self) -> Result<(), Diagnostic> {
        match self.0.keys().next() {
            Some(id) => Err(invalid(
                &format!("/steps/{id}"),
                "The base has an extra step",
            )),
            None => Ok(()),
        }
    }
}

/// One step of the composed chain and how its routes reach the next one.
struct Link {
    id: String,
    step: Map<String, Value>,
    routes: Routes,
}
enum Routes {
    /// A base step continues on success.
    Base,
    /// The review gate continues on approval and ends on rejection.
    Review,
    /// A custom step's own routes, with `$next` resolved.
    Custom,
}
impl Link {
    fn base(id: &str, step: Map<String, Value>) -> Self {
        Self {
            id: id.into(),
            step,
            routes: Routes::Base,
        }
    }
    fn custom(step: &CustomStep) -> Self {
        Self {
            id: step.id.clone(),
            step: step.definition.clone(),
            routes: Routes::Custom,
        }
    }
}

/// Each step's routes, written against the step that follows it, and each step's fields in
/// reading order: `type`, `with`, any other field, then `routes`.
fn route(chain: Vec<Link>) -> Result<Vec<(String, SourceNode)>, Diagnostic> {
    let next: Vec<String> = chain
        .iter()
        .skip(1)
        .map(|link| link.id.clone())
        .chain([END_STEP.into()])
        .collect();
    chain
        .into_iter()
        .zip(next)
        .map(|(mut link, next)| {
            let routes = match link.routes {
                Routes::Base => json!({"success": next}),
                Routes::Review => {
                    json!({"choices": {"approve": next, "reject": REVIEW_REJECTED_STEP}})
                }
                Routes::Custom => {
                    let mut routes = link
                        .step
                        .remove("routes")
                        .unwrap_or_else(|| json!({"success": NEXT_STEP}));
                    resolve_next(&mut routes, &next, &format!("/steps/{}/routes", link.id))?;
                    routes
                }
            };
            link.step.insert("routes".into(), routes);
            Ok((link.id, ordered(link.step)))
        })
        .collect()
}

fn resolve_next(routes: &mut Value, next: &str, at: &str) -> Result<(), Diagnostic> {
    match routes {
        Value::String(target) if target == NEXT_STEP => {
            *target = next.into();
            Ok(())
        }
        Value::String(target) if target == END_STEP => Ok(()),
        Value::Object(map) => map
            .values_mut()
            .try_for_each(|value| resolve_next(value, next, at)),
        _ => Err(invalid(at, "A custom step routes only to $next or $end")),
    }
}

fn ordered(mut step: Map<String, Value>) -> SourceNode {
    let mut fields = Vec::with_capacity(step.len());
    for key in ["type", "with"] {
        if let Some(value) = step.remove(key) {
            fields.push((key.to_owned(), SourceNode::Value(value)));
        }
    }
    let routes = step.remove("routes");
    fields.extend(
        step.into_iter()
            .map(|(key, value)| (key, SourceNode::Value(value))),
    );
    if let Some(routes) = routes {
        fields.push(("routes".into(), SourceNode::Value(routes)));
    }
    SourceNode::Mapping(fields)
}

/// The draft's context gains a step's output, beside the message, subject and history.
fn hand_to_agent(draft: &mut Map<String, Value>, step: &str) -> Result<(), Diagnostic> {
    let at = format!("/steps/{REPLY_AGENT_STEP}/with/context/object");
    let context = draft
        .get_mut("with")
        .and_then(|with| with.get_mut("context"))
        .and_then(|context| context.get_mut("object"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid(&at, "The base draft context is not an object binding"))?;
    let key = format!("untrusted_{step}");
    if context.contains_key(&key) {
        return Err(invalid(
            &format!("/steps/{step}"),
            "A custom step's output would replace part of the agent's context",
        ));
    }
    context.insert(key, json!({"ref": format!("/steps/{step}/output")}));
    Ok(())
}

/// The reviewer sees the current answer and edits it within the draft's own output schema.
fn review_gate(draft: &Map<String, Value>, answer: &str) -> Result<Map<String, Value>, Diagnostic> {
    let data_schema = draft
        .get("with")
        .and_then(|with| with.get("output_schema"))
        .cloned()
        .ok_or_else(|| {
            invalid(
                &format!("/steps/{REPLY_AGENT_STEP}/with/output_schema"),
                "The base draft has no output schema",
            )
        })?;
    let Value::Object(gate) = json!({
        "type": "decision.human",
        "with": {
            "proposal": {"ref": answer},
            "reviewers": {"array": [{"object": {
                "kind": {"literal": "principal"}, "id": {"ref": "/params/reviewer"}}}]},
            "deadline": {"ref": "/run/deadline"},
            "data_schema": data_schema,
            "feedback_required": {"literal": ["reject"]}
        }
    }) else {
        unreachable!("the gate is an object")
    };
    Ok(gate)
}

fn reply_with(reply: &mut Map<String, Value>, answer: &str) -> Result<(), Diagnostic> {
    let content = reply
        .get_mut("with")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            invalid(
                &format!("/steps/{REPLY_STEP}/with"),
                "The base reply has no inputs",
            )
        })?;
    content.insert("value".into(), json!({"ref": answer}));
    Ok(())
}

fn reject_custom_end(routes: &Value, at: &str) -> Result<(), Diagnostic> {
    match routes {
        Value::String(target) if target == END_STEP => Err(invalid(
            at,
            "Use a terminal channel.no_reply (Finish without reply) for intentional silence; custom steps cannot end the native response directly",
        )),
        Value::Object(routes) => routes
            .values()
            .try_for_each(|route| reject_custom_end(route, at)),
        _ => Ok(()),
    }
}

fn response_map(answer: &str, schema: &Value) -> Map<String, Value> {
    json!({"type":"data.map","with":{"value":{"ref":answer},"output_schema":{"literal":schema}}})
        .as_object()
        .cloned()
        .expect("object")
}

/// The base's parameters plus the reviewer the review gate names.
fn with_reviewer(mut schema: Value) -> Result<Value, Diagnostic> {
    let missing = || {
        invalid(
            "/parameter_schema",
            "The base parameter schema is not an object",
        )
    };
    let object = schema.as_object_mut().ok_or_else(missing)?;
    object
        .entry("properties")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(missing)?
        .insert(
            "reviewer".into(),
            json!({"type": "string", "maxLength": 128}),
        );
    let required = object
        .entry("required")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(missing)?;
    if !required.iter().any(|name| name == "reviewer") {
        required.push(json!("reviewer"));
    }
    Ok(schema)
}

fn entries<'a>(
    node: &'a LocatedNode,
    path: &str,
) -> Result<Vec<(String, &'a LocatedNode)>, Diagnostic> {
    let NodeValue::Mapping(pairs) = &node.value else {
        return Err(Diagnostic::at(
            "syntax.mapping",
            "Expected a mapping",
            path,
            node.span,
        ));
    };
    pairs
        .iter()
        .map(|(key, value)| Ok((scalar_string(key, path)?.to_owned(), value)))
        .collect()
}

fn invalid(path: &str, message: &str) -> Diagnostic {
    Diagnostic::at(
        "compose.agent_workflow",
        message,
        path,
        crate::application::workflow::compiler::SourceSpan {
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        },
    )
}

#[cfg(test)]
#[path = "agent_workflow_tests.rs"]
mod tests;
