use super::test_support::*;
use super::*;
use crate::domain::workflow::{RunId, RunMetadata};
use serde_json::json;
use uuid::Uuid;

#[test]
fn workflow_compiler_checks_nested_schema_and_literal_types() {
    let source = single("{ref: /input/message}")
        .replace("required: [message]", "required: [user]")
        .replace(
            "properties: {message: {type: string}}",
            "properties: {user: {type: object, properties: {message: {type: string}}}}",
        )
        .replace("/input/message", "/input/user/message");
    let error = compile_one(&source).err().unwrap();
    assert_eq!(error.code, "reference.optional");
    let fixed = source.replace(
        "{ref: /input/user/message}",
        "{default: {ref: /input/user/message, value: {literal: fallback}}}",
    );
    assert!(compile_one(&fixed).is_ok());
    let invalid_type = single("{literal: 1}");
    assert!(matches!(
        compile_one(&invalid_type),
        Err(Diagnostic {
            code: "binding.type",
            ..
        })
    ));
    let invalid_schema = single("{literal: hello}").replace("{type: string}", "{type: fanciful}");
    assert!(matches!(
        compile_one(&invalid_schema),
        Err(Diagnostic {
            code: "schema.meta",
            ..
        })
    ));
    let mut nested_descriptor = descriptor();
    nested_descriptor.input_schema = json!({"type":"object","properties":{"message":{"type":"object","properties":{"inner":{"type":"string"}},"required":["inner"]}},"required":["message"]});
    let constructed = single("{object: {inner: {literal: 1}}}");
    assert!(matches!(
        compile_custom(&constructed, vec![nested_descriptor], empty_deps()),
        Err(Diagnostic {
            code: "binding.type",
            ..
        })
    ));
}

#[test]
fn workflow_compiler_rejects_impossible_descriptor_bindings() {
    let mut forbidden = descriptor();
    forbidden.input_schema = json!({
        "type": "object",
        "properties": {"message": false},
        "required": ["message"]
    });
    let error = compile_custom(
        &single("{ref: /input/message}"),
        vec![forbidden],
        empty_deps(),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "binding.property");
    assert_eq!(error.field_path.as_ref(), "/steps/start/with/message");

    let mut referenced_forbidden = descriptor();
    referenced_forbidden.input_schema = json!({
        "type": "object",
        "properties": {"message": {"$ref": "#/$defs/forbidden"}},
        "required": ["message"],
        "$defs": {"forbidden": false}
    });
    let error = compile_custom(
        &single("{ref: /input/message}"),
        vec![referenced_forbidden],
        empty_deps(),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "binding.property");

    let mut permissive = descriptor();
    permissive.input_schema = json!({
        "type": "object",
        "properties": {"message": true},
        "required": ["message"]
    });
    for binding in [
        "{concat: [{literal: 1}]}",
        "{and: [{literal: hello}]}",
        "{or: [{literal: null}]}",
        "{not: {literal: []}}",
        "{in: [{literal: item}, {literal: nope}]}",
        "{lt: [{literal: true}, {literal: 1}]}",
        "{ge: [{literal: 1}, {literal: hello}]}",
        "{object: {inner: {concat: [{literal: 1}]}}}",
        "{default: {ref: /input/missing, value: {not: {literal: 1}}}}",
    ] {
        let error = compile_custom(&single(binding), vec![permissive.clone()], empty_deps())
            .err()
            .unwrap();
        assert_eq!(error.code, "binding.operand", "binding: {binding}");
    }
    assert!(
        compile_custom(
            &single("{eq: [{literal: 1}, {literal: hello}]}"),
            vec![permissive.clone()],
            empty_deps(),
        )
        .is_ok()
    );
    assert!(
        compile_custom(
            &single("{lt: [{literal: 1}, {literal: 1.5}]}"),
            vec![permissive],
            empty_deps(),
        )
        .is_ok()
    );
}

