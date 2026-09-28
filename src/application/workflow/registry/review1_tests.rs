use super::tests::*;
use super::*;

#[test]
fn workflow_registry_dialects_follow_active_custom_targets_only() {
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
    let old = json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"string"});
    for schema in [
        json!({"$ref":"#/custom","custom":old}),
        json!({"$ref":"#/custom","custom":{"$ref":"#/data/next"},"data":{"next":old}}),
        json!({"$dynamicRef":"#/custom","custom":old}),
        json!({"$id":"https://local.invalid/root","properties":{"value":{"$id":"child","$ref":"#/custom","custom":old}}}),
    ] {
        assert_eq!(
            Schema::compile(schema, "/schema", span).err().unwrap().code,
            "schema.dialect"
        );
    }
    for schema in [
        json!({"custom":old}),
        json!({"const":old}),
        json!({"enum":[old]}),
    ] {
        Schema::compile(schema, "/schema", span).unwrap();
    }
    let valid = Schema::compile(json!({"$ref":"#/custom","custom":{"$ref":"#/data/next"},"data":{"next":{"type":"string"}}}), "/schema", span).unwrap();
    valid.validate(&json!("ok"), "/", span).unwrap();
    assert!(valid.validate(&json!(12), "/", span).is_err());
}

#[test]
fn workflow_registry_classifier_contract_excludes_scalar_loopholes() {
    for kind in [
        Value::Null,
        json!(["object", "string"]),
        json!(["object", "number"]),
    ] {
        let mut example = example("ai.classify").unwrap();
        changed(&mut example, |v| {
            let mut schema =
                json!({"properties":{"label":{"enum":["support"]}},"required":["label"]});
            if !kind.is_null() {
                schema["type"] = kind;
            }
            v["steps"]["start"]["with"]["output_schema"] = json!({"literal":schema});
        });
        assert_eq!(
            compile_example(&example).err().unwrap().code,
            "registry.labels"
        );
    }
    for field in ["label", "profile"] {
        let mut example = example("ai.classify").unwrap();
        changed(&mut example, |v| {
            let mut properties = json!({});
            properties[field] = json!({"enum":["support"]});
            v["steps"]["start"]["with"]["output_schema"] =
                json!({"literal":{"type":"object","properties":properties,"required":[field]}});
        });
        let compiled = compile_example(&example).unwrap();
        let mut valid = json!({});
        valid[field] = json!("support");
        compiled.validate_step_output(&step(), &valid).unwrap();
        valid[field] = json!("unknown");
        assert!(compiled.validate_step_output(&step(), &valid).is_err());
        for bad in [
            json!("support"),
            json!("unknown"),
            json!(42),
            json!({}),
            json!({field:"unknown"}),
        ] {
            assert!(compiled.validate_step_output(&step(), &bad).is_err());
        }
    }
    let mut example = example("ai.classify").unwrap();
    changed(&mut example, |v| {
        v["steps"]["start"]["with"]["output_schema"] = json!({"literal":{"enum":["support"]}})
    });
    let compiled = compile_example(&example).unwrap();
    compiled
        .validate_step_output(&step(), &json!("support"))
        .unwrap();
    for bad in [json!("unknown"), json!(1), json!({"label":"support"})] {
        assert!(compiled.validate_step_output(&step(), &bad).is_err());
    }
}

