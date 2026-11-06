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
}

#[test]
fn autonomous_agent_workflow_reference_unknown_key_identifies_the_field() {
    let mut facts = snapshots();
    facts.agents[0].key = AgentKey::parse("other").unwrap();
    assert_diagnostic(
        &source("agent.run"),
        facts,
        "publication.agent",
        "agent",
        "assistant: has missing or inconsistent source uses",
    );
}

#[test]
fn workflow_publication_duplicate_selections_identify_the_list_and_member() {
    let mut facts = snapshots();
    facts.agents.push(agent());
    assert_diagnostic(
        &source("agent.run"),
        facts,
        "publication.duplicate",
        "agent",
        "agent assistant: has duplicate snapshots",
    );
}
