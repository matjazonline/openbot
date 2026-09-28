use super::test_support::*;
use super::*;
use crate::domain::workflow::{RunId, RunMetadata};
use serde_json::json;
use uuid::Uuid;

#[test]
fn workflow_compiler_round_trip_identity_and_runtime_snapshot() {
    let source = single("{ref: /input/message}");
    let compiled = compile_one(&source).unwrap();
    assert_eq!(compiled.source(), source);
    assert_eq!(
        compiled.content_hash(),
        compile_one(&source).unwrap().content_hash()
    );
    assert_ne!(
        compiled.content_hash(),
        compile_one(&format!("{source}\n")).unwrap().content_hash()
    );
    assert_eq!(
        compiled.representation()["definition"]["steps"]["start"]["with"]["message"]["ref"],
        "/input/message"
    );
    let mut input = json!({"message":"hello"});
    let params = json!({});
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
    let prepared = compiled
        .prepare_step_inputs(&StepId::parse("start").unwrap(), &context)
        .unwrap();
    input = json!({"message":"changed"});
    assert_eq!(prepared.value(), &json!({"message":"hello"}));
    assert_eq!(input["message"], "changed");
    assert!(
        compiled
            .validate_step_output(&StepId::parse("start").unwrap(), &json!({"result":"ok"}))
            .is_ok()
    );
    let error = compiled
        .validate_step_output(&StepId::parse("start").unwrap(), &json!({"result":12}))
        .unwrap_err();
    assert_eq!(error.instance_path.as_deref(), Some("/result"));
    assert!(error.schema_path.is_some());
    assert!(compiled.validate_input(&json!({"message":12})).is_err());
    assert!(compiled.validate_params(&json!({})).is_ok());
}

#[test]
fn workflow_compiler_rejects_malformed_language_and_includes_descriptor_facts_in_hash() {
    let source = single("{ref: /input/message}");
    for (bad, code) in [
        (
            source.replace("format_version: 1", "format_version: 2"),
            "format.version",
        ),
        (
            source.replace("type: data.map", "type: unknown.step"),
            "step.type",
        ),
        (
            source.replace(
                "routes: {success: $end}",
                "routes: {success: $end, choices: {yes: $end}}",
            ),
            "route.shape",
        ),
        (
            source.replace("resources: []", "resources: []\nunknown: yes"),
            "syntax.unknown_field",
        ),
        (
            source.replace("{ref: /input/message}", "{ref: /steps/start/output/result}"),
            "reference.path",
        ),
        (
            source.replace(
                "{ref: /input/message}",
                "{default: {ref: /steps/unknown/output/result, value: {literal: ok}}}",
            ),
            "reference.step",
        ),
    ] {
        let error = compile_one(&bad).err().unwrap();
        assert_eq!(error.code, code, "{bad}");
    }
    let original = compile_one(&source).unwrap();
    let mut changed = descriptor();
    changed.output_schema = json!({"type":"object","properties":{"result":{"type":"string","maxLength":10}},"required":["result"]});
    let updated = compile_custom(&source, vec![changed], empty_deps()).unwrap();
    assert_ne!(original.content_hash(), updated.content_hash());
}

#[test]
fn workflow_compiler_parses_every_binding_operator() {
    let mut permissive = descriptor();
    permissive.input_schema =
        json!({"type":"object","properties":{"message":true},"required":["message"]});
    for binding in [
        "{literal: {ref: /looks/like/an/operator}}",
        "{ref: /input/message}",
        "{object: {key: {literal: 1}}}",
        "{array: [{literal: null}, {literal: true}]}",
        "{concat: [{literal: a}, {literal: b}]}",
        "{default: {ref: /input/optional, value: {literal: fallback}}}",
        "{eq: [{literal: 1}, {literal: 1.0}]}",
        "{ne: [{literal: 1}, {literal: 2}]}",
        "{lt: [{literal: 1}, {literal: 2}]}",
        "{le: [{literal: 1}, {literal: 2}]}",
        "{gt: [{literal: 2}, {literal: 1}]}",
        "{ge: [{literal: 2}, {literal: 1}]}",
        "{in: [{literal: 1}, {array: [{literal: 1}]}]}",
        "{exists: /input/optional}",
        "{and: [{literal: true}, {literal: false}]}",
        "{or: [{literal: true}, {literal: false}]}",
        "{not: {literal: false}}",
    ] {
        let source = single(binding);
        assert!(
            compile_custom(&source, vec![permissive.clone()], empty_deps()).is_ok(),
            "{binding}"
        );
    }
}

#[test]
fn workflow_compiler_reports_exact_reference_and_route_locations() {
    let source = single("{ref: /input/optional}");
    let error = compile_one(&source).err().unwrap();
    assert_eq!(error.code, "reference.optional");
    assert_eq!(error.field_path.as_ref(), "/steps/start/with/message/ref");
    assert_eq!(&source[error.span.start..error.span.end], "/input/optional");

    let source = single("{ref: /input/message}").replace("success: $end", "success: unknown");
    let error = compile_one(&source).err().unwrap();
    assert_eq!(error.code, "graph.invalid");
    assert_eq!(&source[error.span.start..error.span.end], "unknown");
}

#[test]
fn workflow_compiler_rejects_error_edge_and_diamond_output_gaps() {
    let base = format!(
        r#"format_version: 1
workflow_id: {ID}
input_schema: true
parameter_schema: true
output_schema: true
resources: []
entry: start
steps:
  start:
    type: data.map
    with: {{message: {{literal: hello}}}}
    routes: {{success: merge, error: merge}}
  merge:
    type: data.map
    with: {{message: {{ref: /steps/start/output/result}}}}
    routes: {{success: $end}}
limits: {{max_steps: 20, max_context_bytes: 4096}}
"#
    );
    let error = compile_one(&base).err().unwrap();
    assert_eq!(error.code, "reference.path");
    assert_eq!(
        &base[error.span.start..error.span.end],
        "/steps/start/output/result"
    );
    let fixed = base.replace(
        "{success: merge, error: merge}",
        "{success: merge, error: $end}",
    );
    assert!(compile_one(&fixed).is_ok());

    let branch = StepDescriptor {
        type_name: TypeName::parse("decision.branch").unwrap(),
        input_schema: json!({"type":"object"}),
        output_schema: json!(true),
        routes: RouteContract::Choices(BTreeSet::from([
            ChoiceName::parse("left").unwrap(),
            ChoiceName::parse("right").unwrap(),
        ])),
        control: ControlKind::None,
        ordered_rule: false,
    };
    let diamond = format!(
        r#"format_version: 1
workflow_id: {ID}
input_schema: true
parameter_schema: true
output_schema: true
resources: []
entry: start
steps:
  start:
    type: decision.branch
    with: {{}}
    routes: {{choices: {{left: left, right: right}}}}
  left:
    type: data.map
    with: {{message: {{literal: hi}}}}
    routes: {{success: merge}}
  right:
    type: data.map
    with: {{message: {{literal: hi}}}}
    routes: {{success: merge}}
  merge:
    type: data.map
    with: {{message: {{ref: /steps/left/output/result}}}}
    routes: {{success: $end}}
limits: {{max_steps: 20, max_context_bytes: 4096}}
"#
    );
    let error = compile_custom(&diamond, vec![branch, descriptor()], empty_deps())
        .err()
        .unwrap();
    assert_eq!(error.code, "reference.path");
}
