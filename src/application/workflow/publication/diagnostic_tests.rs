use super::*;

fn assert_diagnostic(
    source: &Value,
    facts: DependencySnapshots,
    code: &str,
    field: &str,
    reason: &str,
) {
    let error = build(source, facts).err().unwrap();
    assert_eq!(error.code, code);
    assert_eq!(&*error.field_path, format!("/steps/start/with/{field}"));
    assert!(error.message.contains(reason), "{}", error.message);
    assert!(error.span.line > 0);
    let start = error.span.start;
    let end = error.span.end;
    // Container spans mark their opening token, not the entire value.
    assert!(end > start);
    assert!(
        source.to_string()[start..]
            .starts_with(&source["steps"]["start"]["with"][field].to_string())
    );
}

#[test]
fn workflow_publication_duplicate_contracts_identify_the_dependency() {
    let tool = tool_snapshot("tool.call");
    assert_diagnostic(
        &source("tool.call"),
        DependencySnapshots {
            tools: vec![tool.clone(), tool],
            ..Default::default()
        },
        "publication.duplicate",
        "tool",
        "tool lookup: has duplicate snapshots for connection native",
    );
    let mut source = source("agent.run");
    source["steps"]["start"]["with"]["capability_profile"] = json!({"literal":"limited"});
    let profile = registry::CapabilityProfile {
        name: name("limited"),
        tools: vec![],
        skills: vec![],
    };
    let mut facts = snapshots();
    facts.profiles = vec![profile.clone(), profile];
    assert_diagnostic(
        &source,
        facts,
        "publication.duplicate",
        "capability_profile",
        "capability_profile limited: has duplicate snapshots",
    );
}

#[test]
fn workflow_publication_invalid_agent_settings_identify_the_field() {
    let mut facts = snapshots();
    facts.agents[0].model.model.clear();
    assert_diagnostic(
        &source("agent.run"),
        facts,
        "publication.model",
        "agent",
        "agent assistant: model name must contain",
    );
    let mut facts = snapshots();
    facts.agents[0].model.max_output_tokens = 0;
    assert_diagnostic(
        &source("agent.run"),
        facts,
        "publication.model",
        "agent",
        "agent assistant: max_output_tokens must be in 1..=131072",
    );
}

#[test]
fn workflow_publication_duplicate_selections_identify_the_list_and_member() {
    let mut facts = snapshots();
    facts.agents[0].tools = vec![name("lookup"), name("lookup")];
    assert_diagnostic(
        &source("agent.run"),
        facts,
        "publication.duplicate",
        "agent",
        "agent assistant: tools repeats selection lookup",
    );
    let mut source = source("agent.run");
    source["steps"]["start"]["with"]["capability_profile"] = json!({"literal":"limited"});
    let mut facts = snapshots();
    facts.profiles.push(registry::CapabilityProfile {
        name: name("limited"),
        tools: vec![],
        skills: vec![name("support"), name("support")],
    });
    assert_diagnostic(
        &source,
        facts,
        "publication.duplicate",
        "capability_profile",
        "capability_profile limited: skills repeats selection support",
    );
}