#[test]
fn workflow_compiler_projects_local_refs_unions_and_array_bounds() {
    let base = single("{ref: /input/user/message}");
    let local = base.replace(
        "{type: object, properties: {message: {type: string}}, required: [message]}",
        "{type: object, properties: {user: {$ref: '#/$defs/user'}}, required: [user], $defs: {user: {type: object, properties: {message: {type: string}}, required: [message]}}}",
    );
    assert!(compile_one(&local).is_ok());
    let union = base.replace(
        "{type: object, properties: {message: {type: string}}, required: [message]}",
        "{oneOf: [{type: object, properties: {user: {type: object, properties: {message: {type: string}}, required: [message]}}, required: [user]}, {type: object, properties: {user: {type: object}}, required: [user]}]}",
    );
    assert!(matches!(
        compile_one(&union),
        Err(Diagnostic {
            code: "reference.optional",
            ..
        })
    ));
    let nullable = base.replace(
        "{type: object, properties: {message: {type: string}}, required: [message]}",
        "{type: object, properties: {user: {type: [object, 'null'], properties: {message: {type: string}}, required: [message]}}, required: [user]}",
    ).replace("{ref: /input/user/message}", "{default: {ref: /input/user/message, value: {literal: fallback}}}");
    assert!(matches!(
        compile_one(&nullable),
        Err(Diagnostic {
            code: "reference.traversal",
            ..
        })
    ));
    let array = single("{ref: /input/items/1}").replace(
        "{type: object, properties: {message: {type: string}}, required: [message]}",
        "{type: object, properties: {items: {type: array, minItems: 2, prefixItems: [{type: string}, {type: string}], items: false}}, required: [items]}",
    );
    assert!(compile_one(&array).is_ok());
    let impossible = array.replace("/input/items/1", "/input/items/2");
    assert!(matches!(
        compile_one(&impossible),
        Err(Diagnostic {
            code: "reference.impossible",
            ..
        })
    ));
}

#[test]
fn workflow_compiler_ordered_rules_are_checked() {
    let rule_descriptor = StepDescriptor {
        type_name: TypeName::parse("decision.rule").unwrap(),
        input_schema: json!({"type":"object"}),
        output_schema: json!(true),
        routes: RouteContract::Choices(BTreeSet::from([
            ChoiceName::parse("yes").unwrap(),
            ChoiceName::parse("no").unwrap(),
        ])),
        control: ControlKind::None,
        ordered_rule: true,
    };
    let source = format!(
        r#"format_version: 1
workflow_id: {ID}
input_schema: true
parameter_schema: true
output_schema: true
resources: []
entry: decide
steps:
  decide:
    type: decision.rule
    with: {{}}
    rule:
      cases:
        - when: {{literal: false}}
          choice: yes
        - when: {{literal: true}}
          choice: no
      default: yes
    routes: {{choices: {{yes: $end, no: $end}}}}
limits: {{max_steps: 20, max_context_bytes: 4096}}
"#
    );
    let compiled = compile_custom(&source, vec![rule_descriptor.clone()], empty_deps()).unwrap();
    let input = json!({});
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
    assert_eq!(
        compiled
            .rule(&StepId::parse("decide").unwrap())
            .unwrap()
            .decide(&context, compiled.graph().context_limits())
            .unwrap()
            .as_str(),
        "no"
    );
    let bad = source.replace("default: yes", "default: missing");
    assert!(matches!(
        compile_custom(&bad, vec![rule_descriptor], empty_deps()),
        Err(Diagnostic {
            code: "rule.invalid",
            ..
        })
    ));
}

