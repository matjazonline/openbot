use super::*;
use std::io::{self, Write};

/// Explicit frozen contract facts, supplied by the caller. These are not grants.
#[derive(Default)]
pub struct CatalogueFacts {
    pub tools: Vec<ToolContract>,
    pub profiles: Vec<CapabilityProfile>,
    pub children: Vec<ChildContract>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolContract {
    pub name: TypeName,
    /// Some selects an MCP slot; None selects a shared tool action.
    pub connection: Option<ResourceName>,
    pub input_schema: Value,
    pub output_schema: Value,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CapabilityProfile {
    pub name: TypeName,
    pub tools: Vec<TypeName>,
    pub skills: Vec<TypeName>,
}

pub struct ChildContract {
    pub workflow_id: WorkflowId,
    pub version_id: VersionId,
    pub input_schema: Value,
    pub output_schema: Value,
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
        if self.tools.len() + self.profiles.len() + self.children.len() > MAX_FACTS {
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
        let mut names = BTreeSet::new();
        for profile in &self.profiles {
            if !names.insert(&profile.name)
                || profile.tools.len() > MAX_SELECTIONS
                || profile.skills.len() > MAX_SELECTIONS
                || !unique(&profile.tools)
                || !unique(&profile.skills)
            {
                return Err(fact_error(span));
            }
            for name in std::iter::once(&profile.name)
                .chain(&profile.tools)
                .chain(&profile.skills)
            {
                budget
                    .write_all(name.as_str().as_bytes())
                    .map_err(|_| fact_error(span))?;
            }
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
fn unique(names: &[TypeName]) -> bool {
    names.iter().collect::<BTreeSet<_>>().len() == names.len()
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
