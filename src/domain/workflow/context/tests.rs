use super::*;
use serde_json::json;
use uuid::Uuid;
fn id(s: &str) -> StepId {
    StepId::parse(s).unwrap()
}
fn context<'a>(
    input: &'a Value,
    params: &'a Value,
    outputs: &'a BTreeMap<StepId, Value>,
) -> Context<'a> {
    Context {
        input,
        params,
        step_outputs: outputs,
        run: RunMetadata {
            run_id: RunId::new(Uuid::nil()),
            parent_run_id: None,
        },
    }
}
fn reference(path: &str) -> Binding {
    Binding::Reference(ContextReference::parse(path).unwrap())
}

#[test]
fn typed_operators_preserve_numbers_null_and_missing() {
    let input = json!({"present": null, "values": [1, 2]});
    let params = json!({});
    let outputs = BTreeMap::new();
    let context = context(&input, &params, &outputs);
    let limits = ContextLimits::default();
    let literal = |value| Binding::Literal(value);
    let greater = Binding::Compare {
        op: Comparison::Gt,
        left: Box::new(literal(json!(9007199254740993_u64))),
        right: Box::new(literal(json!(9007199254740992.0))),
    };
    assert_eq!(resolve(&greater, &context, limits).unwrap(), json!(true));
    let equal = Binding::Compare {
        op: Comparison::Eq,
        left: Box::new(literal(json!(1))),
        right: Box::new(literal(json!(1.0))),
    };
    assert_eq!(resolve(&equal, &context, limits).unwrap(), json!(true));
    assert_eq!(
        resolve(
            &Binding::In {
                value: Box::new(literal(json!(1.0))),
                items: Box::new(reference("/input/values"))
            },
            &context,
            limits
        )
        .unwrap(),
        json!(true)
    );
    assert_eq!(
        resolve(
            &Binding::Concat(vec![literal(json!("a")), literal(json!("b"))]),
            &context,
            limits
        )
        .unwrap(),
        json!("ab")
    );
    assert_eq!(
        resolve(
            &Binding::Exists(ContextReference::parse("/input/present").unwrap()),
            &context,
            limits
        )
        .unwrap(),
        json!(true)
    );
    assert_eq!(
        resolve(
            &Binding::Exists(ContextReference::parse("/input/absent").unwrap()),
            &context,
            limits
        )
        .unwrap(),
        json!(false)
    );
    let default = Binding::Default {
        reference: ContextReference::parse("/input/present").unwrap(),
        fallback: Box::new(literal(json!("fallback"))),
    };
    assert_eq!(resolve(&default, &context, limits).unwrap(), Value::Null);
    let missing = Binding::Default {
        reference: ContextReference::parse("/input/absent").unwrap(),
        fallback: Box::new(literal(json!("fallback"))),
    };
    assert_eq!(
        resolve(&missing, &context, limits).unwrap(),
        json!("fallback")
    );
    let malformed = Binding::Default {
        reference: ContextReference::parse("/input/present/x").unwrap(),
        fallback: Box::new(literal(json!(0))),
    };
    assert!(matches!(
        resolve(&malformed, &context, limits),
        Err(ContextError::InvalidReference(_))
    ));
}

#[test]
fn boolean_short_circuit_and_operand_errors() {
    let input = json!({});
    let params = json!({});
    let outputs = BTreeMap::new();
    let context = context(&input, &params, &outputs);
    let limits = ContextLimits::default();
    let missing = reference("/input/missing");
    assert_eq!(
        resolve(
            &Binding::And(vec![Binding::Literal(json!(false)), missing.clone()]),
            &context,
            limits
        )
        .unwrap(),
        json!(false)
    );
    assert_eq!(
        resolve(
            &Binding::Or(vec![Binding::Literal(json!(true)), missing]),
            &context,
            limits
        )
        .unwrap(),
        json!(true)
    );
    assert_eq!(
        resolve(
            &Binding::Not(Box::new(Binding::Literal(json!(false)))),
            &context,
            limits
        )
        .unwrap(),
        json!(true)
    );
    assert!(matches!(
        resolve(
            &Binding::Concat(vec![Binding::Literal(json!(1))]),
            &context,
            limits
        ),
        Err(ContextError::Operand(_))
    ));
    assert!(matches!(
        resolve(
            &Binding::Compare {
                op: Comparison::Lt,
                left: Box::new(Binding::Literal(json!(1))),
                right: Box::new(Binding::Literal(json!("1")))
            },
            &context,
            limits
        ),
        Err(ContextError::Operand(_))
    ));
    assert!(matches!(
        resolve(
            &Binding::And(vec![Binding::Literal(json!(1))]),
            &context,
            limits
        ),
        Err(ContextError::Operand(_))
    ));
    let skipped_large_literal = Binding::And(vec![
        Binding::Literal(json!(false)),
        Binding::Literal(Value::Array(vec![Value::Null; MAX_BINDING_NODES])),
    ]);
    assert!(matches!(
        resolve(&skipped_large_literal, &context, limits),
        Err(ContextError::Limit("work nodes"))
    ));
}

