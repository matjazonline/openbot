use super::tests::*;
use super::*;
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
            "context.load" => ("max_tokens", json!(0)),
            "memory.load" | "memory.save" => ("scope", json!({"kind":"user"})),
            "ai.classify" => continue, // Context is intentionally schema-free JSON; declaration must be static.
            "agent.run" | "decision.agent" => ("agent", json!(3)),
            "decision.rule" => ("data", json!(3)),
            "decision.human" => ("reviewer", json!({"kind":"alien"})),
            "data.map" => ("value", json!(3)),
            "http.request" => ("method", json!("BOGUS")),
            "tool.call" | "mcp.call" => ("arguments", json!({"key":3})),
            "message.send" | "message.reply" => ("content", json!({"body":true})),
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
    let mut example = example("agent.run").unwrap();
    for _ in 0..257 {
        example.facts.profiles.push(CapabilityProfile {
            name: TypeName::parse("same").unwrap(),
            tools: vec![],
            skills: vec![],
        });
    }
    assert_eq!(
        compile_example(&example).err().unwrap().code,
        "registry.limit"
    );
    example.facts.profiles.truncate(2);
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
