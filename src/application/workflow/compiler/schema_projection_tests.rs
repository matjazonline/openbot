use super::*;

fn projection(raw: Value, path: &[&str]) -> PathGuarantee {
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
    Schema::compile(raw, "/schema", span)
        .unwrap()
        .guarantees(&path.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn workflow_schema_allof_keeps_sibling_required_properties() {
    let mut schema = json!({
        "type":"object",
        "properties":{"data":{"type":"object","properties":{"body":{"type":"string"}},"required":["body"],"additionalProperties":false}},
        "required":["data"],"additionalProperties":false,
        "allOf":[{"if":{"properties":{"choice":{"const":"revise"}}},"then":{"properties":{"feedback":{"minLength":1}}}}]
    });
    assert_eq!(
        projection(schema.clone(), &["data"]),
        PathGuarantee::Guaranteed
    );
    assert_eq!(
        projection(schema.clone(), &["data", "body"]),
        PathGuarantee::Guaranteed
    );
    assert_eq!(
        projection(schema.clone(), &["missing"]),
        PathGuarantee::Impossible
    );
    schema["required"] = json!([]);
    assert_eq!(
        projection(schema.clone(), &["data"]),
        PathGuarantee::Optional
    );
    schema["properties"]["data"]["type"] = json!(["object", "null"]);
    assert_eq!(
        projection(schema, &["data", "body"]),
        PathGuarantee::UnsafeTraversal
    );
}

#[test]
fn workflow_schema_allof_preserves_branch_and_sibling_restrictions() {
    let required =
        json!({"type":"object","properties":{"data":{"type":"string"}},"required":["data"]});
    assert_eq!(
        projection(json!({"allOf":[required.clone()]}), &["data"]),
        PathGuarantee::Guaranteed
    );
    assert_eq!(
        projection(json!({"allOf":[required.clone(), false]}), &["data"]),
        PathGuarantee::Impossible
    );
    assert_eq!(
        projection(
            json!({"type":"object","additionalProperties":false,"allOf":[required]}),
            &["data"]
        ),
        PathGuarantee::Impossible
    );
}