#[test]
fn typed_values_defaults_and_immutability() {
    let input = json!({"null":null,"number":3,"escaped/key":true});
    let params = json!(["a", "b"]);
    let outputs = BTreeMap::from([(id("first"), json!({"done":[1,2]}))]);
    let original = outputs.clone();
    let ctx = context(&input, &params, &outputs);
    let binding = Binding::Object(BTreeMap::from([
        (
            "null".into(),
            Binding::Default {
                reference: ContextReference::parse("/input/null").unwrap(),
                fallback: Box::new(Binding::Literal(json!("wrong"))),
            },
        ),
        (
            "missing".into(),
            Binding::Default {
                reference: ContextReference::parse("/input/absent").unwrap(),
                fallback: Box::new(reference("/params/1")),
            },
        ),
        ("value".into(), reference("/steps/first/output/done/0")),
        ("escaped".into(), reference("/input/escaped~1key")),
    ]));
    assert_eq!(
        resolve(&binding, &ctx, ContextLimits::default()).unwrap(),
        json!({"null":null,"missing":"b","value":1,"escaped":true})
    );
    assert_eq!(outputs, original);
}

#[test]
fn malformed_paths_do_not_default() {
    let input = json!([1]);
    let params = json!({});
    let outputs = BTreeMap::new();
    let ctx = context(&input, &params, &outputs);
    for path in [
        "input/0",
        "/unknown/x",
        "/input/~2",
        "/steps/x/other",
        "/steps/1bad/output",
        "/run/private",
    ] {
        assert!(
            matches!(
                ContextReference::parse(path),
                Err(ContextError::InvalidReference(_))
            ),
            "{path}"
        );
    }
    for path in [
        "/input/00",
        "/input/9999999999999999999999999999999999999999",
    ] {
        let fallback = Binding::Default {
            reference: ContextReference::parse(path).unwrap(),
            fallback: Box::new(Binding::Literal(json!(1))),
        };
        assert!(
            matches!(
                resolve(&fallback, &ctx, ContextLimits::default()),
                Err(ContextError::InvalidReference(_))
            ),
            "{path}"
        );
    }
}

#[test]
fn scalar_traversal_is_invalid_and_null_is_a_value() {
    let input = json!({"number": 4, "boolean": true, "string": "text", "null": null});
    let params = json!({});
    let outputs = BTreeMap::new();
    let ctx = context(&input, &params, &outputs);
    assert_eq!(
        resolve(&reference("/input/null"), &ctx, ContextLimits::default()).unwrap(),
        Value::Null
    );
    for field in ["number", "boolean", "string", "null"] {
        let path = format!("/input/{field}/child");
        let binding = Binding::Default {
            reference: ContextReference::parse(&path).unwrap(),
            fallback: Box::new(Binding::Literal(json!("fallback"))),
        };
        assert!(
            matches!(
                resolve(&binding, &ctx, ContextLimits::default()),
                Err(ContextError::InvalidReference(_))
            ),
            "{field}"
        );
    }
}

