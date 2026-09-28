use super::*;
use crate::domain::workflow::TypeName;
use std::collections::BTreeSet;

pub(super) fn validate(
    snapshots: &DependencySnapshots,
    children: &[Arc<PublishedBundle>],
    parsed: &compiler::ParsedWorkflow,
) -> Result<(), Diagnostic> {
    let count = snapshots
        .agents
        .len()
        .saturating_add(snapshots.skills.len())
        .saturating_add(snapshots.profiles.len())
        .saturating_add(snapshots.tools.len())
        .saturating_add(children.len());
    if count > MAX_DEPENDENCIES {
        return Err(Diagnostic::at(
            "publication.limit",
            "At most 256 dependency snapshots are allowed",
            "/steps",
            parsed.locations["/steps"],
        ));
    }
    // Bound borrowed content before cloning facts or serializing the manifest.
    let mut budget = compiler::FactBudget::new(parsed.locations["/steps"]);
    for tool in &snapshots.tools {
        for schema in tool.schemas() {
            budget.schema(schema)?;
        }
    }
    for child in children {
        let definition = child.compiled.graph().definition();
        budget.schema(definition.input_schema.as_ref().expect("compiled schema"))?;
        budget.schema(definition.output_schema.as_ref().expect("compiled schema"))?;
    }
    let check = SnapshotCheck { parsed };
    for agent in &snapshots.agents {
        check.agent(agent)?;
    }
    for skill in &snapshots.skills {
        check.text("skill", skill.name.as_str(), &skill.instructions)?;
        check.selection(
            "skill",
            skill.name.as_str(),
            "required_tools",
            &skill.required_tools,
        )?;
    }
    check.unique_contracts(snapshots)?;
    budget.charge(snapshots)?;
    Ok(())
}

struct SnapshotCheck<'a> {
    parsed: &'a compiler::ParsedWorkflow,
}

impl SnapshotCheck<'_> {
    fn error(&self, field: &str, name: &str, code: &'static str, reason: &str) -> Diagnostic {
        let path = validation::dependency_path(&self.parsed.definition, field, name);
        Diagnostic::at(
            code,
            &format!("{field} {name}: {reason}"),
            &path,
            compiler::input_span(&self.parsed.locations, &path),
        )
    }

    fn text(&self, field: &str, name: &str, text: &str) -> Result<(), Diagnostic> {
        if text.len() > registry::MAX_TEXT {
            return Err(self.error(
                field,
                name,
                "publication.limit",
                &format!("instructions exceed {} bytes", registry::MAX_TEXT),
            ));
        }
        Ok(())
    }

    fn selection(
        &self,
        field: &str,
        name: &str,
        list: &str,
        values: &[TypeName],
    ) -> Result<(), Diagnostic> {
        if values.len() > registry::MAX_SELECTIONS {
            return Err(self.error(
                field,
                name,
                "publication.limit",
                &format!("{list} exceeds {} selections", registry::MAX_SELECTIONS),
            ));
        }
        let mut seen = BTreeSet::new();
        for value in values {
            if !seen.insert(value) {
                return Err(self.error(
                    field,
                    name,
                    "publication.duplicate",
                    &format!("{list} repeats selection {value}"),
                ));
            }
        }
        Ok(())
    }

    fn agent(&self, agent: &AgentSnapshot) -> Result<(), Diagnostic> {
        let name = agent.key.as_str();
        self.text("agent", name, &agent.instructions)?;
        if agent.model.model.is_empty() || agent.model.model.len() > registry::MAX_IDENTIFIER {
            return Err(self.error(
                "agent",
                name,
                "publication.model",
                &format!(
                    "model name must contain 1..={} bytes",
                    registry::MAX_IDENTIFIER
                ),
            ));
        }
        if !(1..=131_072).contains(&agent.model.max_output_tokens) {
            return Err(self.error(
                "agent",
                name,
                "publication.model",
                "max_output_tokens must be in 1..=131072",
            ));
        }
        self.selection("agent", name, "tools", &agent.tools)?;
        self.selection("agent", name, "skills", &agent.skills)
    }

    fn unique_contracts(&self, snapshots: &DependencySnapshots) -> Result<(), Diagnostic> {
        let mut tools = BTreeSet::new();
        for tool in &snapshots.tools {
            let contract = &tool.contract;
            if !tools.insert((&contract.connection, &contract.name)) {
                let slot = contract
                    .connection
                    .as_ref()
                    .map_or("native", |s| s.as_str());
                return Err(self.error(
                    "tool",
                    contract.name.as_str(),
                    "publication.duplicate",
                    &format!("has duplicate snapshots for connection {slot}"),
                ));
            }
        }
        let mut profiles = BTreeSet::new();
        for profile in &snapshots.profiles {
            let name = profile.name.as_str();
            if !profiles.insert(&profile.name) {
                return Err(self.error(
                    "capability_profile",
                    name,
                    "publication.duplicate",
                    "has duplicate snapshots",
                ));
            }
            self.selection("capability_profile", name, "tools", &profile.tools)?;
            self.selection("capability_profile", name, "skills", &profile.skills)?;
        }
        Ok(())
    }
}
