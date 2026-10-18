use super::tests::*;
use super::*;

#[test]
fn workflow_registry_action_approval_reviewers_are_static_and_bounded() {
    let reviewer = json!({"kind":"user","id":"00000000-0000-0000-0000-000000000006"});
    for kind in [
        "http.request",
        "tool.call",
        "mcp.call",
        "message.send",
        "message.reply",
    ] {
        let mut valid = example(kind).unwrap();
        changed(&mut valid, |v| {
            v["steps"]["start"]["with"]["approval"] = json!({"literal":{
                "reviewers":[reviewer.clone()],"deadline":"2099-01-01T00:00:00Z"
            }});
        });
        compile_example(&valid).unwrap();
        for reviewers in [
            json!([]),
            json!([reviewer.clone(), reviewer.clone()]),
            json!([{"kind":"group","id":"hidden"}]),
        ] {
            let mut invalid = example(kind).unwrap();
            changed(&mut invalid, |v| {
                v["steps"]["start"]["with"]["approval"] = json!({"literal":{
                    "reviewers":reviewers,"deadline":"2099-01-01T00:00:00Z"
                }});
            });
            assert!(compile_example(&invalid).is_err(), "{kind}");
        }
        changed(&mut valid, |v| {
            v["input_schema"] = json!({"type":"object","properties":{"reviewers":true}});
            v["steps"]["start"]["with"]["approval"] = json!({"object":{
                "reviewers":{"ref":"/input/reviewers"},"deadline":{"literal":"2099-01-01T00:00:00Z"}
            }});
        });
        assert_eq!(
            compile_example(&valid).err().unwrap().code,
            "registry.static_reviewers"
        );
    }
}

#[test]
fn workflow_registry_action_approval_allows_dynamic_deadline_but_validates_it() {
    let mut example = example("http.request").unwrap();
    changed(&mut example, |v| {
        v["input_schema"] = json!({"type":"object","properties":{"deadline":{"type":"string"}},"required":["deadline"]});
        v["parameter_schema"] =
            json!({"type":"object","properties":{"reviewers":true},"required":["reviewers"]});
        v["steps"]["start"]["with"]["approval"] = json!({"object":{
            "reviewers":{"ref":"/params/reviewers"},"deadline":{"ref":"/input/deadline"}
        }});
    });
    let compiled = compile_example(&example).unwrap();
    let params = json!({"reviewers":[{"kind":"user","id":"00000000-0000-0000-0000-000000000006"}]});
    for (deadline, valid) in [("2099-01-01T00:00:00Z", true), ("tomorrow", false)] {
        let input = json!({"deadline":deadline});
        let outputs = BTreeMap::new();
        let run = json!({});
        let result =
            compiled.prepare_step_inputs(&step(), &context(&input, &params, &outputs, &run));
        assert_eq!(result.is_ok(), valid);
    }
}

#[test]
fn workflow_registry_decision_reviewers_static_nested_and_bounded() {
    let reviewer = json!({"kind":"user","id":"00000000-0000-0000-0000-000000000006"});
    for invalid in [
        json!([]),
        json!([reviewer.clone(), reviewer.clone()]),
        json!([{"kind":"group","id":"00000000-0000-0000-0000-000000000006"}]),
        json!([{"kind":"external","email":"user@example.test"}]),
        json!([{"kind":"user","id":"not-a-uuid"}]),
        json!(
            (1..=17)
                .map(|id| json!({"kind":"user","id":uuid::Uuid::from_u128(id)}))
                .collect::<Vec<_>>()
        ),
    ] {
        let mut example = example("decision.human").unwrap();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"]["reviewers"] = json!({"literal":invalid})
        });
        assert!(compile_example(&example).is_err());
    }
    let mut sixteen = example("decision.human").unwrap();
    changed(
        &mut sixteen,
        |v| v["steps"]["start"]["with"]["reviewers"] = json!({"literal":(1..=16).map(|id| json!({"kind":"user","id":uuid::Uuid::from_u128(id)})).collect::<Vec<_>>()}),
    );
    compile_example(&sixteen).unwrap();
    for binding in [
        json!({"ref":"/input/reviewers"}),
        json!({"array":[{"object":{"kind":{"literal":"user"},"id":{"ref":"/input/id"}}}]}),
        json!({"default":{"ref":"/input/reviewers","value":{"literal":[reviewer.clone()]}}}),
    ] {
        let mut example = example("decision.human").unwrap();
        changed(&mut example, |v| {
            v["input_schema"] = json!({"type":"object","properties":{"reviewers":true,"id":true}});
            v["steps"]["start"]["with"]["reviewers"] = binding;
        });
        assert_eq!(
            compile_example(&example).err().unwrap().code,
            "registry.static_reviewers"
        );
    }
    let mut example = example("decision.human").unwrap();
    changed(&mut example, |v| {
        v["parameter_schema"] =
            json!({"type":"object","properties":{"reviewers":true},"required":["reviewers"]});
        v["steps"]["start"]["with"]["reviewers"] = json!({"ref":"/params/reviewers"});
    });
    let compiled = compile_example(&example).unwrap();
    let (input, params, outputs, run) = (
        json!({}),
        json!({"reviewers":[reviewer]}),
        BTreeMap::new(),
        json!({}),
    );
    compiled
        .prepare_step_inputs(&step(), &context(&input, &params, &outputs, &run))
        .unwrap();
    let invalid = json!({"reviewers":[{"kind":"group","id":"hidden"}]});
    assert!(
        compiled
            .prepare_step_inputs(&step(), &context(&input, &invalid, &outputs, &run))
            .is_err()
    );
}