#[test]
fn workflow_registry_mcp_all_protocol_content_variants_and_bounds() {
    let example = example("mcp.call").unwrap();
    let compiled = compile_example(&example).unwrap();
    let valid = [
        json!({"type":"text","text":"","annotations":{"audience":["user"],"priority":0.5,"lastModified":"2026-01-01T00:00:00Z"},"_meta":{"vendor":{"hint":true}}}),
        json!({"type":"image","data":"aGk=","mimeType":"image/png"}),
        json!({"type":"audio","data":"aGk=","mimeType":"audio/wav"}),
        json!({"type":"resource","resource":{"uri":"file:///local/example","text":"","mimeType":"text/plain","_meta":{"vendor":1}}}),
        json!({"type":"resource","resource":{"uri":"resource://example","blob":"aGk="}}),
        json!({"type":"resource_link","uri":"https://never-fetched.invalid/private","name":"item","title":"Title","description":"Example","mimeType":"text/plain","size":10,"annotations":{},"_meta":{},"icons":[{"src":"data:image/png;base64,aGk=","mimeType":"image/png","sizes":["48x48","any"],"theme":"dark"}]}),
    ];
    for content in valid {
        compiled
            .validate_step_output(
                &step(),
                &json!({"content":[content],"isError":false,"_meta":{"vendor":true}}),
            )
            .unwrap();
    }
    let invalid = [
        json!({"type":"unknown","text":"x"}),
        json!({"type":"text"}),
        json!({"type":"image","data":"?","mimeType":"image/png"}),
        json!({"type":"audio","data":"aGk="}),
        json!({"type":"resource","resource":{"uri":"resource://example"}}),
        json!({"type":"resource_link","uri":"https://never-fetched.invalid"}),
        json!({"type":"resource_link","uri":"resource://example","name":"item","icons":[{"src":"https://never-fetched.invalid/icon","theme":"unknown"}]}),
        json!({"type":"text","text":"x","annotations":{"priority":2}}),
        json!({"type":"text","text":"x","_meta":[]}),
        json!({"type":"text","text":"x".repeat(MAX_TEXT + 1)}),
        json!({"type":"image","data":"AAAA".repeat(MAX_TEXT / 4 + 1),"mimeType":"image/png"}),
    ];
    for content in invalid {
        assert!(
            compiled
                .validate_step_output(&step(), &json!({"content":[content],"isError":false}))
                .is_err()
        );
    }
    assert!(compiled.validate_step_output(&step(), &json!({"content":vec![json!({"type":"text","text":""});MAX_ITEMS + 1],"isError":false})).is_err());
    assert!(
        compiled
            .validate_step_output(
                &step(),
                &json!({"content":[],"isError":false,"structuredContent":23})
            )
            .is_err()
    );
    assert!(
        compiled
            .validate_step_output(&step(), &json!({"content":[]}))
            .is_err()
    );
}

#[test]
fn workflow_registry_shared_actions_advertise_durable_suspension() {
    for name in [
        "http.request",
        "tool.call",
        "mcp.call",
        "message.send",
        "message.reply",
    ] {
        let compiled = compile_example(&example(name).unwrap()).unwrap();
        let constraints =
            &compiled.representation()["descriptors"]["start"]["registration"]["constraints"];
        assert_eq!(constraints["may_suspend"], true, "{name}");
        assert_eq!(
            constraints["recovery"],
            "logical_idempotency_receipt_reconciliation"
        );
    }
    for name in ["data.map", "decision.rule"] {
        let compiled = compile_example(&example(name).unwrap()).unwrap();
        assert_eq!(
            compiled.representation()["descriptors"]["start"]["registration"]["constraints"]["may_suspend"],
            false
        );
    }
}

fn repeat_steps(example: &mut AuthoringExample, count: usize) {
    changed(example, |v| {
        let template = v["steps"]["start"].clone();
        let mut steps = serde_json::Map::new();
        for i in 0..count {
            let id = if i == 0 {
                "start".to_owned()
            } else {
                format!("step{i}")
            };
            let target = if i + 1 == count {
                "$end".to_owned()
            } else {
                format!("step{}", i + 1)
            };
            let mut step = template.clone();
            step["routes"]["success"] = json!(target);
            steps.insert(id, step);
        }
        v["steps"] = Value::Object(steps);
        v["limits"]["max_steps"] = json!(count);
    });
}

#[test]
fn workflow_registry_expansion_rejects_reused_facts_before_specialization() {
    for name in [
        "agent.run",
        "tool.call",
        "mcp.call",
        "workflow.call",
        "flow.repeat",
    ] {
        let mut example = example(name).unwrap();
        match name {
            "agent.run" => {
                example.facts.profiles = (0..50)
                    .map(|i| CapabilityProfile {
                        name: TypeName::parse(format!("profile{i}")).unwrap(),
                        tools: (0..128)
                            .map(|j| {
                                TypeName::parse(format!("tool{j}{}", "x".repeat(100))).unwrap()
                            })
                            .collect(),
                        skills: vec![],
                    })
                    .collect();
            }
            "tool.call" | "mcp.call" => {
                example.facts.tools[0].output_schema["description"] = json!("x".repeat(60_000))
            }
            _ => example.facts.children[0].output_schema["description"] = json!("x".repeat(60_000)),
        }
        // If specialization runs, this deliberately invalid declaration/tool fact fails first.
        // The aggregate guard must reject before any native validator or registration runs.
        if name == "agent.run" {
            changed(&mut example, |v| {
                v["steps"]["start"]["with"]["output_schema"] = json!({"literal":{"type":"invalid"}})
            });
        } else if name.contains("call") && !name.starts_with("workflow") {
            example.facts.tools[0].output_schema["type"] = json!("invalid");
        } else {
            example.facts.children[0].output_schema["type"] = json!("invalid");
        }
        repeat_steps(&mut example, 32);
        assert_eq!(
            compile_example(&example).err().unwrap().code,
            "dependency.limit",
            "{name}"
        );
    }
}
