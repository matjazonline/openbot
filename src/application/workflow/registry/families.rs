use super::*;
impl Registration<'_> {
    pub(super) fn basic(&mut self) -> Result<Contract, Diagnostic> {
        match self.name() {
            "context.load" => Ok(contract(
                json!({"sources":array(identifier(), 1, MAX_SELECTIONS),"max_tokens":integer(MAX_CONTEXT_TOKENS)}),
                &["sources", "max_tokens"],
                object(
                    json!({"items":array(provenance_item(),0,MAX_ITEMS),"token_count":{"type":"integer","minimum":0,"maximum":MAX_CONTEXT_TOKENS}}),
                    &["items", "token_count"],
                ),
            )),
            "memory.load" => Ok(contract(
                json!({"scope":scope(),"query":text(MAX_TEXT),"limit":integer(MAX_ITEMS)}),
                &["scope", "query", "limit"],
                object(
                    json!({"items":array(provenance_item(),0,MAX_ITEMS)}),
                    &["items"],
                ),
            )),
            "memory.save" => Ok(contract(
                json!({"scope":scope(),"facts":array(text(MAX_TEXT),1,MAX_ITEMS)}),
                &["scope", "facts"],
                object(
                    json!({"saved_ids":array(identifier(),1,MAX_ITEMS)}),
                    &["saved_ids"],
                ),
            )),
            "ai.classify" | "agent.run" | "data.map" => self.schema_output(),
            "decision.rule" | "decision.human" | "decision.agent" => self.decision(),
            "wait.event" | "wait.timer" => self.wait(),
            _ => unreachable!("basic registration family"),
        }
    }
    fn schema_output(&mut self) -> Result<Contract, Diagnostic> {
        let schema = self.declared_schema("output_schema")?;
        let declaration = json!({"const":schema});
        let result = match self.name() {
            "data.map" => contract(
                json!({"value":embedded(schema.clone(),"mapped"),"output_schema":declaration}),
                &["value", "output_schema"],
                schema,
            ),
            "ai.classify" => {
                if !finite_labels(&schema) {
                    return Err(self.error(
                        "registry.labels",
                        "Declare a finite scalar enum or a label/profile enum property",
                        "output_schema",
                    ));
                }
                contract(
                    json!({"context":true,"output_schema":declaration}),
                    &["context", "output_schema"],
                    schema,
                )
            }
            "agent.run" => {
                let profiles: BTreeMap<_,_> = self.facts.profiles.iter().map(|p| (p.name.to_string(),json!({"tools":p.tools.iter().map(TypeName::as_str).collect::<Vec<_>>(),"skills":p.skills.iter().map(TypeName::as_str).collect::<Vec<_>>()}))).collect();
                self.effective["profiles"] = json!(profiles);
                contract(
                    json!({"agent":identifier(),"context":true,"output_schema":declaration,"capability_profile":profile(self.facts)}),
                    &["agent", "context", "output_schema"],
                    schema,
                )
            }
            _ => unreachable!("schema output family"),
        };
        Ok(result)
    }
    fn decision(&mut self) -> Result<Contract, Diagnostic> {
        let schema = self.declared_schema("data_schema")?;
        let choices = self.choices()?;
        let choice = json!({"type":"string","enum":choices});
        let data = embedded(schema.clone(), "decision-data");
        let mut output = object(json!({"choice":choice,"data":data}), &["choice", "data"]);
        let declaration = json!({"const":schema});
        let result = match self.name() {
            "decision.rule" => contract(
                json!({"data":data,"data_schema":declaration}),
                &["data", "data_schema"],
                output,
            ),
            "decision.agent" => contract(
                json!({"agent":identifier(),"context":true,"data_schema":declaration}),
                &["agent", "context", "data_schema"],
                output,
            ),
            "decision.human" => {
                let feedback = array(choice, 0, MAX_SELECTIONS);
                if self.parsed.definition.steps[self.id]
                    .inputs
                    .contains_key("feedback_required")
                {
                    let value = self.literal("feedback_required")?;
                    Schema::compile(
                        feedback.clone(),
                        &self.path("feedback_required"),
                        self.parsed.locations[""],
                    )?
                    .validate(
                        &value,
                        &self.path("feedback_required"),
                        self.parsed.locations[""],
                    )?;
                    output["allOf"] = json!([{"if":{"properties":{"choice":{"enum":value}},"required":["choice"]},"then":{"properties":{"feedback":{"minLength":1}}}}]);
                }
                output["properties"]["feedback"] = json!({"type":"string","maxLength":MAX_TEXT});
                output["required"] = json!(["choice", "data", "feedback"]);
                self.check(InputCheck::Deadline)?;
                contract(
                    json!({"proposal":true,"reviewer":selector(),"deadline":text(MAX_DEADLINE),"data_schema":declaration,"feedback_required":feedback}),
                    &["proposal", "reviewer", "deadline", "data_schema"],
                    output,
                )
            }
            _ => unreachable!("decision family"),
        };
        Ok(result)
    }
    fn wait(&mut self) -> Result<Contract, Diagnostic> {
        self.check(InputCheck::Deadline)?;
        if self.name() == "wait.timer" {
            return Ok(contract(
                json!({"deadline":text(MAX_DEADLINE)}),
                &["deadline"],
                object(
                    json!({"deadline":{"type":"string","format":"date-time","maxLength":MAX_DEADLINE}}),
                    &["deadline"],
                ),
            ));
        }
        let schema = self.declared_schema("payload_schema")?;
        Ok(contract(
            json!({"event":identifier(),"correlation":text(MAX_CORRELATION),"deadline":text(MAX_DEADLINE),"payload_schema":{"const":schema}}),
            &["event", "correlation", "deadline", "payload_schema"],
            schema,
        ))
    }
    pub(super) fn child(&mut self) -> Result<Contract, Diagnostic> {
        let control = self.parsed.controls.get(self.id).ok_or_else(|| {
            self.error(
                "registry.child",
                "Supply pinned child workflow and version identifiers",
                "input",
            )
        })?;
        let child = self
            .facts
            .children
            .iter()
            .find(|child| {
                child.workflow_id == control.child_workflow_id
                    && child.version_id == control.child_version_id
            })
            .ok_or_else(|| {
                self.error(
                    "registry.child",
                    "Supply the matching pinned child contract",
                    "input",
                )
            })?;
        self.effective = json!({"child_workflow_id":child.workflow_id.to_string(),"child_version_id":child.version_id.to_string(),"input_schema":child.input_schema,"output_schema":child.output_schema});
        let input =
            json!({"allOf":[{"type":"object"},embedded(child.input_schema.clone(),"child-input")]});
        let output = if self.name() == "flow.repeat" {
            object(
                json!({"result":embedded(child.output_schema.clone(),"child-output"),"rounds":integer(control.max_iterations.unwrap_or(0) as usize)}),
                &["result", "rounds"],
            )
        } else {
            child.output_schema.clone()
        };
        Ok(contract(json!({"input":input}), &["input"], output))
    }
}
fn finite_labels(schema: &Value) -> bool {
    fn finite(value: &Value) -> bool {
        value
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|values| {
                !values.is_empty()
                    && values.len() <= MAX_SELECTIONS
                    && values.iter().all(|v| {
                        v.as_str()
                            .is_some_and(|s| !s.is_empty() && s.len() <= MAX_IDENTIFIER)
                    })
            })
    }
    finite(schema)
        || ((matches!(schema.get("type"), Some(Value::String(kind)) if kind == "object")
            || schema.get("type") == Some(&json!(["object"])))
            && ["label", "profile"].iter().any(|name| {
                schema
                    .get("properties")
                    .and_then(|p| p.get(name))
                    .is_some_and(finite)
                    && schema
                        .get("required")
                        .and_then(Value::as_array)
                        .is_some_and(|required| required.contains(&json!(name)))
            }))
}
