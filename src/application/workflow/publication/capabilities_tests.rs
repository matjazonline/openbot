use super::*;
use crate::domain::workflow::CapabilityCeiling;

fn capable() -> DependencySnapshots {
    let mut facts = snapshots();
    let mut second = tool_snapshot("tool.call");
    second.contract.name = name("search");
    facts.tools = vec![tool_snapshot("tool.call"), second];
    facts.skills = vec![
        SkillSnapshot {
            company_id: company(),
            name: name("support"),
            instructions: "Support instructions".into(),
            required_tools: vec![name("lookup")],
        },
        SkillSnapshot {
            company_id: company(),
            name: name("style"),
            instructions: "Style instructions".into(),
            required_tools: vec![],
        },
    ];
    facts.agents[0].tools = vec![name("search"), name("lookup")];
    facts.agents[0].skills = vec![name("style"), name("support")];
    facts.profiles = vec![registry::CapabilityProfile {
        name: name("support"),
        tools: vec![name("lookup")],
        skills: vec![name("support")],
    }];
    facts
}

fn resolve(
    bundle: &PublishedBundle,
    profile: Option<&Value>,
) -> Result<ResolvedAgent, crate::app_error::AppError> {
    bundle.resolve_agent(company(), &AgentKey::parse("assistant").unwrap(), profile)
}

#[test]
fn workflow_capabilities_defaults_are_ordered_operational_snapshots() {
    let bundle = build(&source("agent.run"), capable()).unwrap();
    let resolved = resolve(&bundle, None).unwrap();
    assert_eq!(resolved.source(), SelectionSource::AgentDefaults);
    assert_eq!(resolved.profile(), None);
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
    assert_eq!(resolved.skills()[0].instructions, "Style instructions");
    assert_eq!(resolved.skills()[1].instructions, "Support instructions");
    assert_eq!(resolved.agent().model.model, "scripted");
}

#[test]
fn workflow_capabilities_explicit_profile_replaces_defaults_and_preserves_order() {
    let bundle = build(&source("agent.run"), capable()).unwrap();
    let selected = resolve(&bundle, Some(&json!("support"))).unwrap();
    assert_eq!(selected.source(), SelectionSource::Profile);
    assert_eq!(selected.profile(), Some(&name("support")));
    assert_eq!(selected.agent().tools, vec![name("lookup")]);
    assert_eq!(selected.agent().skills, vec![name("support")]);
    let inline = resolve(
        &bundle,
        Some(&json!({"tools":["lookup","search"],"skills":["support","style"]})),
    )
    .unwrap();
    assert_eq!(inline.agent().tools, vec![name("lookup"), name("search")]);
    assert_eq!(inline.agent().skills, vec![name("support"), name("style")]);
    assert_eq!(inline.profile(), None);
    let empty = resolve(&bundle, Some(&json!({"tools":[],"skills":[]}))).unwrap();
    assert!(empty.tools().is_empty());
    assert!(empty.skills().is_empty());
    let empty_agent = build(&source("agent.run"), snapshots()).unwrap();
    let none = resolve(&empty_agent, None).unwrap();
    assert!(none.tools().is_empty() && none.skills().is_empty());
}

#[test]
fn workflow_capabilities_invalid_explicit_profiles_never_inherit() {
    let bundle = build(&source("agent.run"), capable()).unwrap();
    let cases = [
        Value::Null,
        json!(42),
        json!("missing"),
        json!({}),
        json!({"tools":[]}),
        json!({"skills":[]}),
        json!({"tools":[],"skills":null}),
        json!({"tools":[],"skills":[],"extra":1}),
        json!({"tools":["lookup","lookup"],"skills":[]}),
        json!({"tools":[],"skills":["style","style"]}),
        json!({"tools":["missing"],"skills":[]}),
        json!({"tools":[],"skills":["missing"]}),
        json!({"tools":[""],"skills":[]}),
        json!({"tools":[],"skills":["support"]}),
        json!({"tools":vec!["lookup"; registry::MAX_SELECTIONS + 1],"skills":[]}),
    ];
    for case in cases {
        assert!(resolve(&bundle, Some(&case)).is_err(), "{case}");
    }
    assert!(
        bundle
            .resolve_agent(
                CompanyId::new(Uuid::new_v4()),
                &AgentKey::parse("assistant").unwrap(),
                None
            )
            .is_err()
    );
    assert!(
        bundle
            .resolve_agent(company(), &AgentKey::parse("unknown").unwrap(), None)
            .is_err()
    );
}

#[test]
fn workflow_capabilities_profiles_cannot_add_frozen_agent_grants() {
    let mut facts = capable();
    facts.agents[0].tools.clear();
    facts.agents[0].skills.clear();
    let bundle = build(&source("agent.run"), facts).unwrap();
    let result = resolve(&bundle, Some(&json!("support"))).unwrap();
    assert!(result.tools().is_empty() && result.skills().is_empty());
}