#[test]
fn workflow_compiler_child_dependencies_and_repeat_bounds_are_checked() {
    let child = WorkflowId::new(Uuid::from_u128(2));
    let mut deps = BTreeMap::from([
        (
            WorkflowId::new(Uuid::parse_str(ID).unwrap()),
            BTreeSet::from([child]),
        ),
        (
            child,
            BTreeSet::from([WorkflowId::new(Uuid::parse_str(ID).unwrap())]),
        ),
    ]);
    let repeat = StepDescriptor {
        type_name: TypeName::parse("flow.repeat").unwrap(),
        input_schema: json!(true),
        output_schema: json!(true),
        routes: RouteContract::Success,
        control: ControlKind::Repeat,
        ordered_rule: false,
    };
    let child_source = single("{literal: hello}").replace("type: data.map", "type: flow.repeat\n    child_workflow_id: 00000000-0000-0000-0000-000000000002\n    child_version_id: 00000000-0000-0000-0000-000000000003\n    max_iterations: 2");
    assert!(matches!(
        compile_custom(&child_source, vec![repeat.clone()], deps.clone()),
        Err(Diagnostic {
            code: "dependency.cycle",
            ..
        })
    ));
    deps.get_mut(&child).unwrap().clear();
    assert!(compile_custom(&child_source, vec![repeat.clone()], deps.clone()).is_ok());
    let zero = child_source.replace("max_iterations: 2", "max_iterations: 0");
    assert!(compile_custom(&zero, vec![repeat.clone()], deps.clone()).is_err());
    let excessive = child_source.replace("max_iterations: 2", "max_iterations: 10001");
    assert!(compile_custom(&excessive, vec![repeat.clone()], deps.clone()).is_err());
    let missing = child_source.replace("    max_iterations: 2\n", "");
    assert!(matches!(
        compile_custom(&missing, vec![repeat], deps),
        Err(Diagnostic {
            code: "step.control",
            ..
        })
    ));
}

#[test]
fn workflow_compiler_checks_root_local_ref_properties_and_required_inputs() {
    let mut descriptor = descriptor();
    descriptor.input_schema = json!({
        "$ref": "#/$defs/args",
        "$defs": {"args": {
            "type": "object",
            "properties": {"message": {"type": "string"}},
            "required": ["message"],
            "additionalProperties": false
        }}
    });
    let bad = compile_custom(
        &single("{literal: 1}"),
        vec![descriptor.clone()],
        empty_deps(),
    )
    .err()
    .unwrap();
    assert_eq!(bad.code, "binding.type");
    let missing =
        single("{literal: ok}").replace("with:\n      message: {literal: ok}", "with: {}");
    let error = compile_custom(&missing, vec![descriptor.clone()], empty_deps())
        .err()
        .unwrap();
    assert_eq!(error.code, "binding.required");
    assert!(compile_custom(&single("{literal: ok}"), vec![descriptor], empty_deps()).is_ok());
}

#[test]
fn workflow_compiler_validates_known_nested_literals_with_local_refs() {
    let mut descriptor = descriptor();
    descriptor.input_schema = json!({
        "type": "object",
        "properties": {"message": {
            "type": "object",
            "properties": {"x": {"$ref": "#/$defs/text"}},
            "required": ["x"]
        }},
        "required": ["message"],
        "$defs": {"text": {"type": "string"}}
    });
    let error = compile_custom(
        &single("{literal: {x: 1}}"),
        vec![descriptor.clone()],
        empty_deps(),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "binding.type");
    assert!(
        compile_custom(
            &single("{literal: {x: ok}}"),
            vec![descriptor.clone()],
            empty_deps()
        )
        .is_ok()
    );
    descriptor.input_schema["properties"]["message"]["properties"]["x"] =
        json!({"$dynamicRef":"#/$defs/text"});
    assert!(
        compile_custom(
            &single("{literal: {x: ok}}"),
            vec![descriptor.clone()],
            empty_deps()
        )
        .is_ok()
    );
    let error = compile_custom(&single("{literal: {x: 1}}"), vec![descriptor], empty_deps())
        .err()
        .unwrap();
    assert_eq!(error.code, "binding.type");
}

#[test]
fn workflow_compiler_preserves_local_ref_sibling_constraints() {
    let mut descriptor = descriptor();
    descriptor.input_schema = json!({
        "type": "object",
        "properties": {"message": {"$ref": "#/$defs/anything", "type": "string"}},
        "required": ["message"],
        "$defs": {"anything": true}
    });
    let error = compile_custom(
        &single("{literal: 1}"),
        vec![descriptor.clone()],
        empty_deps(),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "binding.type");
    assert!(compile_custom(&single("{literal: ok}"), vec![descriptor], empty_deps()).is_ok());
}

