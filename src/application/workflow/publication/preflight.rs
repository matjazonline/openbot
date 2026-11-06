use super::*;
use std::collections::BTreeSet;

pub(super) fn validate(
    snapshots: &DependencySnapshots,
    children: &[Arc<PublishedBundle>],
    parsed: &compiler::ParsedWorkflow,
) -> Result<(), Diagnostic> {
    let fail = |code, reason| Diagnostic::at(code, reason, "/steps", parsed.locations["/steps"]);
    let count = snapshots
        .agents
        .len()
        .saturating_add(snapshots.tools.len())
        .saturating_add(children.len());
    if count > MAX_DEPENDENCIES {
        return Err(fail(
            "publication.limit",
            "At most 256 workflow dependency selections are allowed",
        ));
    }
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
    for reference in &snapshots.agents {
        if reference.agent_id.as_uuid().is_nil()
            || reference.uses.is_empty()
            || reference.uses.len() > parsed.definition.steps.len()
        {
            return Err(fail(
                "publication.agent_identity",
                "Agent references require a trusted UUID and declared source uses",
            ));
        }
    }
    let mut tools = BTreeSet::new();
    for tool in &snapshots.tools {
        let contract = &tool.contract;
        if !tools.insert((&contract.connection, &contract.name)) {
            let field = "tool";
            let path = parsed.definition.steps.iter().find_map(|(id, step)| {
                matches!(step.inputs.get(field), Some(crate::domain::workflow::Binding::Literal(value)) if value.as_str() == Some(contract.name.as_str()))
                    .then(|| format!("/steps/{id}/with/{field}"))
            }).unwrap_or_else(|| "/steps".into());
            let span = parsed
                .locations
                .get(&path)
                .copied()
                .unwrap_or(parsed.locations["/steps"]);
            return Err(Diagnostic::at(
                "publication.duplicate",
                &format!(
                    "tool {}: has duplicate snapshots for connection {}",
                    contract.name.as_str(),
                    contract
                        .connection
                        .as_ref()
                        .map(|slot| slot.as_str())
                        .unwrap_or("native")
                ),
                &path,
                span,
            ));
        }
    }
    budget.charge(snapshots)?;
    Ok(())
}
