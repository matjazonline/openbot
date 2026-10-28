use super::tests::*;
use super::*;

fn sample() -> AuthoringExample {
    example("jev.evaluate").unwrap()
}
fn output() -> Value {
    json!({"model":"jev-1.14.0","answers":{
        "department":{"type":"choice","choice":"billing","probabilities":{"billing":0.8,"technical":0.2},"confidence":0.6},
        "urgent":{"type":"noul","noul":0.9},
        "severity":{"type":"score","score":1.5,"probabilities":{"0":0.0,"1":0.5,"2":0.5},"legend":{"0":"Cosmetic","1":"Workaround available","2":"Blocking"},"confidence":0.5}
    },"usage":{"input_tokens":25,"output_tokens":10}})
}
fn set_question(example: &mut AuthoringExample, question: Value) {
    changed(example, |source| {
        source["steps"]["start"]["with"]["questions"] = json!({"literal":{"q":question}})
    });
}
#[test]
fn jev_contract_mixed_exact_schema_and_metadata() {
    let compiled = compile_example(&sample()).unwrap();
    compiled.validate_step_output(&step(), &output()).unwrap();
    let constraints =
        &compiled.representation()["descriptors"]["start"]["registration"]["constraints"];
    assert_eq!(constraints["effect"], "model_invocation");
    assert_eq!(constraints["recovery"], "reuse_committed_result");
    assert_eq!(constraints["shared_action_required"], false);
    assert_eq!(
        constraints["capabilities"],
        json!(["model_invoke", "resource_use"])
    );
    assert_eq!(ModelStep::of("jev.evaluate"), None);
    assert!(crate::application::workflow::completion::is_io_kind(
        "jev.evaluate"
    ));
    assert!(crate::application::workflow::actions::effect_free_kind(
        "jev.evaluate"
    ));
}
#[test]
fn jev_contract_static_fields_models_and_resources() {
    for field in ["connection", "model", "questions"] {
        let mut example = sample();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"][field] = json!({"ref":"/input"})
        });
        assert!(compile_example(&example).is_err(), "{field}");
    }
    for model in [
        "jev-latest",
        "jev-preview",
        "jev-1.14",
        "jev-1.14.0-beta",
        "jev-01.14.0",
        "other-1.14.0",
        "jev-1.14.x",
    ] {
        let mut example = sample();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"]["model"] = json!({"literal":model})
        });
        assert!(compile_example(&example).is_err(), "{model}");
    }
    for resources in [
        json!([]),
        json!([{"slot":"service","kind":"http"}]),
        json!([{"slot":"unused","kind":"typesafe"},{"slot":"service","kind":"typesafe"}]),
    ] {
        let mut example = sample();
        changed(&mut example, |v| v["resources"] = resources);
        assert!(compile_example(&example).is_err());
    }
}
#[test]
fn jev_contract_rejects_invalid_question_shapes() {
    for question in [
        json!({"type":"box","instructions":"Find it"}),
        json!({"type":"noul","instructions":"?","extra":1}),
        json!({"type":"noul"}),
        json!({"type":"noul","instructions":{}}),
        json!({"type":"noul","instructions":"?","criteria":null}),
        json!({"type":"noul","instructions":"?","criteria":{"true":"yes"}}),
        json!({"type":"noul","instructions":"?","criteria":{"true":"yes","false":"no","maybe":"unclear"}}),
        json!({"type":"choice","instructions":"?","criteria":{}}),
        json!({"type":"choice","instructions":"?","criteria":{" ":"bad"}}),
        json!({"type":"choice","instructions":"?","criteria":{"ok":null}}),
        json!({"type":"score","instructions":"?","criteria":["only"]}),
        json!({"type":"score","instructions":"?","criteria":["low",3]}),
        json!({"type":"noul","instructions":"  "}),
    ] {
        let mut example = sample();
        set_question(&mut example, question.clone());
        assert!(compile_example(&example).is_err(), "{question}");
    }
    for questions in [
        json!({}),
        json!([]),
        json!({" ":{"type":"noul","instructions":"?"}}),
    ] {
        let mut example = sample();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"]["questions"] = json!({"literal":questions})
        });
        assert!(compile_example(&example).is_err());
    }
}
#[test]
fn jev_contract_duplicate_keys_and_nonfinite_source_rejected() {
    let original = sample().source;
    for malformed in [
        original.replace("\"urgent\":", "\"department\":"),
        original.replace(
            "\"billing\":\"Payment issues\"",
            "\"technical\":\"Payment issues\"",
        ),
        original.replace("\"Payment failed\"", ".nan"),
    ] {
        let result = crate::adapters::workflow_source::decode(&malformed).and_then(|decoded| {
            compile(
                decoded,
                VersionId::new(uuid::Uuid::nil()),
                &CatalogueFacts::default(),
                &BTreeMap::new(),
            )
        });
        assert!(result.is_err());
    }
}
#[test]
fn jev_contract_primitive_limits_fail_at_next_item() {
    for count in [32, 33] {
        let mut example = sample();
        let questions: serde_json::Map<_, _> = (0..count)
            .map(|i| (format!("q{i}"), json!({"type":"noul","instructions":"?"})))
            .collect();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"]["questions"] = json!({"literal":questions})
        });
        assert_eq!(compile_example(&example).is_ok(), count == 32);
    }
    for count in [64, 65] {
        let mut example = sample();
        let options: serde_json::Map<_, _> = (0..count)
            .map(|i| (format!("o{i}"), json!("Description")))
            .collect();
        set_question(
            &mut example,
            json!({"type":"choice","instructions":"?","criteria":options}),
        );
        assert_eq!(compile_example(&example).is_ok(), count == 64);
    }
    for count in [2, 10, 11] {
        let mut example = sample();
        set_question(
            &mut example,
            json!({"type":"score","instructions":"?","criteria":vec!["Level"; count]}),
        );
        assert_eq!(compile_example(&example).is_ok(), count <= 10);
    }
}
#[test]
fn jev_contract_text_and_identifier_limits_count_utf8_bytes() {
    for text in [
        "x".repeat(16384),
        "é".repeat(8192),
        "x".repeat(16385),
        "é".repeat(8193),
    ] {
        let mut example = sample();
        set_question(&mut example, json!({"type":"noul","instructions":text}));
        assert_eq!(compile_example(&example).is_ok(), text.len() <= 16384);
    }
    for len in [128, 129] {
        let mut example = sample();
        let mut questions = serde_json::Map::new();
        questions.insert("q".repeat(len), json!({"type":"noul","instructions":"?"}));
        changed(&mut example, |v| {
            v["steps"]["start"]["with"]["questions"] = json!({"literal":questions})
        });
        assert_eq!(compile_example(&example).is_ok(), len == 128);
    }
}
#[test]
fn jev_contract_request_budget_counts_envelope_and_escaping() {
    let mut inputs = json!({"model":"jev-1.14.0","questions":{"q":{"type":"noul","instructions":"?"}},"state":""});
    let overhead = serde_json::to_vec(&inputs).unwrap().len();
    inputs["state"] = json!("x".repeat(jev::MAX_REQUEST_BYTES - overhead));
    jev::validate_inputs(&inputs).unwrap();
    inputs["state"] = json!("x".repeat(jev::MAX_REQUEST_BYTES - overhead + 1));
    assert!(jev::validate_inputs(&inputs).is_err());
    inputs["state"] = json!("\n".repeat(32768));
    assert!(jev::validate_inputs(&inputs).is_err());
    let mut example = sample();
    changed(&mut example, |v| {
        v["steps"]["start"]["with"]["state"] = json!({"literal":"x".repeat(65536)})
    });
    assert!(compile_example(&example).is_err());
}
#[test]
fn jev_contract_dynamic_state_checks_and_workflow_context_budget() {
    let mut example = sample();
    changed(&mut example, |v| {
        v["limits"]["max_context_bytes"] = json!(131072);
        v["steps"]["start"]["with"]["state"] = json!({"ref":"/input"});
    });
    let compiled = compile_example(&example).unwrap();
    let params = json!({});
    let run = json!({});
    let outputs = BTreeMap::new();
    for input in [
        json!("text"),
        json!({"ticket":"hello"}),
        json!(["one", "two"]),
    ] {
        compiled
            .prepare_step_inputs(&step(), &context(&input, &params, &outputs, &run))
            .unwrap();
    }
    for input in [json!(null), json!(true), json!(3), json!("x".repeat(65536))] {
        assert!(
            compiled
                .prepare_step_inputs(&step(), &context(&input, &params, &outputs, &run))
                .is_err()
        );
    }
    changed(&mut example, |v| {
        v["limits"]["max_context_bytes"] = json!(1024)
    });
    let compiled = compile_example(&example).unwrap();
    assert!(
        compiled
            .prepare_step_inputs(
                &step(),
                &context(&json!("x".repeat(1024)), &params, &outputs, &run)
            )
            .is_err()
    );
}
#[test]
fn jev_contract_rejects_malformed_outputs() {
    let compiled = compile_example(&sample()).unwrap();
    for (path, replacement) in [
        ("/model", json!("jev-1.13.0")),
        ("/usage/input_tokens", json!(-1)),
        ("/usage/output_tokens", json!(0.5)),
        ("/answers/urgent/type", json!("choice")),
        ("/answers/urgent/noul", json!(true)),
        ("/answers/urgent/noul", json!(1.01)),
        ("/answers/urgent/noul", json!(null)),
        ("/answers/department/choice", json!("other")),
        ("/answers/department/confidence", json!(-0.1)),
        ("/answers/department/probabilities/billing", json!(2)),
        ("/answers/severity/score", json!(2.1)),
        ("/answers/severity/legend/0", json!("Different")),
        ("/answers/severity/probabilities", json!({"0":0.5,"1":0.5})),
    ] {
        let mut invalid = output();
        *invalid.pointer_mut(path).unwrap() = replacement;
        assert!(
            compiled.validate_step_output(&step(), &invalid).is_err(),
            "{path}: {invalid}"
        );
    }
    for path in [
        "",
        "/answers",
        "/answers/urgent",
        "/answers/department",
        "/answers/severity",
        "/answers/severity/legend",
        "/answers/department/probabilities",
        "/usage",
    ] {
        let mut invalid = output();
        invalid
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(0));
        assert!(
            compiled.validate_step_output(&step(), &invalid).is_err(),
            "extra at {path}"
        );
        let mut invalid = output();
        let object = invalid.pointer_mut(path).unwrap().as_object_mut().unwrap();
        let key = object.keys().next().unwrap().clone();
        object.remove(&key);
        assert!(
            compiled.validate_step_output(&step(), &invalid).is_err(),
            "missing at {path}"
        );
    }
}
#[test]
fn jev_contract_probability_sum_tolerance() {
    let compiled = compile_example(&sample()).unwrap();
    for (probability, valid) in [(0.2000005, true), (0.200002, false), (0.1, false)] {
        let mut value = output();
        value["answers"]["department"]["probabilities"]["technical"] = json!(probability);
        assert_eq!(
            compiled.validate_step_output(&step(), &value).is_ok(),
            valid
        );
    }
}
#[test]
fn jev_contract_downstream_paths_are_exact_and_typed() {
    for (pointer, schema, valid) in [
        ("/answers/urgent/noul", json!({"type":"number"}), true),
        ("/answers/department/choice", json!({"type":"string"}), true),
        ("/answers/severity/legend/0", json!({"type":"string"}), true),
        ("/answers/severity/score", json!({"type":"number"}), true),
        ("/answers/missing/noul", json!({"type":"number"}), false),
        (
            "/answers/urgent/confidence",
            json!({"type":"number"}),
            false,
        ),
        (
            "/answers/department/probabilities/missing",
            json!({"type":"number"}),
            false,
        ),
        (
            "/answers/severity/legend/3",
            json!({"type":"string"}),
            false,
        ),
    ] {
        let mut example = sample();
        changed(&mut example, |v| {
            v["steps"]["start"]["routes"]["success"] = json!("next");
            v["steps"]["next"] = json!({"type":"data.map","with":{"value":{"ref":format!("/steps/start/output{pointer}")},"output_schema":{"literal":schema}},"routes":{"success":"$end"}});
        });
        assert_eq!(compile_example(&example).is_ok(), valid, "{pointer}");
    }
}
#[test]
fn jev_contract_frozen_semantics_change_hash() {
    let original = compile_example(&sample())
        .unwrap()
        .content_hash()
        .to_owned();
    for (field, value) in [
        ("model", json!("jev-1.13.0")),
        (
            "questions",
            json!({"urgent":{"type":"noul","instructions":"Different question"}}),
        ),
        ("state", json!("Different state")),
    ] {
        let mut example = sample();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"][field] = json!({"literal":value})
        });
        assert_ne!(compile_example(&example).unwrap().content_hash(), original);
    }
}
#[test]
fn jev_contract_noul_criteria_and_escaped_question_paths() {
    let mut example = sample();
    set_question(
        &mut example,
        json!({"type":"noul","instructions":"?","criteria":{"true":"Yes","false":"No"}}),
    );
    let compiled = compile_example(&example).unwrap();
    compiled.validate_step_output(&step(), &json!({"model":"jev-1.14.0","answers":{"q":{"type":"noul","noul":0}},"usage":{"input_tokens":0,"output_tokens":0}})).unwrap();
    changed(&mut example, |v| {
        let q = v["steps"]["start"]["with"]["questions"]["literal"]["q"].clone();
        v["steps"]["start"]["with"]["questions"]["literal"] = json!({"a/b~c":q});
        v["steps"]["start"]["routes"]["success"] = json!("next");
        v["steps"]["next"] = json!({"type":"data.map","with":{"value":{"ref":"/steps/start/output/answers/a~1b~0c/noul"},"output_schema":{"literal":{"type":"number"}}},"routes":{"success":"$end"}});
    });
    compile_example(&example).unwrap();
}