#[test]
fn workflow_compiler_validates_literals_in_original_reference_scopes() {
    let cases = [
        (
            "plain reference",
            json!({"type":"object", "properties":{"message":{"$ref":"#/$defs/text"}},
                "required":["message"], "$defs":{"text":{"type":"string"}}}),
        ),
        (
            "dynamic reference",
            json!({"type":"object", "properties":{"message":{"$dynamicRef":"#/$defs/text"}},
                "required":["message"], "$defs":{"text":{"type":"string"}}}),
        ),
        (
            "root id and two references",
            json!({"$id":"https://example.test/args", "type":"object",
                "properties":{"message":{"$ref":"#/$defs/alias"}}, "required":["message"],
                "$defs":{"alias":{"$ref":"#/$defs/text"}, "text":{"type":"string"}}}),
        ),
        (
            "nested id overrides root definition",
            json!({"type":"object", "properties":{"message":{
                "$id":"nested", "$ref":"#/$defs/text",
                "$defs":{"text":{"type":"string"}}}}, "required":["message"],
                "$defs":{"text":{"type":"number"}}}),
        ),
        (
            "nested id overrides a false root definition",
            json!({"type":"object", "properties":{"message":{
                "$id":"nested", "$ref":"#/$defs/text",
                "$defs":{"text":{"type":"string"}}}}, "required":["message"],
                "$defs":{"text":false}}),
        ),
        (
            "relative id",
            json!({"$id":"relative", "type":"object",
                "properties":{"message":{"$ref":"#/$defs/text"}}, "required":["message"],
                "$defs":{"text":{"type":"string"}}}),
        ),
        (
            "selector base collision",
            json!({"$id":"urn:workflow:selection", "type":"object",
                "properties":{"message":{"$ref":"#/$defs/text"}}, "required":["message"],
                "$defs":{"text":{"type":"string"}}}),
        ),
    ];
    for (name, root_schema) in cases {
        let mut descriptor = descriptor();
        descriptor.input_schema = root_schema;
        assert!(
            compile_custom(
                &single("{literal: ok}"),
                vec![descriptor.clone()],
                empty_deps()
            )
            .is_ok(),
            "{name}: valid literal"
        );
        let error = compile_custom(&single("{literal: 1}"), vec![descriptor], empty_deps())
            .err()
            .unwrap();
        assert_eq!(error.code, "binding.type", "{name}: invalid literal");
    }
}

#[test]
fn workflow_compiler_selects_escaped_and_unicode_destination_names() {
    let name = "a/b~%# ☃";
    let mut descriptor = descriptor();
    descriptor.input_schema = json!({"type":"object", "properties":{name:{"type":"string"}},
        "required":[name]});
    let source =
        single("{literal: ok}").replace("message: {literal: ok}", "\"a/b~%# ☃\": {literal: ok}");
    assert!(compile_custom(&source, vec![descriptor.clone()], empty_deps()).is_ok());
    let invalid = source.replace("{literal: ok}", "{literal: 1}");
    let error = compile_custom(&invalid, vec![descriptor], empty_deps())
        .err()
        .unwrap();
    assert_eq!(error.code, "binding.type");
}

#[test]
fn workflow_compiler_distinguishes_equal_fragments_under_different_ids() {
    let mut descriptor = descriptor();
    descriptor.input_schema = json!({
        "type":"object", "properties":{"message":{
            "type":"object", "properties":{
                "first":{"$id":"first", "type":"object",
                    "properties":{"leaf":{"$ref":"#/$defs/value"}},
                    "required":["leaf"], "$defs":{"value":{"type":"string"}}},
                "second":{"$id":"second", "type":"object",
                    "properties":{"leaf":{"$ref":"#/$defs/value"}},
                    "required":["leaf"], "$defs":{"value":{"type":"number"}}}
            }, "required":["first", "second"]
        }}, "required":["message"], "$defs":{"value":true}
    });
    let valid = single(
        "{object: {first: {object: {leaf: {literal: ok}}}, second: {object: {leaf: {literal: 1}}}}}",
    );
    assert!(compile_custom(&valid, vec![descriptor.clone()], empty_deps()).is_ok());
    for invalid in [
        valid.replace("leaf: {literal: ok}", "leaf: {literal: 1}"),
        valid.replace("leaf: {literal: 1}", "leaf: {literal: ok}"),
    ] {
        let error = compile_custom(&invalid, vec![descriptor.clone()], empty_deps())
            .err()
            .unwrap();
        assert_eq!(error.code, "binding.type");
    }
}