#[test]
fn wide_inputs_are_rejected_before_pending_stack_growth() {
    let input = Value::Array(vec![Value::Null; MAX_WORK_NODES + 1]);
    let params = json!({});
    let outputs = BTreeMap::new();
    let ctx = context(&input, &params, &outputs);
    assert!(matches!(
        resolve(&reference("/input"), &ctx, ContextLimits::default()),
        Err(ContextError::Limit("work nodes"))
    ));
    let input = Value::Object(
        (0..MAX_WORK_NODES + 1)
            .map(|n| (format!("k{n}"), Value::Null))
            .collect(),
    );
    let ctx = context(&input, &params, &outputs);
    assert!(matches!(
        resolve(&reference("/input"), &ctx, ContextLimits::default()),
        Err(ContextError::Limit("work nodes"))
    ));
    let wide_array = Binding::Array(vec![Binding::Literal(Value::Null); MAX_BINDING_NODES]);
    assert!(matches!(
        resolve(&wide_array, &ctx, ContextLimits::default()),
        Err(ContextError::Limit("binding nodes"))
    ));
    let wide_object = Binding::Object(
        (0..MAX_BINDING_NODES)
            .map(|n| (format!("k{n}"), Binding::Literal(Value::Null)))
            .collect(),
    );
    assert!(matches!(
        resolve(&wide_object, &ctx, ContextLimits::default()),
        Err(ContextError::Limit("binding nodes"))
    ));
}

#[test]
fn depth_work_and_output_budgets() {
    let mut deep = json!(0);
    for _ in 0..65 {
        deep = Value::Array(vec![deep]);
    }
    let params = json!({});
    let outputs = BTreeMap::new();
    let ctx = context(&deep, &params, &outputs);
    assert!(matches!(
        resolve(&reference("/input"), &ctx, ContextLimits::default()),
        Err(ContextError::Limit("JSON depth"))
    ));
    let input = json!({"value":[1,2,3]});
    let ctx = context(&input, &params, &outputs);
    let many = Binding::Array((0..100).map(|_| reference("/input/value")).collect());
    assert!(matches!(
        resolve(
            &many,
            &ctx,
            ContextLimits {
                output_bytes: MAX_OUTPUT_BYTES,
                work_nodes: 100
            }
        ),
        Err(ContextError::Limit("work nodes"))
    ));
    assert!(matches!(
        resolve(
            &many,
            &ctx,
            ContextLimits {
                output_bytes: 10,
                work_nodes: MAX_WORK_NODES
            }
        ),
        Err(ContextError::Limit("output bytes"))
    ));
}

#[test]
fn exact_binding_and_pointer_limits() {
    let input = json!(0);
    let params = json!({});
    let outputs = BTreeMap::new();
    let ctx = context(&input, &params, &outputs);
    let mut nested = Binding::Literal(json!(1));
    for _ in 0..MAX_BINDING_DEPTH {
        nested = Binding::Array(vec![nested]);
    }
    assert!(resolve(&nested, &ctx, ContextLimits::default()).is_ok());
    let too_deep = Binding::Array(vec![nested]);
    assert!(matches!(
        resolve(&too_deep, &ctx, ContextLimits::default()),
        Err(ContextError::Limit("binding depth"))
    ));
    let exact = Binding::Array(
        (0..MAX_BINDING_NODES - 1)
            .map(|_| Binding::Literal(Value::Null))
            .collect(),
    );
    assert!(resolve(&exact, &ctx, ContextLimits::default()).is_ok());
    let excessive = Binding::Array(
        (0..MAX_BINDING_NODES)
            .map(|_| Binding::Literal(Value::Null))
            .collect(),
    );
    assert!(matches!(
        resolve(&excessive, &ctx, ContextLimits::default()),
        Err(ContextError::Limit("binding nodes"))
    ));
    let many_segments = format!("/input{}", "/x".repeat(MAX_POINTER_SEGMENTS));
    assert!(matches!(
        ContextReference::parse(&many_segments),
        Err(ContextError::Limit("pointer segments"))
    ));
    let long_pointer = format!("/input/{}", "x".repeat(MAX_POINTER_BYTES));
    assert!(matches!(
        ContextReference::parse(&long_pointer),
        Err(ContextError::Limit("pointer bytes"))
    ));
    let exact_output = Binding::Literal(Value::String("x".repeat(MAX_OUTPUT_BYTES - 2)));
    assert!(resolve(&exact_output, &ctx, ContextLimits::default()).is_ok());
    let oversized_key = Binding::Object(BTreeMap::from([(
        "x".repeat(MAX_OUTPUT_BYTES + 1),
        Binding::Literal(Value::Null),
    )]));
    assert!(matches!(
        resolve(&oversized_key, &ctx, ContextLimits::default()),
        Err(ContextError::Limit("output bytes"))
    ));
}
