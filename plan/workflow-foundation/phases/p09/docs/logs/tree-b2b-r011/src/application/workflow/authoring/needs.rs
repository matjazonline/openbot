use crate::application::workflow::compiler::{self, DecodedSource, Diagnostic};
use crate::domain::workflow::{Binding, ResourceName, StepId, TypeName, VersionId};
use serde_json::Value;
use std::collections::BTreeMap;

/// An `mcp.call` tool reached through a declared slot: publication freezes it from the company
/// MCP connection the author selects for that slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpNeed {
    pub slot: ResourceName,
    pub tool: TypeName,
    pub steps: Vec<StepId>,
}

/// A `tool.call` tool named literally: a company HTTP operation needs its connection
/// selected; workflow tools and built-in tools are resolved by name at publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolNeed {
    pub tool: TypeName,
    pub steps: Vec<StepId>,
}

/// What a draft's steps ask the author to select before validation and publication. A request
/// only: the publication directories decide what a selection may freeze.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublicationNeeds {
    pub mcp: Vec<McpNeed>,
    pub tools: Vec<ToolNeed>,
}

impl PublicationNeeds {
    pub fn is_empty(&self) -> bool {
        self.mcp.is_empty() && self.tools.is_empty()
    }
}

fn literal_name(step: &crate::domain::workflow::StepDefinition, input: &str) -> Option<String> {
    match step.inputs.get(input) {
        Some(Binding::Literal(Value::String(value))) => Some(value.clone()),
        _ => None,
    }
}

/// The selections a parsed draft names. A step whose slot or tool is not a literal name is
/// left to the compiler, which reports it against the source.
pub fn publication_needs(decoded: &DecodedSource) -> Result<PublicationNeeds, Diagnostic> {
    let parsed = compiler::parse_workflow(decoded, VersionId::new(uuid::Uuid::nil()))?;
    let mut mcp: BTreeMap<(ResourceName, TypeName), Vec<StepId>> = BTreeMap::new();
    let mut tools: BTreeMap<TypeName, Vec<StepId>> = BTreeMap::new();
    for (id, step) in &parsed.definition.steps {
        match step.step_type.as_str() {
            "mcp.call" => {
                let slot =
                    literal_name(step, "connection").and_then(|v| ResourceName::parse(v).ok());
                let tool = literal_name(step, "tool").and_then(|v| TypeName::parse(v).ok());
                if let (Some(slot), Some(tool)) = (slot, tool) {
                    mcp.entry((slot, tool)).or_default().push(id.clone());
                }
            }
            "tool.call" => {
                if let Some(tool) = literal_name(step, "tool").and_then(|v| TypeName::parse(v).ok())
                {
                    tools.entry(tool).or_default().push(id.clone());
                }
            }
            _ => {}
        }
    }
    Ok(PublicationNeeds {
        mcp: mcp
            .into_iter()
            .map(|((slot, tool), steps)| McpNeed { slot, tool, steps })
            .collect(),
        tools: tools
            .into_iter()
            .map(|(tool, steps)| ToolNeed { tool, steps })
            .collect(),
    })
}
