use super::tests::{changed, compile_example, step};
use super::*;
use uuid::Uuid;

#[test]
fn channel_reply_terminal_contract_roundtrip_and_identity() {
    let sample = example("channel.no_reply").unwrap();
    let compiled = compile_example(&sample).unwrap();
    let roundtrip = compile(
        crate::adapters::workflow_source::decode(compiled.source()).unwrap(),
        VersionId::new(Uuid::nil()),
        &sample.facts,
        &sample.dependencies,
    )
    .unwrap();
    assert_eq!(roundtrip.representation(), compiled.representation());
    assert_eq!(compiler::SEMANTIC_REVISION, 12);
    assert_eq!(
        compiled.representation()["descriptors"]["start"]["registration"]["revision"],
        CONTRACT_REVISION
    );
    assert_eq!(
        metadata::constraints("channel.no_reply").effect,
        EffectClass::Pure
    );
    let ordinary = compile_example(&example("data.map").unwrap()).unwrap();
    assert_ne!(compiled.representation(), ordinary.representation());
}

#[test]
fn channel_reply_terminal_contract_rejects_nonterminal_and_choice_routes() {
    for routes in [json!({"success":"next"}), json!({"choices":{"yes":"$end"}})] {
        let mut sample = example("channel.no_reply").unwrap();
        changed(&mut sample, |source| {
            source["steps"]["start"]["routes"] = routes;
            source["steps"]["next"] = json!({"type":"data.map","with":{
                "value":{"literal":"done"},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
        });
        assert_eq!(
            compile_example(&sample).err().unwrap().code,
            "registry.terminal_route"
        );
    }
}

#[test]
fn channel_reply_terminal_contract_retains_error_routing_and_typed_value() {
    let mut sample = example("channel.no_reply").unwrap();
    changed(&mut sample, |source| {
        source["steps"]["start"]["routes"]["error"] = json!("recover");
        source["steps"]["recover"] = json!({"type":"data.map","with":{
            "value":{"literal":"recovered"},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    });
    let compiled = compile_example(&sample).unwrap();
    assert!(compiled.validate_step_output(&step(), &json!(7)).is_err());
    assert!(
        compiled
            .validate_step_output(&step(), &json!("done"))
            .is_ok()
    );
}

#[test]
fn channel_reply_terminal_contract_accepts_schema_valid_empty_business_values() {
    for value in [json!(null), json!(""), json!({}), json!([])] {
        let mut sample = example("channel.no_reply").unwrap();
        changed(&mut sample, |source| {
            source["steps"]["start"]["with"] = json!({
                "value":{"literal":value},"output_schema":{"literal":true}});
        });
        let compiled = compile_example(&sample).unwrap();
        compiled.validate_step_output(&step(), &value).unwrap();
        compiled.validate_output(&value).unwrap();
    }
}

fn preflight_sample(kind: &str, padding: usize) -> AuthoringExample {
    let mut sample = example(kind).unwrap();
    let tool = example("tool.call").unwrap();
    let tool_source: Value = serde_json::from_str(&tool.source).unwrap();
    sample.facts = tool.facts;
    let schema = json!({"type":"object","$comment":"x".repeat(padding)});
    sample.facts.tools[0].input_schema = schema.clone();
    sample.facts.tools[0].output_schema = schema;
    changed(&mut sample, |source| {
        let mut terminal = source["steps"]["start"].clone();
        terminal["with"]["output_schema"] = json!({"literal":{
            "type":"string","$comment":"x".repeat(8_128)}});
        source["steps"] = json!({"finish":terminal});
        source["entry"] = json!("t00");
        for index in 0..16 {
            let mut node = tool_source["steps"]["start"].clone();
            node["routes"]["success"] = json!(if index == 15 {
                "finish".to_owned()
            } else {
                format!("t{:02}", index + 1)
            });
            source["steps"][format!("t{index:02}")] = node;
        }
    });
    sample
}

#[test]
fn channel_reply_terminal_contract_preflight_charges_all_schema_copies() {
    // Shared external facts fill the budget without exceeding the wire-source cap.
    // At 16_025 bytes of padding, two terminal copies fit but three do not.
    let check = |kind, padding| {
        let sample = preflight_sample(kind, padding);
        let decoded = crate::adapters::workflow_source::decode(&sample.source).unwrap();
        let parsed = compiler::parse_workflow(&decoded, VersionId::new(Uuid::nil())).unwrap();
        budget::preflight(&parsed, &sample.facts)
    };
    for kind in ["data.map", "channel.no_reply"] {
        assert_eq!(
            check(kind, 16_025).expect_err(kind).code,
            "dependency.limit"
        );
    }
    check("channel.no_reply", 15_500).unwrap();
}
