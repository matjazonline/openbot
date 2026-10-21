use super::*;

fn skill(name_: &str, id: u128, required: Vec<TypeName>) -> SkillSnapshot {
    SkillSnapshot {
        company_id: company(),
        skill_id: Uuid::from_u128(id),
        name: name(name_),
        instructions: format!("{name_} instructions"),
        required_tools: required,
    }
}

fn capable() -> DependencySnapshots {
    let mut facts = snapshots();
    let mut second = tool_snapshot("tool.call");
    second.contract.name = name("search");
    facts.tools = vec![tool_snapshot("tool.call"), second];
    facts.skills = vec![
        skill("support", 11, vec![name("lookup")]),
        skill("style", 12, vec![]),
    ];
    facts.agents[0].tools = vec![name("search"), name("lookup")];
    facts.agents[0].skills = vec![name("style"), name("support")];
    facts
}

fn resolve(bundle: &PublishedBundle) -> Result<ResolvedAgent, crate::app_error::AppError> {
    bundle.resolve_agent(company(), &AgentKey::parse("assistant").unwrap())
}

#[test]
fn workflow_capabilities_agent_receives_exactly_its_saved_tools_and_skills_in_order() {
    let bundle = build(&source("agent.run"), capable()).unwrap();
    let resolved = resolve(&bundle).unwrap();
    assert_eq!(resolved.company_id(), company());
    assert_eq!(resolved.agent().tools, vec![name("search"), name("lookup")]);
    assert_eq!(
        resolved
            .tools()
            .iter()
            .map(|t| t.contract.name.clone())
            .collect::<Vec<_>>(),
        resolved.agent().tools
    );
    assert_eq!(
        resolved.agent().skills,
        vec![name("style"), name("support")]
    );
    assert_eq!(resolved.skills()[0].skill_id, Uuid::from_u128(12));
    assert_eq!(resolved.skills()[1].instructions, "support instructions");
    // Frozen content survives serialization unchanged.
    let first = serde_json::to_value(&resolved).unwrap();
    assert_eq!(
        serde_json::to_value(resolve(&bundle).unwrap()).unwrap(),
        first
    );
}

#[test]
fn workflow_capabilities_agent_without_saved_capabilities_receives_none() {
    let bundle = build(&source("agent.run"), snapshots()).unwrap();
    let resolved = resolve(&bundle).unwrap();
    assert!(resolved.tools().is_empty());
    assert!(resolved.skills().is_empty());
}

#[test]
fn workflow_capabilities_refuse_other_company_and_unknown_agent() {
    let bundle = build(&source("agent.run"), capable()).unwrap();
    assert!(matches!(
        bundle.resolve_agent(
            CompanyId::new(Uuid::from_u128(999)),
            &AgentKey::parse("assistant").unwrap()
        ),
        Err(crate::app_error::AppError::NotFound(_))
    ));
    assert!(
        bundle
            .resolve_agent(company(), &AgentKey::parse("other").unwrap())
            .is_err()
    );
}

#[test]
fn workflow_capabilities_skill_requirement_outside_agent_tools_fails_publication() {
    let mut facts = capable();
    facts.agents[0].tools = vec![name("search")];
    let error = build(&source("agent.run"), facts).err().unwrap();
    assert_eq!(error.code, "publication.selection");
    assert!(error.message.contains("support"), "{}", error.message);
}

#[test]
fn workflow_capabilities_step_and_workflow_narrowing_inputs_are_rejected() {
    let mut profiled = source("agent.run");
    profiled["steps"]["start"]["with"]["capability_profile"] =
        json!({"literal":{"tools":[],"skills":[]}});
    assert!(build(&profiled, capable()).is_err());
    let mut ceiling = source("agent.run");
    ceiling["capability_ceiling"] = json!({"tools":[],"skills":[]});
    assert!(build(&ceiling, capable()).is_err());
}

#[test]
fn workflow_capabilities_agent_universe_is_literal_or_finite_parameter_enum() {
    let decode =
        |value: &Value| crate::adapters::workflow_source::decode(&value.to_string()).unwrap();
    let literal = source("agent.run");
    assert_eq!(
        agent_universe(&decode(&literal)).unwrap(),
        vec![AgentKey::parse("assistant").unwrap()]
    );
    let mut enumerated = source("agent.run");
    enumerated["parameter_schema"] = json!({"type":"object","properties":{"agent":{"type":"string","enum":["sales","support"]}}});
    enumerated["steps"]["start"]["with"]["agent"] = json!({"ref":"/params/agent"});
    assert_eq!(
        agent_universe(&decode(&enumerated)).unwrap(),
        vec![
            AgentKey::parse("sales").unwrap(),
            AgentKey::parse("support").unwrap()
        ]
    );
    let mut open = enumerated.clone();
    open["parameter_schema"] = json!({"type":"object","properties":{"agent":{"type":"string"}}});
    assert!(agent_universe(&decode(&open)).is_err());
    let mut input = enumerated;
    input["steps"]["start"]["with"]["agent"] = json!({"ref":"/input/agent"});
    assert!(agent_universe(&decode(&input)).is_err());
}

/// 06-D6 B: `ai.classify` selects a frozen agent exactly like `agent.run`.
#[test]
fn workflow_capabilities_classify_captures_and_requires_its_agent() {
    let decode =
        |value: &Value| crate::adapters::workflow_source::decode(&value.to_string()).unwrap();
    let classify = source("ai.classify");
    assert_eq!(
        agent_universe(&decode(&classify)).unwrap(),
        vec![AgentKey::parse("assistant").unwrap()]
    );
    let bundle = build(&classify, snapshots()).unwrap();
    assert!(resolve(&bundle).is_ok());
    let mut missing = snapshots();
    missing.agents.clear();
    assert_eq!(
        build(&classify, missing).err().unwrap().code,
        "publication.agent"
    );
    let mut without = classify;
    without["steps"]["start"]["with"]
        .as_object_mut()
        .unwrap()
        .remove("agent");
    assert!(build(&without, snapshots()).is_err());
}