#[test]
fn workflow_registry_event_name_is_literal_and_has_one_schema() {
    let mut example = example("wait.event").unwrap();
    changed(&mut example, |v| {
        v["steps"]["second"] = v["steps"]["start"].clone();
        v["steps"]["start"]["routes"]["success"] = json!("second");
    });
    compile_example(&example).unwrap();
    let mut conflicting = super::example("wait.event").unwrap();
    conflicting.source = example.source.clone();
    changed(&mut conflicting, |v| {
        v["steps"]["second"]["with"]["payload_schema"] = json!({"literal":{"type":"integer"}})
    });
    assert_eq!(
        compile_example(&conflicting).err().unwrap().code,
        "registry.event_schema"
    );
    changed(&mut example, |v| {
        v["steps"]["start"]["with"]["event"] = json!({"ref":"/input/name"})
    });
    assert_eq!(
        compile_example(&example).err().unwrap().code,
        "registry.literal"
    );
}

#[test]
fn workflow_decision_digest_survives_jsonb_numeric_normalization() {
    use crate::application::workflow::decisions::hex_digest;
    assert_eq!(
        hex_digest(&json!({"b":[-0.0,1.0],"a":2})).unwrap(),
        hex_digest(&json!({"a":2,"b":[0,1]})).unwrap()
    );
    let mut deep = json!(0);
    for _ in 0..66 {
        deep = json!([deep]);
    }
    assert!(hex_digest(&deep).is_err());
}

#[test]
fn workflow_registry_decision_object_cannot_be_disclosed_as_message_content() {
    let mut example = example("message.send").unwrap();
    changed(&mut example, |v| {
        v["steps"]["send"] = v["steps"]["start"].clone();
        v["steps"]["send"]["with"]["content"] = json!({"ref":"/steps/start/output"});
        let review = super::example("decision.human").unwrap();
        let review: Value = serde_json::from_str(&review.source).unwrap();
        v["steps"]["start"] = review["steps"]["start"].clone();
        v["steps"]["start"]["routes"]["choices"] = json!({"accept":"send","revise":"send"});
    });
    let compiled = compile_example(&example).unwrap();
    let outputs = BTreeMap::from([(
        step(),
        json!({"choice":"accept","data":"approved text","feedback":"private discussion"}),
    )]);
    let empty = json!({});
    assert!(
        compiled
            .prepare_step_inputs(
                &StepId::parse("send").unwrap(),
                &context(&empty, &empty, &outputs, &empty),
            )
            .is_err()
    );
    changed(&mut example, |v| {
        v["steps"]["send"]["with"]["content"] =
            json!({"object":{"body":{"ref":"/steps/start/output/data"}}});
    });
    let compiled = compile_example(&example).unwrap();
    let prepared = compiled
        .prepare_step_inputs(
            &StepId::parse("send").unwrap(),
            &context(&empty, &empty, &outputs, &empty),
        )
        .unwrap();
    assert_eq!(prepared.value()["content"], json!({"body":"approved text"}));
}