#[test]
fn jev_contract_downstream_runtime_never_coerces_probability_to_boolean() {
    let mut example = sample();
    changed(&mut example, |v| {
        v["steps"]["start"]["routes"]["success"] = json!("next");
        v["steps"]["next"] = json!({"type":"data.map","with":{"value":{"ref":"/steps/start/output/answers/urgent/noul"},"output_schema":{"literal":{"type":"boolean"}}},"routes":{"success":"$end"}});
    });
    // The compiler proves reference availability, while resolved values are checked against
    // destination schemas at preparation (the existing generic binding contract).
    let compiled = compile_example(&example).unwrap();
    let outputs = BTreeMap::from([(step(), output())]);
    assert!(
        compiled
            .prepare_step_inputs(
                &StepId::parse("next").unwrap(),
                &context(&json!({}), &json!({}), &outputs, &json!({}))
            )
            .is_err()
    );
}

#[test]
fn jev_contract_response_byte_limit_and_aggregate_questions() {
    let response = json!({"answers":{},"padding":"x".repeat(jev::MAX_RESPONSE_BYTES)});
    assert!(jev::validate_output(&response).is_err());
    let mut example = sample();
    let questions: serde_json::Map<_, _> = (0..5)
        .map(|i| {
            (
                format!("q{i}"),
                json!({"type":"noul","instructions":"x".repeat(16384)}),
            )
        })
        .collect();
    changed(&mut example, |v| {
        v["steps"]["start"]["with"]["questions"] = json!({"literal":questions})
    });
    assert!(compile_example(&example).is_err());
}
