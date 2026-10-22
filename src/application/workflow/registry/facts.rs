use super::*;
use std::io::{self, Write};

/// Explicit frozen contract facts, supplied by the caller. These are not grants.
#[derive(Default)]
pub struct CatalogueFacts {
    pub tools: Vec<ToolContract>,
    pub children: Vec<ChildContract>,
    /// Workflows the definition's workflow tools pin (07-D2 §D2.2): dependency edges like child
    /// steps, so recursion and depth apply to them too.
    pub tool_workflows: BTreeSet<WorkflowId>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolContract {
    pub name: TypeName,
    /// Some selects an MCP slot; None selects a shared tool action.
    pub connection: Option<ResourceName>,
    pub input_schema: Value,
    pub output_schema: Value,
}

pub struct ChildContract {
    pub workflow_id: WorkflowId,
    pub version_id: VersionId,
    pub input_schema: Value,
    pub output_schema: Value,
    /// The child's own binding parameters, supplied by the parent step's `with.params`.
    pub parameter_schema: Value,
    /// The child's resource slots; the parent must declare each by the same name and contract.
    pub resources: Vec<crate::domain::workflow::ResourceRequirement>,
}

struct Budget(usize);
impl Write for Budget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("fact limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl CatalogueFacts {
    pub(super) fn validate(&self, span: SourceSpan) -> Result<(), Diagnostic> {
        if self.tools.len() + self.children.len() > MAX_FACTS {
            return Err(Diagnostic::at(
                "registry.limit",
                "At most 256 supplied contracts are allowed",
                "/steps",
                span,
            ));
        }
        let mut budget = Budget(1_048_576);
        let mut names = BTreeSet::new();
        for tool in &self.tools {
            if !names.insert((tool.connection.clone(), tool.name.clone())) {
                return Err(fact_error(span));
            }
            check_fact_schema(&tool.input_schema, &mut budget, span)?;
            check_fact_schema(&tool.output_schema, &mut budget, span)?;
        }
        let mut versions = BTreeSet::new();
        for child in &self.children {
            if !versions.insert(child.version_id) {
                return Err(fact_error(span));
            }
            check_fact_schema(&child.input_schema, &mut budget, span)?;
            check_fact_schema(&child.output_schema, &mut budget, span)?;
        }
        Ok(())
    }
}
fn fact_error(span: SourceSpan) -> Diagnostic {
    Diagnostic::at(
        "registry.facts",
        "Supplied contracts must be unique and bounded",
        "/steps",
        span,
    )
}
fn check_fact_schema(
    value: &Value,
    budget: &mut Budget,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    // Check borrowed values before cloning or serializing an untrusted caller's tree.
    crate::domain::workflow::validate_context_value(
        value,
        crate::domain::workflow::ContextLimits {
            output_bytes: 1_048_576,
            work_nodes: 65_536,
        },
    )
    .map_err(|_| fact_error(span))?;
    serde_json::to_writer(budget, value).map_err(|_| fact_error(span))?;
    Schema::compile(value.clone(), "/steps", span)?;
    Ok(())
}
