use super::*;
use crate::domain::workflow::Binding;
use std::collections::BTreeSet;
use std::io::{self, Write};

pub(super) fn validate(
    company_id: CompanyId,
    snapshots: &DependencySnapshots,
    compiled: &CompiledWorkflow,
    expected: &BTreeMap<crate::application::agent::AgentKey, Vec<AgentReferenceUse>>,
) -> Result<(), Diagnostic> {
    let mut agents = BTreeSet::new();
    for agent in &snapshots.agents {
        let fail = |code: &'static str, reason: &str| {
            dependency_error(compiled, "agent", agent.key.as_str(), code, reason)
        };
        if agent.company_id != company_id {
            return Err(fail("publication.ownership", "belongs to another company"));
        }
        if !agents.insert(&agent.key) {
            return Err(fail("publication.duplicate", "has duplicate snapshots"));
        }
    }
    validate_tools(company_id, snapshots, compiled)?;
    validate_agents(snapshots, compiled, expected)
}

fn validate_tools(
    company_id: CompanyId,
    snapshots: &DependencySnapshots,
    compiled: &CompiledWorkflow,
) -> Result<(), Diagnostic> {
    for tool in &snapshots.tools {
        let fail = |code: &'static str, reason: &str| {
            dependency_error(compiled, "tool", tool.contract.name.as_str(), code, reason)
        };
        if tool.company_id != company_id {
            return Err(fail("publication.ownership", "belongs to another company"));
        }
        if tool.policy.policy_revision == 0 {
            return Err(fail(
                "publication.policy",
                "requires a positive approved policy revision",
            ));
        }
        if let Some(slot) = &tool.contract.connection {
            let valid = compiled
                .graph()
                .definition()
                .resources
                .iter()
                .any(|r| &r.slot == slot && matches!(r.kind.as_str(), "mcp" | "http"));
            if !valid {
                return Err(fail(
                    "publication.resource",
                    &format!("requires declared HTTP or MCP resource slot {slot}"),
                ));
            }
        }
    }
    Ok(())
}

fn dependency_error(
    compiled: &CompiledWorkflow,
    field: &str,
    name: &str,
    code: &'static str,
    reason: &str,
) -> Diagnostic {
    let path = dependency_path(compiled.graph().definition(), field, name);
    error(compiled, &path, code, &format!("{field} {name}: {reason}"))
}

pub(super) fn dependency_path(
    definition: &crate::domain::workflow::WorkflowDefinition,
    field: &str,
    name: &str,
) -> String {
    definition
        .steps
        .iter()
        .find_map(|(id, step)| {
            matches!(step.inputs.get(field), Some(Binding::Literal(v)) if v.as_str() == Some(name))
                .then(|| format!("/steps/{id}/with/{field}"))
        })
        .unwrap_or_else(|| "/steps".into())
}

fn validate_agents(
    snapshots: &DependencySnapshots,
    compiled: &CompiledWorkflow,
    expected: &BTreeMap<crate::application::agent::AgentKey, Vec<AgentReferenceUse>>,
) -> Result<(), Diagnostic> {
    if snapshots.agents.len() != expected.len() {
        return Err(error(
            compiled,
            "/steps",
            "publication.agent",
            "Supply exactly the declared finite agent reference universe",
        ));
    }
    for (key, uses) in expected {
        if !snapshots
            .agents
            .iter()
            .any(|reference| &reference.key == key && &reference.uses == uses)
        {
            return Err(dependency_error(
                compiled,
                "agent",
                key.as_str(),
                "publication.agent",
                "has missing or inconsistent source uses",
            ));
        }
    }
    Ok(())
}

pub(super) fn error(
    compiled: &CompiledWorkflow,
    path: &str,
    code: &'static str,
    message: &str,
) -> Diagnostic {
    Diagnostic::at(
        code,
        message,
        path,
        compiler::input_span(compiled.source_map(), path),
    )
}

struct SizeBudget(usize);
impl Write for SizeBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("bundle size"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn check_manifest_size(
    value: &Value,
    compiled: &CompiledWorkflow,
) -> Result<(), Diagnostic> {
    serde_json::to_writer(SizeBudget(MAX_BUNDLE_BYTES), value).map_err(|_| {
        error(
            compiled,
            "/steps",
            "publication.limit",
            "Published manifest exceeds 4 MiB",
        )
    })
}
