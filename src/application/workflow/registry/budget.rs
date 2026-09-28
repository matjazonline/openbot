use super::*;
use compiler::FactBudget;

/// Charge each future copy using borrowed inputs before constructing any descriptor.
/// Generated fixed-size envelopes are charged exactly as each descriptor is admitted.
pub(super) fn preflight(parsed: &ParsedWorkflow, facts: &CatalogueFacts) -> Result<(), Diagnostic> {
    let mut budget = FactBudget::new(parsed.locations[""]);
    if facts.tools.len() + facts.profiles.len() + facts.children.len() > MAX_FACTS {
        return Err(Diagnostic::at(
            "registry.limit",
            "At most 256 supplied contracts are allowed",
            "/steps",
            parsed.locations[""],
        ));
    }
    for (id, step) in &parsed.definition.steps {
        declarations(&mut budget, step)?;
        match step.step_type.as_str() {
            "agent.run" => profiles(&mut budget, facts)?,
            "tool.call" | "mcp.call" => tool(&mut budget, step, facts)?,
            "workflow.call" | "flow.repeat" => {
                if let Some(control) = parsed.controls.get(id)
                    && let Some(child) = facts.children.iter().find(|child| {
                        child.workflow_id == control.child_workflow_id
                            && child.version_id == control.child_version_id
                    })
                {
                    schemas(&mut budget, &child.input_schema, &child.output_schema)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn schemas(budget: &mut FactBudget, input: &Value, output: &Value) -> Result<(), Diagnostic> {
    // The effective fact and specialized input/output each retain these schemas.
    for schema in [input, output, input, output] {
        budget.schema(schema)?;
    }
    Ok(())
}
fn profiles(budget: &mut FactBudget, facts: &CatalogueFacts) -> Result<(), Diagnostic> {
    for profile in &facts.profiles {
        budget.charge(&profile.name.as_str())?;
        // Names occur both in selection schema and frozen effective facts.
        budget.charge(&profile.name.as_str())?;
        for name in profile.tools.iter().chain(&profile.skills) {
            budget.charge(&name.as_str())?;
        }
    }
    Ok(())
}
fn tool(
    budget: &mut FactBudget,
    step: &crate::domain::workflow::StepDefinition,
    facts: &CatalogueFacts,
) -> Result<(), Diagnostic> {
    let literal = |field| match step.inputs.get(field) {
        Some(Binding::Literal(value)) => value.as_str(),
        _ => None,
    };
    if let Some(tool) = facts.tools.iter().find(|tool| {
        Some(tool.name.as_str()) == literal("tool")
            && tool.connection.as_ref().map(ResourceName::as_str) == literal("connection")
    }) {
        schemas(budget, &tool.input_schema, &tool.output_schema)?;
    }
    Ok(())
}

fn declarations(
    budget: &mut FactBudget,
    step: &crate::domain::workflow::StepDefinition,
) -> Result<(), Diagnostic> {
    for field in ["output_schema", "data_schema", "payload_schema"] {
        let Some(Binding::Literal(schema)) = step.inputs.get(field) else {
            continue;
        };
        budget.schema(schema)?;
        budget.schema(schema)?;
        if step.step_type.as_str() == "data.map" || step.step_type.as_str().starts_with("decision.")
        {
            budget.schema(schema)?;
        }
    }
    Ok(())
}
