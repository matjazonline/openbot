use super::tests::*;
use super::*;

fn private_export_compiled(call: &Value, catalog: &Value) -> CompiledWorkflow {
    let mut sample = example("mcp.call").unwrap();
    let tool = catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == call["tool"])
        .unwrap();
    sample.facts.tools[0].name = TypeName::parse(call["tool"].as_str().unwrap()).unwrap();
    sample.facts.tools[0].input_schema = tool["inputSchema"].clone();
    sample.facts.tools[0].output_schema = tool.get("outputSchema").cloned().unwrap_or(json!(true));
    changed(&mut sample, |source| {
        source["steps"]["start"]["with"]["tool"] = json!({"literal":call["tool"]});
        source["steps"]["start"]["with"]["arguments"] = json!({"literal":call["arguments"]});
    });
    compile_example(&sample).unwrap()
}

#[test]
fn doop_private_export_actual_results_fit_registry_and_inline_overflow_still_fails() {
    let provenance: Value = serde_json::from_str(include_str!(
        "../../../adapters/mcp/fixtures/doop/private-exports-provenance.json"
    ))
    .unwrap();
    let catalog: Value = serde_json::from_str(include_str!(
        "../../../adapters/mcp/fixtures/doop/private-exports-tools.json"
    ))
    .unwrap();
    for (name, source) in [
        (
            "screenshot",
            include_str!("../../../adapters/mcp/fixtures/doop/private-screenshot.json"),
        ),
        (
            "png",
            include_str!("../../../adapters/mcp/fixtures/doop/private-png.json"),
        ),
        (
            "jpg",
            include_str!("../../../adapters/mcp/fixtures/doop/private-jpg.json"),
        ),
        (
            "pdf",
            include_str!("../../../adapters/mcp/fixtures/doop/private-pdf.json"),
        ),
        (
            "pdf-small",
            include_str!("../../../adapters/mcp/fixtures/doop/private-pdf-small.json"),
        ),
    ] {
        let compiled = private_export_compiled(&provenance["calls"][name], &catalog);
        let mut output: Value = serde_json::from_str(source).unwrap();
        // Production McpCallResult normalizes the optional status at ingress;
        // the guarded transport test checks these same captured results.
        output["isError"] = json!(false);
        compiled.validate_step_output(&step(), &output).unwrap();
        let content = output["content"].as_array_mut().unwrap();
        content.push(
            json!({"type":"image","data":"AAAA".repeat(MAX_TEXT / 4),"mimeType":"image/png"}),
        );
        compiled.validate_step_output(&step(), &output).unwrap();
        let last = output["content"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap();
        last["data"] = json!("AAAA".repeat(MAX_TEXT / 4 + 1));
        assert!(
            compiled.validate_step_output(&step(), &output).is_err(),
            "{name}"
        );
    }
}
fn prepare(compiled: &CompiledWorkflow, input: &Value) -> Result<Value, Diagnostic> {
    let params = json!({});
    let run = json!({});
    let outputs = BTreeMap::new();
    compiled
        .prepare_step_inputs(&step(), &context(input, &params, &outputs, &run))
        .map(|p| p.value().clone())
}
#[test]
fn workflow_registry_dynamic_http_and_deadline_checks_are_authoritative_and_redacted() {
    for (name, field, bad, good, code) in [
        (
            "http.request",
            "path",
            json!("https://secret.invalid/token"),
            json!("/items"),
            "registry.http_path",
        ),
        (
            "http.request",
            "headers",
            json!({"AUTHORIZATION":"secret-token"}),
            json!({"Accept":"application/json"}),
            "registry.http_header",
        ),
        (
            "wait.event",
            "deadline",
            json!("secret-invalid"),
            json!("2030-01-01T00:00:00Z"),
            "registry.deadline",
        ),
        (
            "decision.human",
            "deadline",
            json!("tomorrow"),
            json!("2020-01-01T00:00:00+00:00"),
            "registry.deadline",
        ),
    ] {
        let mut example = example(name).unwrap();
        changed(&mut example, |v| {
            v["input_schema"] =
                json!({"type":"object","properties":{"value":true},"required":["value"]});
            v["steps"]["start"]["with"][field] = json!({"ref":"/input/value"});
        });
        let compiled = compile_example(&example).unwrap();
        prepare(&compiled, &json!({"value":good})).unwrap();
        let error = prepare(&compiled, &json!({"value":bad})).unwrap_err();
        assert_eq!(error.code, code);
        assert!(!error.message.contains("secret"));
        assert!(
            error
                .field_path
                .starts_with(&format!("/steps/start/with/{field}"))
        );
        assert!(error.message.len() <= 512);
    }
}
#[test]
fn workflow_registry_each_type_validates_dynamic_required_inputs() {
    for name in TYPES {
        let mut example = example(name).unwrap();
        let (field, bad) = match name {
            "context.history" => ("max_tokens", json!(0)),
            // The scope is a literal (E4); the dynamic inputs are the query and the facts.
            "memory.load" => ("query", json!("")),
            "memory.save" => ("facts", json!([])),
            "jev.evaluate" => ("state", json!(true)),
            "ai.classify" => continue, // Context is intentionally schema-free JSON; declaration must be static.
            "agent.run" | "decision.agent" => ("agent", json!(3)),
            "decision.rule" => ("data", json!(3)),
            "decision.human" => ("deadline", json!("bad")),
            "data.map" | "channel.no_reply" => ("value", json!(3)),
            "data.url" => ("segments", json!(vec!["x"; 17])),
            "http.request" => ("method", json!("BOGUS")),
            "local_agent.run" => ("profile_id", json!(3)),
            "tool.call" | "mcp.call" => ("arguments", json!({"key":3})),
            "message.send" | "message.reply" | "message.ask" => ("content", json!({"body":true})),
            "workflow.call" | "flow.repeat" => ("input", json!({"text":3})),
            _ => ("deadline", json!("bad")),
        };
        changed(&mut example, |v| {
            v["input_schema"] =
                json!({"type":"object","properties":{"value":true},"required":["value"]});
            v["steps"]["start"]["with"][field] = json!({"ref":"/input/value"});
        });
        let compiled = compile_example(&example).unwrap();
        assert!(prepare(&compiled, &json!({"value":bad})).is_err(), "{name}");
    }
}
#[test]
fn workflow_registry_mcp_output_and_downstream_structured_reference() {
    let mut example = example("mcp.call").unwrap();
    changed(&mut example, |v| {
        v["steps"]["start"]["routes"]["success"] = json!("map");
        v["steps"]["map"] = json!({"type":"data.map","with":{"value":{"default":{"ref":"/steps/start/output/structuredContent/result","value":{"literal":"none"}}},"output_schema":{"literal":{"type":"string"}}},"routes":{"success":"$end"}});
    });
    let compiled = compile_example(&example).unwrap();
    let output = json!({"content":[{"type":"text","text":"ok"}],"structuredContent":{"result":"found"},"isError":false});
    compiled.validate_step_output(&step(), &output).unwrap();
    for invalid in [
        json!({"content":[]}),
        json!({"content":[],"structuredContent":{"result":3},"isError":false}),
        json!({"content":[],"isError":false,"extra":true}),
    ] {
        assert!(compiled.validate_step_output(&step(), &invalid).is_err());
    }
    let params = json!({});
    let input = json!({});
    let run = json!({});
    let outputs = BTreeMap::from([(step(), output)]);
    let prepared = compiled
        .prepare_step_inputs(
            &StepId::parse("map").unwrap(),
            &context(&input, &params, &outputs, &run),
        )
        .unwrap();
    assert_eq!(prepared.value()["value"], "found");
}
#[test]
fn workflow_registry_facts_bounded_unique_and_identity_sensitive() {
    for name in ["tool.call", "workflow.call"] {
        let mut example = example(name).unwrap();
        let initial = compile_example(&example).unwrap().content_hash().to_owned();
        if name == "tool.call" {
            example.facts.tools[0].output_schema["description"] = json!("new result contract");
        } else {
            example.facts.children[0].output_schema["description"] = json!("new child contract");
        }
        assert_ne!(initial, compile_example(&example).unwrap().content_hash());
        if name == "tool.call" {
            example.facts.tools[0].input_schema = json!({"$ref":"https://secret.invalid"});
        } else {
            example.facts.children[0].workflow_id = WorkflowId::new(uuid::Uuid::nil());
        }
        assert!(compile_example(&example).is_err());
    }
    let mut example = example("tool.call").unwrap();
    let tool = example.facts.tools[0].clone();
    for _ in 0..257 {
        example.facts.tools.push(tool.clone());
    }
    assert_eq!(
        compile_example(&example).err().unwrap().code,
        "registry.limit"
    );
    example.facts.tools.truncate(2);
    assert_eq!(
        compile_example(&example).err().unwrap().code,
        "registry.facts"
    );
}
#[test]
fn workflow_registry_human_feedback_and_repeat_output_constraints() {
    let human = compile_example(&example("decision.human").unwrap()).unwrap();
    human
        .validate_step_output(
            &step(),
            &json!({"choice":"continue","data":"ok","feedback":""}),
        )
        .unwrap();
    assert!(
        human
            .validate_step_output(
                &step(),
                &json!({"choice":"revise","data":"ok","feedback":""})
            )
            .is_err()
    );
    let repeat = compile_example(&example("flow.repeat").unwrap()).unwrap();
    repeat
        .validate_step_output(&step(), &json!({"rounds":3,"result":"done"}))
        .unwrap();
    for rounds in [0, 4] {
        assert!(
            repeat
                .validate_step_output(&step(), &json!({"rounds":rounds,"result":"done"}))
                .is_err()
        );
    }
}

/// A child's parameters come only from literals or the parent's binding parameters, a missing
/// `with.params` must satisfy the child, and every child resource slot must exist on the parent
/// with the same kind and contract (07-D1 §D1.1).
#[test]
fn workflow_registry_child_params_are_static_and_resources_match_by_slot() {
    use crate::domain::workflow::ResourceRequirement;
    let code = |example: &AuthoringExample| compile_example(example).err().map(|error| error.code);
    let call = |edit: &dyn Fn(&mut Value), child: &dyn Fn(&mut ChildContract)| {
        let mut example = example("workflow.call").unwrap();
        changed(&mut example, |source| {
            source["parameter_schema"] = json!({"type":"object","properties":{"reviewer":{"type":"string"}},"required":["reviewer"],"additionalProperties":false});
            edit(source)
        });
        child(&mut example.facts.children[0]);
        example
    };
    let requires_reviewer = |child: &mut ChildContract| {
        child.parameter_schema = json!({"type":"object","properties":{"reviewer":{"type":"string"}},"required":["reviewer"],"additionalProperties":false});
    };
    let params = |binding: Value| {
        move |source: &mut Value| source["steps"]["start"]["with"]["params"] = binding.clone()
    };
    // Literal and binding-parameter sources are accepted.
    assert_eq!(
        code(&call(
            &params(json!({"literal":{"reviewer":"r1"}})),
            &requires_reviewer
        )),
        None
    );
    assert_eq!(
        code(&call(
            &params(json!({"object":{"reviewer":{"ref":"/params/reviewer"}}})),
            &requires_reviewer
        )),
        None
    );
    // Run input, step output or run context never choose a child's parameters.
    for dynamic in [
        json!({"ref":"/input"}),
        json!({"object":{"reviewer":{"ref":"/run/id"}}}),
    ] {
        assert_eq!(
            code(&call(&params(dynamic), &requires_reviewer)),
            Some("registry.static_params")
        );
    }
    // Literal parameters are checked against the child's parameter schema.
    assert!(
        code(&call(
            &params(json!({"literal":{"reviewer":5}})),
            &requires_reviewer
        ))
        .is_some()
    );
    // Omitted parameters run the child with `{}`, which this child refuses.
    assert_eq!(
        code(&call(&|_| {}, &requires_reviewer)),
        Some("registry.child_params")
    );
    assert_eq!(code(&call(&|_| {}, &|_| {})), None);
    // Resources pass by slot name only: the parent must declare the same slot, kind and contract.
    let slot = |kind: &str| ResourceRequirement {
        slot: ResourceName::parse("knowledge").unwrap(),
        kind: TypeName::parse(kind).unwrap(),
        contract: None,
    };
    let needs_http = |child: &mut ChildContract| child.resources = vec![slot("http")];
    let declares = |kind: &'static str| {
        move |source: &mut Value| source["resources"] = json!([{"slot":"knowledge","kind":kind}])
    };
    assert_eq!(code(&call(&declares("http"), &needs_http)), None);
    assert_eq!(
        code(&call(&|_| {}, &needs_http)),
        Some("registry.child_resources")
    );
    assert_eq!(
        code(&call(&declares("mcp"), &needs_http)),
        Some("registry.child_resources")
    );
}

/// E4: a memory scope is a literal, so publication can resolve its agent; a reference is refused.
#[test]
fn memory_scope_step_scope_must_be_a_literal() {
    for name in ["memory.load", "memory.save"] {
        let mut example = example(name).unwrap();
        changed(&mut example, |v| {
            v["input_schema"] =
                json!({"type":"object","properties":{"value":true},"required":["value"]});
            v["steps"]["start"]["with"]["scope"] = json!({"ref":"/input/value"});
        });
        let Err(error) = compile_example(&example) else {
            panic!("{name}: a referenced scope compiled");
        };
        assert_eq!(error.code, "registry.literal", "{name}");
        assert_eq!(&*error.field_path, "/steps/start/with/scope");
    }
}
