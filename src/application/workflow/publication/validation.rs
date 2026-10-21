use super::*;
use crate::domain::workflow::{Binding, TypeName};
use std::collections::BTreeSet;
use std::io::{self, Write};

pub(super) fn validate(
    company_id: CompanyId,
    snapshots: &DependencySnapshots,
    compiled: &CompiledWorkflow,
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
        check_selection(&agent.tools, &agent.skills, snapshots)
            .map_err(|reason| fail("publication.selection", &reason))?;
    }
    validate_skills(company_id, snapshots, compiled)?;
    validate_tools(company_id, snapshots, compiled)?;
    validate_agents(snapshots, compiled)
}

fn validate_skills(
    company_id: CompanyId,
    snapshots: &DependencySnapshots,
    compiled: &CompiledWorkflow,
) -> Result<(), Diagnostic> {
    let mut skills = BTreeSet::new();
    for skill in &snapshots.skills {
        let fail = |code: &'static str, reason: &str| {
            dependency_error(compiled, "skill", skill.name.as_str(), code, reason)
        };
        if skill.company_id != company_id {
            return Err(fail("publication.ownership", "belongs to another company"));
        }
        if !skills.insert(&skill.name) {
            return Err(fail("publication.duplicate", "has duplicate snapshots"));
        }
        for tool in &skill.required_tools {
            if !has_tool(tool, snapshots) {
                return Err(fail(
                    "publication.selection",
                    &format!("requires missing or ambiguous tool {tool}"),
                ));
            }
        }
    }
    Ok(())
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

fn has_tool(name: &TypeName, snapshots: &DependencySnapshots) -> bool {
    // Name-only capability selections cannot disambiguate identical tool names on
    // different connections. Explicit mcp.call selections retain their slot key.
    snapshots
        .tools
        .iter()
        .filter(|t| &t.contract.name == name)
        .count()
        == 1
}

pub(super) fn check_selection(
    tools: &[TypeName],
    skills: &[TypeName],
    snapshots: &DependencySnapshots,
) -> Result<(), String> {
    for tool in tools {
        if !has_tool(tool, snapshots) {
            return Err(format!(
                "Tool {tool} is missing or ambiguous in the frozen catalogue"
            ));
        }
    }
    for name in skills {
        let Some(skill) = snapshots.skills.iter().find(|s| &s.name == name) else {
            return Err(format!("Skill {name} is missing from the frozen catalogue"));
        };
        for required in &skill.required_tools {
            if !tools.contains(required) {
                return Err(format!("Skill {name} requires unselected tool {required}"));
            }
        }
    }
    Ok(())
}

fn validate_agents(
    snapshots: &DependencySnapshots,
    compiled: &CompiledWorkflow,
) -> Result<(), Diagnostic> {
    for (id, step) in &compiled.graph().definition().steps {
        if crate::application::workflow::registry::ModelStep::of(step.step_type.as_str()).is_none()
        {
            continue;
        }
        let path = format!("/steps/{id}/with/agent");
        let fail = || {
            error(
                compiled,
                &path,
                "publication.agent",
                "Supply the selected agent snapshot, or a finite catalogue for a runtime selector",
            )
        };
        if snapshots.agents.is_empty() {
            return Err(fail());
        }
        if let Some(Binding::Literal(value)) = step.inputs.get("agent")
            && !snapshots
                .agents
                .iter()
                .any(|a| Some(a.key.as_str()) == value.as_str())
        {
            return Err(fail());
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
