use super::*;
impl Registration<'_> {
    pub(super) fn action(&mut self) -> Result<Contract, Diagnostic> {
        let mut contract = match self.name() {
            "http.request" => self.http(),
            "tool.call" | "mcp.call" => self.tool(),
            name @ ("message.send" | "message.reply") => {
                let name = name.to_owned();
                self.check(InputCheck::Message)?;
                let (properties, required) = message_inputs(&name).expect("message family");
                Ok(contract(properties, required, message_result()))
            }
            _ => unreachable!("action family"),
        }?;
        self.approval(&mut contract)?;
        Ok(contract)
    }

    pub(super) fn approval(&mut self, contract: &mut Contract) -> Result<(), Diagnostic> {
        if let Some(binding) = self.parsed.definition.steps[self.id].inputs.get("approval") {
            if !static_approval_reviewers(binding) {
                return Err(self.error(
                    "registry.static_reviewers",
                    "Approvers must be literal or come from binding parameters",
                    "approval",
                ));
            }
            self.check(InputCheck::Approval)?;
        }
        contract.input["properties"]["approval"] = object(
            json!({"reviewers":reviewers(),"deadline":text(MAX_DEADLINE)}),
            &["reviewers", "deadline"],
        );
        Ok(())
    }
    pub(super) fn resource(&mut self, kind: &str) -> Result<Value, Diagnostic> {
        let connection = self.literal("connection")?;
        let Some(slot) = connection.as_str() else {
            return Err(self.error(
                "registry.resource",
                "Connection must name a declared resource slot",
                "connection",
            ));
        };
        let resource = self
            .parsed
            .definition
            .resources
            .iter()
            .find(|r| r.slot.as_str() == slot && r.kind.as_str() == kind)
            .ok_or_else(|| {
                self.error(
                    "registry.resource",
                    "Declare a resource slot with the required connection kind",
                    "connection",
                )
            })?;
        self.effective["resource"] = json!({"slot":resource.slot.as_str(),"kind":resource.kind.as_str(),"contract":resource.contract.as_ref().map(TypeName::as_str)});
        Ok(connection)
    }
    fn http(&mut self) -> Result<Contract, Diagnostic> {
        let connection = self.resource("http")?;
        self.check(InputCheck::Http)?;
        Ok(contract(
            json!({"connection":{"const":connection},"method":{"enum":["GET","HEAD","POST","PUT","PATCH","DELETE","OPTIONS"]},"path":text(MAX_PATH),"headers":headers(),"body":true}),
            &["connection", "method", "path"],
            object(
                json!({"status":{"type":"integer","minimum":100,"maximum":599},"headers":headers(),"body":true}),
                &["status", "headers", "body"],
            ),
        ))
    }
    fn tool(&mut self) -> Result<Contract, Diagnostic> {
        let mcp = self.name() == "mcp.call";
        let connection = if mcp {
            Some(self.resource("mcp")?)
        } else {
            None
        };
        let name = self.literal("tool")?;
        let Some(name) = name.as_str() else {
            return Err(self.error(
                "registry.tool",
                "Tool must name a supplied contract",
                "tool",
            ));
        };
        if crate::application::workflow::message::is_builtin_message_tool(name) {
            return Err(self.error(
                "registry.tool",
                "Built-in messaging is a step type, not a selectable tool",
                "tool",
            ));
        }
        // Provisioning runs only as an agent's tool, under its requester rule (p08 D2.1); an MCP
        // tool of the same name is the server's own.
        if !mcp && crate::application::workflow::provisioning::is_builtin_provisioning_tool(name) {
            return Err(self.error(
                "registry.tool",
                "Built-in provisioning tools are agent tools, not step tools",
                "tool",
            ));
        }
        let tool = self
            .facts
            .tools
            .iter()
            .find(|t| {
                t.name.as_str() == name
                    && t.connection.as_ref().map(ResourceName::as_str)
                        == connection.as_ref().and_then(Value::as_str)
            })
            .ok_or_else(|| {
                self.error(
                    "registry.tool",
                    "Supply the selected tool argument and result contract",
                    "tool",
                )
            })?;
        self.effective["tool"] = json!({"name":tool.name.as_str(),"input_schema":tool.input_schema,"output_schema":tool.output_schema});
        let arguments =
            json!({"allOf":[{"type":"object"},embedded(tool.input_schema.clone(),"tool-input")]});
        let mut properties = json!({"tool":{"const":name},"arguments":arguments});
        if let Some(connection) = connection {
            properties["connection"] = json!({"const":connection});
            let output = mcp::tool_result(&tool.output_schema);
            Ok(contract(
                properties,
                &["connection", "tool", "arguments"],
                output,
            ))
        } else {
            Ok(contract(
                properties,
                &["tool", "arguments"],
                tool.output_schema.clone(),
            ))
        }
    }
}

/// One definition for both the step contract and the trusted built-in tool snapshot.
fn message_inputs(name: &str) -> Option<(Value, &'static [&'static str])> {
    match name {
        "message.send" => {
            let mut destinations = array(
                object(
                    json!({"kind":{"enum":["user","channel"]},"id":uuid()}),
                    &["kind", "id"],
                ),
                1,
                MAX_MESSAGE_DESTINATIONS,
            );
            destinations["uniqueItems"] = json!(true);
            Some((
                json!({"content":content(),"destinations":destinations}),
                &["content", "destinations"],
            ))
        }
        "message.reply" => Some((
            json!({"source_message":uuid(),"content":content()}),
            &["source_message", "content"],
        )),
        _ => None,
    }
}

/// The (input, output) schemas of a built-in messaging tool, as its step contract states them.
pub(crate) fn message_tool_schemas(name: &str) -> Option<(Value, Value)> {
    let (properties, required) = message_inputs(name)?;
    Some((object(properties, required), message_result()))
}

// Deadline bindings may be dynamic; only reviewer assignment must be static.
fn static_approval_reviewers(binding: &Binding) -> bool {
    match binding {
        Binding::Object(fields) => fields.get("reviewers").is_none_or(|reviewers| {
            reviewers
                .references()
                .iter()
                .all(|r| r.root_name() == "params")
        }),
        Binding::Default {
            reference,
            fallback,
        } => reference.root_name() == "params" && static_approval_reviewers(fallback),
        other => other
            .references()
            .iter()
            .all(|reference| reference.root_name() == "params"),
    }
}
