use super::*;
impl Registration<'_> {
    pub(super) fn action(&mut self) -> Result<Contract, Diagnostic> {
        match self.name() {
            "http.request" => self.http(),
            "tool.call" | "mcp.call" => self.tool(),
            "message.send" => Ok(contract(
                json!({"content":content(),"destinations":array(object(json!({"kind":{"enum":["user","group","channel"]},"id":identifier()}), &["kind","id"]),1,MAX_SELECTIONS)}),
                &["content", "destinations"],
                message_result(),
            )),
            "message.reply" => Ok(contract(
                json!({"source_message":identifier(),"content":content()}),
                &["source_message", "content"],
                message_result(),
            )),
            _ => unreachable!("action family"),
        }
    }
    fn resource(&mut self, kind: &str) -> Result<Value, Diagnostic> {
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
            let content = mcp::content();
            let output = object(
                json!({"content":content,"structuredContent":embedded(tool.output_schema.clone(),"mcp-output"),"isError":{"type":"boolean"},"_meta":mcp::metadata()}),
                &["content", "isError"],
            );
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
