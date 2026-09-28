use super::super::{SourceSpan, check_dependencies};
use super::test_support::*;
use super::*;
use crate::domain::workflow::{
    Binding, ContextLimits, ContextReference, RunId, RunMetadata, resolve, resolve_inputs,
};
use serde_json::json;
use uuid::Uuid;

fn current() -> WorkflowId {
    WorkflowId::new(Uuid::parse_str(ID).unwrap())
}

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    }
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

#[test]
fn dependency_preflight_limits_and_iterative_long_chain() {
    let ids: Vec<_> = (1..=4096)
        .map(|n| WorkflowId::new(Uuid::from_u128(n)))
        .collect();
    let mut graph: BTreeMap<WorkflowId, BTreeSet<WorkflowId>> = BTreeMap::new();
    for (index, id) in ids.iter().enumerate() {
        graph.insert(*id, ids.get(index + 1).copied().into_iter().collect());
    }
    let children = graph[&current()].clone();
    let closure = check_dependencies(current(), &children, &graph, span()).unwrap();
    assert_eq!(closure.len(), 4096);
    graph.insert(WorkflowId::new(Uuid::from_u128(4097)), BTreeSet::new());
    assert_eq!(
        check_dependencies(current(), &children, &graph, span())
            .unwrap_err()
            .code,
        "dependency.limit"
    );
    graph.remove(&WorkflowId::new(Uuid::from_u128(4097)));
    graph.remove(ids.last().unwrap());
    assert_eq!(
        check_dependencies(current(), &children, &graph, span())
            .unwrap_err()
            .code,
        "dependency.missing"
    );
    graph.insert(*ids.last().unwrap(), BTreeSet::from([current()]));
    assert_eq!(
        check_dependencies(current(), &children, &graph, span())
            .unwrap_err()
            .code,
        "dependency.cycle"
    );
}

#[test]
fn dependency_edge_limit_and_unrelated_facts_do_not_affect_identity() {
    let ids: Vec<_> = (1..=4096)
        .map(|n| WorkflowId::new(Uuid::from_u128(n)))
        .collect();
    let mut graph = BTreeMap::new();
    for (index, id) in ids.iter().enumerate() {
        let mut edges = BTreeSet::new();
        for offset in [1, 2] {
            if let Some(next) = ids.get(index + offset) {
                edges.insert(*next);
            }
        }
        if index < 3 {
            edges.insert(ids[index + 3]);
        }
        graph.insert(*id, edges);
    }
    let children = graph[&current()].clone();
    assert_eq!(graph.values().map(BTreeSet::len).sum::<usize>(), 8192);
    assert_eq!(
        check_dependencies(current(), &children, &graph, span())
            .unwrap()
            .len(),
        4096
    );
    graph.get_mut(&ids[0]).unwrap().insert(ids[4]);
    assert_eq!(
        check_dependencies(current(), &graph[&current()].clone(), &graph, span())
            .unwrap_err()
            .code,
        "dependency.limit"
    );

    let source = single("{literal: hello}");
    let base = compile_one(&source).unwrap().content_hash().to_owned();
    let mut unrelated = empty_deps();
    unrelated.insert(WorkflowId::new(Uuid::from_u128(2)), BTreeSet::new());
    assert_eq!(
        compile_custom(&source, vec![descriptor()], unrelated)
            .unwrap()
            .content_hash(),
        base
    );
}

#[test]
fn serialized_facts_have_an_exact_byte_ceiling() {
    let mut descriptors = Map::new();
    descriptors.insert("x".to_owned(), json!(""));
    let dependencies = BTreeMap::new();
    let framing = serde_json::to_vec(&(&descriptors, &dependencies))
        .unwrap()
        .len();
    descriptors.insert(
        "x".to_owned(),
        json!("a".repeat(MAX_COMPILED_FACT_BYTES - framing)),
    );
    assert!(check_fact_budget(&descriptors, &dependencies, span()).is_ok());
    descriptors.insert(
        "x".to_owned(),
        json!("a".repeat(MAX_COMPILED_FACT_BYTES - framing + 1)),
    );
    assert_eq!(
        check_fact_budget(&descriptors, &dependencies, span())
            .unwrap_err()
            .code,
        "dependency.limit"
    );
}

#[test]
fn static_checks_reject_bad_operators_and_impossible_input_roots() {
    for schema in [json!(true), json!({"type":"object"})] {
        let mut descriptor = descriptor();
        descriptor.input_schema = schema;
        let error = compile_custom(
            &single("{concat: [{literal: 1}]}"),
            vec![descriptor.clone()],
            empty_deps(),
        )
        .err()
        .unwrap();
        assert_eq!(error.code, "binding.operand");
        let error = compile_custom(
            &single("{and: [{literal: false}, {concat: [{literal: 1}]}]}"),
            vec![descriptor],
            empty_deps(),
        )
        .err()
        .unwrap();
        assert_eq!(error.code, "binding.operand");
    }
    for schema in [json!(false), json!({"type":"string"})] {
        let mut descriptor = descriptor();
        descriptor.input_schema = schema;
        assert_eq!(
            compile_custom(&single("{literal: hello}"), vec![descriptor], empty_deps())
                .err()
                .unwrap()
                .code,
            "binding.type"
        );
    }
    let mut referenced = descriptor();
    referenced.input_schema = json!({"type":"object","properties":{"message":{"$ref":"#/$defs/text"}},"$defs":{"text":{"type":"string"}}});
    assert_eq!(
        compile_custom(&single("{literal: 1}"), vec![referenced], empty_deps())
            .err()
            .unwrap()
            .code,
        "binding.type"
    );
    let mut descriptor = descriptor();
    descriptor.input_schema = json!({"type":"object","properties":{"message":{"type":"integer"}}});
    assert!(compile_custom(&single("{literal: 1.0}"), vec![descriptor], empty_deps()).is_ok());
}

