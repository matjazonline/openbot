use super::tests::*;
use super::*;
#[test]
fn workflow_registry_per_step_schemas_and_choices_remain_distinct() {
    for name in ["agent.run", "data.map", "decision.agent"] {
        let mut example = example(name).unwrap();
        changed(&mut example, |v| {
            let mut second = v["steps"]["start"].clone();
            let field = if name == "decision.agent" {
                "data_schema"
            } else {
                "output_schema"
            };
            second["with"][field] = json!({"literal":{"type":"integer"}});
            if name == "data.map" {
                second["with"]["value"] = json!({"literal":7});
            }
            if name == "decision.agent" {
                v["steps"]["start"]["routes"]["choices"]["continue"] = json!("second");
                second["routes"]["choices"] = json!({"other":"$end"});
            } else {
                v["steps"]["start"]["routes"]["success"] = json!("second");
            }
            v["steps"]["second"] = second;
        });
        let compiled = compile_example(&example).unwrap();
        let second = StepId::parse("second").unwrap();
        let first_out = if name == "decision.agent" {
            json!({"choice":"continue","data":"ok"})
        } else {
            json!("ok")
        };
        let second_out = if name == "decision.agent" {
            json!({"choice":"other","data":7})
        } else {
            json!(7)
        };
        compiled.validate_step_output(&step(), &first_out).unwrap();
        compiled.validate_step_output(&second, &second_out).unwrap();
        assert!(compiled.validate_step_output(&step(), &second_out).is_err());
        assert!(compiled.validate_step_output(&second, &first_out).is_err());
        assert_ne!(
            compiled.representation()["descriptors"]["start"],
            compiled.representation()["descriptors"]["second"]
        );
    }
}
#[test]
fn workflow_registry_static_declarations_and_nested_configuration_are_checked() {
    for (name, field, value) in [
        ("memory.load", "scope", json!({"kind":"company","extra":1})),
        ("memory.save", "scope", json!({"kind":"user"})),
        (
            "decision.human",
            "reviewer",
            json!({"kind":"user","id":"a","extra":1}),
        ),
        (
            "message.send",
            "destinations",
            json!([{"kind":"channel","id":"a","extra":1}]),
        ),
        (
            "message.reply",
            "content",
            json!({"body":"hello","extra":1}),
        ),
        (
            "agent.run",
            "capability_profile",
            json!({"tools":[],"skills":[],"extra":1}),
        ),
        ("http.request", "headers", json!({"Accept":5})),
        ("tool.call", "arguments", json!({"key":"hello","extra":1})),
        ("mcp.call", "arguments", json!({"key":3})),
        ("flow.repeat", "input", json!({"text":"hello","extra":1})),
    ] {
        let mut example = example(name).unwrap();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"][field] = json!({"literal":value})
        });
        assert!(compile_example(&example).is_err(), "{name}.{field}");
    }
    for (name, field) in [
        ("agent.run", "output_schema"),
        ("decision.rule", "data_schema"),
        ("tool.call", "tool"),
        ("mcp.call", "connection"),
        ("wait.event", "payload_schema"),
    ] {
        let mut example = example(name).unwrap();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"][field] = json!({"ref":"/input"})
        });
        let error = compile_example(&example).err().unwrap();
        assert_eq!(error.code, "registry.literal");
        assert_eq!(
            error.field_path.as_ref(),
            format!("/steps/start/with/{field}")
        );
        assert!(error.span.end > error.span.start);
    }
}
#[test]
fn workflow_registry_choice_resource_child_deadline_and_schema_errors() {
    for (name, edit) in [
        ("decision.rule", 0),
        ("decision.human", 1),
        ("http.request", 2),
        ("mcp.call", 3),
        ("workflow.call", 4),
        ("flow.repeat", 5),
        ("flow.repeat", 6),
        ("wait.timer", 7),
        ("ai.classify", 8),
        ("wait.event", 9),
    ] {
        let mut example = example(name).unwrap();
        changed(&mut example, |v| match edit {
            0 => v["steps"]["start"]["rule"]["default"] = json!("unknown"),
            1 => v["steps"]["start"]["with"]["feedback_required"] = json!({"literal":["unknown"]}),
            2 => v["resources"] = json!([]),
            3 => v["resources"][0]["kind"] = json!("http"),
            4 => {
                v["steps"]["start"]["child_version_id"] =
                    json!("00000000-0000-0000-0000-000000000004")
            }
            5 => v["steps"]["start"]["max_iterations"] = json!(0),
            6 => v["steps"]["start"]["max_iterations"] = json!(10001),
            7 => v["steps"]["start"]["with"]["deadline"] = json!({"literal":"tomorrow"}),
            8 => {
                v["steps"]["start"]["with"]["output_schema"] = json!({"literal":{"type":"string"}})
            }
            _ => {
                v["steps"]["start"]["with"]["payload_schema"] =
                    json!({"literal":{"type":"not-a-type"}})
            }
        });
        assert!(compile_example(&example).is_err(), "{name} edit {edit}");
    }
}
#[test]
fn workflow_registry_native_schema_resources_survive_nested_contracts() {
    let mut example = example("data.map").unwrap();
    let schema = json!({"$defs":{"text":{"type":"string","minLength":2}},"$ref":"#/$defs/text","maxLength":5});
    changed(&mut example, |v| {
        v["steps"]["start"]["with"]["output_schema"] = json!({"literal":schema})
    });
    let compiled = compile_example(&example).unwrap();
    assert!(compiled.validate_step_output(&step(), &json!("a")).is_err());
    assert!(
        compiled
            .validate_step_output(&step(), &json!("toolong"))
            .is_err()
    );
    compiled
        .validate_step_output(&step(), &json!("good"))
        .unwrap();
}