#[test]
fn workflow_capabilities_ceiling_intersects_both_sources_and_validates_requirements() {
    for profile in [None, Some(json!("support"))] {
        let mut src = source("agent.run");
        src["capability_ceiling"] = json!({"tools":[],"skills":[]});
        let bundle = build(&src, capable()).unwrap();
        let result = resolve(&bundle, profile.as_ref()).unwrap();
        assert!(result.tools().is_empty() && result.skills().is_empty());
        src["capability_ceiling"] = json!({"tools":["lookup"],"skills":["support"]});
        let bundle = build(&src, capable()).unwrap();
        let result = resolve(&bundle, profile.as_ref()).unwrap();
        assert_eq!(result.agent().tools, vec![name("lookup")]);
        assert_eq!(result.agent().skills, vec![name("support")]);
        src["capability_ceiling"] = json!({"tools":[],"skills":["support"]});
        let bundle = build(&src, capable()).unwrap();
        let error = resolve(&bundle, profile.as_ref())
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("support") && error.contains("lookup"),
            "{error}"
        );
    }
}

#[test]
fn workflow_capabilities_current_revocation_refuses_without_reloading_or_widening() {
    let mut facts = capable();
    let bundle = build(&source("agent.run"), facts.clone()).unwrap();
    let selected = resolve(&bundle, Some(&json!("support"))).unwrap();
    let before = serde_json::to_value(&selected).unwrap();
    for current in [
        CapabilityCeiling {
            tools: vec![name("lookup")],
            skills: vec![],
        },
        CapabilityCeiling {
            tools: vec![],
            skills: vec![name("support")],
        },
    ] {
        assert!(selected.validate_current(&current).is_err());
    }
    let broader = CapabilityCeiling {
        tools: vec![name("lookup"), name("search")],
        skills: vec![name("support"), name("style")],
    };
    selected.validate_current(&broader).unwrap();
    facts.agents[0].instructions = "Later instructions".into();
    facts.agents[0].model.model = "Later model".into();
    facts.skills[0].instructions = "Later skill".into();
    let later = build(&source("agent.run"), facts).unwrap();
    assert_ne!(later.content_hash(), bundle.content_hash());
    assert_eq!(serde_json::to_value(&selected).unwrap(), before);
    assert_eq!(before["source"], "profile");
    assert_eq!(before["profile"], "support");
    assert_eq!(before["tools"].as_array().unwrap().len(), 1);
}

#[test]
fn workflow_capabilities_ceiling_hash_and_restored_selection_match() {
    let plain = build(&source("agent.run"), capable()).unwrap();
    let mut src = source("agent.run");
    src["capability_ceiling"] = json!({"tools":["lookup"],"skills":["support"]});
    let limited = build(&src, capable()).unwrap();
    assert_ne!(plain.content_hash(), limited.content_hash());
    let encoded = store_bundle(&limited).unwrap();
    let restored = restore_bundle(
        &encoded,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )
    .unwrap();
    assert_eq!(limited.content_hash(), restored.content_hash());
    assert_eq!(
        serde_json::to_value(resolve(&limited, None).unwrap()).unwrap(),
        serde_json::to_value(resolve(&restored, None).unwrap()).unwrap()
    );
}

#[test]
fn workflow_capabilities_ceiling_is_strict_bounded_and_source_located() {
    for value in [
        Value::Null,
        json!({}),
        json!({"tools":[]}),
        json!({"tools":[],"skills":[],"extra":1}),
        json!({"tools":["lookup","lookup"],"skills":[]}),
        json!({"tools":[],"skills":["style","style"]}),
        json!({"tools":[""],"skills":[]}),
        json!({"tools":[],"skills":null}),
        json!({"tools":vec!["lookup"; registry::MAX_SELECTIONS+1],"skills":[]}),
    ] {
        let mut src = source("agent.run");
        src["capability_ceiling"] = value;
        let error = build(&src, capable()).err().unwrap();
        assert!(
            error.field_path.starts_with("/capability_ceiling"),
            "{}",
            error.field_path
        );
    }
}

#[test]
fn workflow_capabilities_resolved_reference_selects_only_the_wired_agent() {
    let mut src = source("agent.run");
    src["parameter_schema"] =
        json!({"type":"object","required":["profile"],"properties":{"profile":{"type":"string"}}});
    src["steps"]["start"]["with"]["capability_profile"] = json!({"ref":"/params/profile"});
    let wired = build(&src, capable()).unwrap();
    let params = json!({"profile":"support"});
    let input = json!({});
    let outputs = BTreeMap::new();
    let context = Context {
        input: &input,
        params: &params,
        step_outputs: &outputs,
        run: RunMetadata {
            run_id: RunId::new(Uuid::nil()),
            parent_run_id: None,
        },
    };
    let prepared = wired
        .prepare_step_inputs(&StepId::parse("start").unwrap(), &context)
        .unwrap();
    let selected = resolve(&wired, prepared.value().get("capability_profile")).unwrap();
    assert_eq!(selected.agent().tools, vec![name("lookup")]);
    src["steps"]["start"]["with"]
        .as_object_mut()
        .unwrap()
        .remove("capability_profile");
    let unwired = build(&src, capable()).unwrap();
    let prepared = unwired
        .prepare_step_inputs(&StepId::parse("start").unwrap(), &context)
        .unwrap();
    let defaults = resolve(&unwired, prepared.value().get("capability_profile")).unwrap();
    assert_eq!(defaults.source(), SelectionSource::AgentDefaults);
    assert_eq!(defaults.agent().tools, vec![name("search"), name("lookup")]);
}