#[test]
fn union_absence_allows_default_on_both_runtime_branches() {
    let source = single("{default: {ref: /input/message, value: {literal: fallback}}}").replace(
        "{type: object, properties: {message: {type: string}}, required: [message]}",
        "{anyOf: [{type: object, properties: {message: {type: string}}, required: [message], additionalProperties: false}, {type: object, properties: {other: {type: string}}, required: [other], additionalProperties: false}]}"
    );
    let compiled = compile_one(&source).unwrap();
    let params = json!({});
    let outputs = BTreeMap::new();
    let step = StepId::parse("start").unwrap();
    for (input, expected) in [
        (json!({"message":"present"}), "present"),
        (json!({"other":"value"}), "fallback"),
    ] {
        assert_eq!(
            compiled
                .prepare_step_inputs(&step, &context(&input, &params, &outputs))
                .unwrap()
                .value()["message"],
            expected
        );
    }
    let source = single("{default: {ref: /input/items/+1, value: {literal: fallback}}}").replace(
        "{type: object, properties: {message: {type: string}}, required: [message]}",
        "{type: object, properties: {items: {type: array, items: {type: string}}}, required: [items]}"
    );
    assert_eq!(
        compile_one(&source).err().unwrap().code,
        "reference.impossible"
    );
}

#[test]
fn malformed_ordered_rule_descriptor_returns_diagnostic() {
    let mut descriptor = descriptor();
    descriptor.ordered_rule = true;
    assert_eq!(
        compile_custom(&single("{literal: hello}"), vec![descriptor], empty_deps())
            .err()
            .unwrap()
            .code,
        "step.descriptor"
    );
}

#[test]
fn escaped_unicode_names_preserve_source_location_and_runtime_error() {
    let source = single("{default: {ref: /input/message/nested, value: {literal: fallback}}}")
        .replace("message: {default:", "'é/~': {default:")
        .replace("input_schema: {type: object, properties: {message: {type: string}}, required: [message]}", "input_schema: true");
    let mut descriptor = descriptor();
    descriptor.input_schema = json!({"type":"object"});
    let compiled = compile_custom(&source, vec![descriptor], empty_deps()).unwrap();
    let input = json!({"message":"ok"});
    let params = json!({});
    let outputs = BTreeMap::new();
    let diagnostic = compiled
        .prepare_step_inputs(
            &StepId::parse("start").unwrap(),
            &context(&input, &params, &outputs),
        )
        .err()
        .unwrap();
    assert_eq!(diagnostic.code, "binding.reference");
    assert_eq!(diagnostic.field_path.as_ref(), "/steps/start/with/é~1~0");
    assert_eq!(diagnostic.span.start, source.find("{default:").unwrap());
}

#[test]
fn aggregate_input_budget_charges_siblings_and_exact_json_size() {
    let input = json!({"value":"x".repeat(2048)});
    let params = json!({});
    let outputs = BTreeMap::new();
    let ctx = context(&input, &params, &outputs);
    let bindings: BTreeMap<_, _> = (0..256)
        .map(|index| {
            (
                format!("field{index}"),
                Binding::Reference(ContextReference::parse("/input/value").unwrap()),
            )
        })
        .collect();
    assert!(matches!(
        resolve_inputs(
            &bindings,
            &ctx,
            ContextLimits {
                output_bytes: 4096,
                work_nodes: 65_536
            }
        ),
        Err((
            Some(_),
            crate::domain::workflow::ContextError::Limit("output bytes")
        ))
    ));

    let one = BTreeMap::from([("a".to_owned(), Binding::Literal(json!("x")))]);
    assert_eq!(
        resolve_inputs(
            &one,
            &ctx,
            ContextLimits {
                output_bytes: 9,
                work_nodes: 65_536
            }
        )
        .unwrap(),
        json!({"a":"x"})
    );
    assert!(matches!(
        resolve_inputs(
            &one,
            &ctx,
            ContextLimits {
                output_bytes: 8,
                work_nodes: 65_536
            }
        ),
        Err((
            Some(_),
            crate::domain::workflow::ContextError::Limit("output bytes")
        ))
    ));
}

#[test]
fn concat_charges_final_json_once_with_exact_and_one_over_limits() {
    let input = json!({"part":"llo"});
    let params = json!({});
    let outputs = BTreeMap::new();
    let ctx = context(&input, &params, &outputs);
    let cases = [
        Binding::Concat(vec![Binding::Literal(json!("hello"))]),
        Binding::Concat(vec![
            Binding::Literal(json!("he")),
            Binding::Literal(json!("llo")),
        ]),
        Binding::Concat(vec![
            Binding::Literal(json!("he")),
            Binding::Reference(ContextReference::parse("/input/part").unwrap()),
        ]),
        Binding::Concat(vec![
            Binding::Literal(json!("h")),
            Binding::Concat(vec![Binding::Literal(json!("ello"))]),
        ]),
    ];
    for binding in cases {
        assert_eq!(
            resolve(
                &binding,
                &ctx,
                ContextLimits {
                    output_bytes: 7,
                    work_nodes: 65_536
                }
            )
            .unwrap(),
            json!("hello")
        );
        assert!(matches!(
            resolve(
                &binding,
                &ctx,
                ContextLimits {
                    output_bytes: 6,
                    work_nodes: 65_536
                }
            ),
            Err(crate::domain::workflow::ContextError::Limit("output bytes"))
        ));
    }
}
