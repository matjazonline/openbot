use super::test_support::*;
use super::*;

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    }
}
#[test]
fn workflow_expanded_descriptor_serialization_matches_exact_byte_limit() {
    let id = StepId::parse("start").unwrap();
    let mut descriptors = BTreeMap::from([(
        id,
        SpecializedDescriptor {
            descriptor: descriptor(),
            facts: json!({"padding":""}),
            checks: vec![],
        },
    )]);
    let dependencies = BTreeMap::new();
    let representation: Map<String, Value> = descriptors
        .iter()
        .map(|(id, d)| {
            (
                id.to_string(),
                json!({"contract":descriptor_fact(&d.descriptor),"registration":d.facts}),
            )
        })
        .collect();
    let overhead = serde_json::to_vec(&(representation, &dependencies))
        .unwrap()
        .len();
    let descriptor = descriptors.values_mut().next().unwrap();
    descriptor.facts["padding"] = json!("x".repeat(MAX_COMPILED_FACT_BYTES - overhead));
    assert!(check_expanded_facts(&descriptors, &dependencies, span()).is_ok());
    descriptors.values_mut().next().unwrap().facts["padding"] =
        json!("x".repeat(MAX_COMPILED_FACT_BYTES - overhead + 1));
    assert_eq!(
        check_expanded_facts(&descriptors, &dependencies, span())
            .unwrap_err()
            .code,
        "dependency.limit"
    );
}

#[test]
fn workflow_explicit_descriptor_reuse_is_rejected_before_clones_and_validators() {
    let mut descriptor = descriptor();
    descriptor.output_schema = json!({"type":"invalid","description":"x".repeat(60_000)});
    let source = single("{literal: ok}").replace("max_steps: 20", "max_steps: 32");
    let start = source.find("steps:\n").unwrap() + "steps:\n".len();
    let end = source.find("limits:").unwrap();
    let mut steps = String::new();
    for i in 0..32 {
        let id = if i == 0 {
            "start".to_owned()
        } else {
            format!("step{i}")
        };
        let target = if i == 31 {
            "$end".to_owned()
        } else {
            format!("step{}", i + 1)
        };
        steps.push_str(&format!("  {id}:\n    type: data.map\n    with: {{message: {{literal: ok}}}}\n    routes: {{success: {target}}}\n"));
    }
    let source = format!("{}{}{}", &source[..start], steps, &source[end..]);
    assert_eq!(
        compile_custom(&source, vec![descriptor], empty_deps())
            .err()
            .unwrap()
            .code,
        "dependency.limit"
    );
}

#[test]
fn workflow_expanded_schema_work_is_bounded_before_serialization() {
    let mut budget = FactBudget::new(span());
    let value = json!(vec![0; 4095]);
    for _ in 0..16 {
        budget.schema(&value).unwrap();
    }
    assert_eq!(
        budget.schema(&Value::Null).unwrap_err().code,
        "dependency.limit"
    );
}
