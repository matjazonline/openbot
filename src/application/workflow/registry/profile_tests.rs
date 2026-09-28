use super::tests::*;
use super::*;
fn profile_fact() -> CapabilityProfile {
    CapabilityProfile {
        name: TypeName::parse("support").unwrap(),
        tools: vec![TypeName::parse("lookup").unwrap()],
        skills: vec![],
    }
}
fn prepared(compiled: &CompiledWorkflow, input: Value) -> Result<Value, Diagnostic> {
    let params = json!({});
    let run = json!({"id":"run","parent_id":null});
    let outputs = BTreeMap::new();
    compiled
        .prepare_step_inputs(&step(), &context(&input, &params, &outputs, &run))
        .map(|p| p.value().clone())
}
#[test]
fn workflow_registry_profile_omission_empty_inline_name_and_runtime_selection() {
    let example = example("agent.run").unwrap();
    let compiled = compile_example(&example).unwrap();
    assert!(
        prepared(&compiled, json!({}))
            .unwrap()
            .get("capability_profile")
            .is_none()
    );
    assert_eq!(
        compiled.representation()["descriptors"]["start"]["registration"]["constraints"]["profile_selection"],
        "agent_defaults"
    );
    for profile in [
        json!({"tools":[],"skills":[]}),
        json!({"tools":["lookup"],"skills":["draft"]}),
        json!("support"),
    ] {
        let mut example = super::example("agent.run").unwrap();
        example.facts.profiles.push(profile_fact());
        changed(&mut example, |v| {
            v["steps"]["start"]["with"]["capability_profile"] = json!({"literal":profile})
        });
        let compiled = compile_example(&example).unwrap();
        assert_eq!(
            prepared(&compiled, json!({})).unwrap()["capability_profile"],
            profile
        );
        assert_eq!(
            compiled.representation()["descriptors"]["start"]["registration"]["constraints"]["profile_selection"],
            "explicit_selection"
        );
    }
    let mut example = super::example("agent.run").unwrap();
    example.facts.profiles.push(profile_fact());
    changed(&mut example, |v| {
        v["input_schema"] =
            json!({"type":"object","properties":{"selection":true},"required":["selection"]});
        v["steps"]["start"]["with"]["capability_profile"] = json!({"ref":"/input/selection"});
    });
    let compiled = compile_example(&example).unwrap();
    for value in [json!("support"), json!({"tools":[],"skills":[]})] {
        assert_eq!(
            prepared(&compiled, json!({"selection":value})).unwrap()["capability_profile"],
            value
        );
    }
    for value in [
        Value::Null,
        json!("unknown"),
        json!({"tools":[]}),
        json!({"tools":[],"skills":[],"extra":true}),
        json!({"tools":["lookup","lookup"],"skills":[]}),
    ] {
        assert!(prepared(&compiled, json!({"selection":value})).is_err());
    }
    assert!(prepared(&compiled, json!({})).is_err());
}
#[test]
fn workflow_registry_profile_invalid_static_and_optional_reference_fail() {
    for selection in [
        Value::Null,
        json!("unknown"),
        json!({}),
        json!({"tools":[],"skills":null}),
        json!({"tools":[],"skills":[],"grants":[]}),
    ] {
        let mut example = example("agent.run").unwrap();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"]["capability_profile"] = json!({"literal":selection})
        });
        assert!(compile_example(&example).is_err());
    }
    let mut example = example("agent.run").unwrap();
    changed(&mut example, |v| {
        v["input_schema"] = json!({"type":"object","properties":{"selection":{"type":"string"}}});
        v["steps"]["start"]["with"]["capability_profile"] = json!({"ref":"/input/selection"});
    });
    assert_eq!(
        compile_example(&example).err().unwrap().code,
        "reference.optional"
    );
}
#[test]
fn workflow_registry_unwired_classifier_does_not_select_agent_profile() {
    let mut example = example("agent.run").unwrap();
    changed(&mut example, |v| {
        v["entry"] = json!("classify");
        v["steps"]["classify"] = json!({"type":"ai.classify","with":{"context":{"literal":{}},"output_schema":{"literal":{"type":"string","enum":["support"]}}},"routes":{"success":"start"}});
    });
    let compiled = compile_example(&example).unwrap();
    assert_eq!(
        compiled.representation()["descriptors"]["start"]["registration"]["constraints"]["profile_selection"],
        "agent_defaults"
    );
}
#[test]
fn workflow_registry_effective_profile_facts_change_hash_and_order_does_not() {
    let mut example = example("agent.run").unwrap();
    example.facts.profiles.push(profile_fact());
    let first = compile_example(&example).unwrap().content_hash().to_owned();
    example.facts.profiles[0]
        .skills
        .push(TypeName::parse("draft").unwrap());
    let second = compile_example(&example).unwrap().content_hash().to_owned();
    assert_ne!(first, second);
    example.facts.profiles.push(CapabilityProfile {
        name: TypeName::parse("empty").unwrap(),
        tools: vec![],
        skills: vec![],
    });
    let third = compile_example(&example).unwrap().content_hash().to_owned();
    example.facts.profiles.reverse();
    assert_eq!(third, compile_example(&example).unwrap().content_hash());
}
