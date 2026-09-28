use super::tests::*;
use super::*;
#[test]
fn workflow_registry_nested_schema_refs_ids_and_data_keywords_preserve_native_meaning() {
    for reference in ["$ref", "$dynamicRef"] {
        let mut example = example("data.map").unwrap();
        let mut schema = json!({"$id":"https://schemas.invalid/local","$defs":{"text":{"type":"string","minLength":2}},"maxLength":5});
        schema[reference] = json!("#/$defs/text");
        changed(&mut example, |v| {
            v["steps"]["start"]["with"]["output_schema"] = json!({"literal":schema})
        });
        let compiled = compile_example(&example).unwrap();
        assert!(compiled.validate_step_output(&step(), &json!("a")).is_err());
        assert!(
            compiled
                .validate_step_output(&step(), &json!("longer"))
                .is_err()
        );
        compiled
            .validate_step_output(&step(), &json!("good"))
            .unwrap();
    }
    let mut example = example("data.map").unwrap();
    let data = json!({"$schema":"plain data","$ref":"https://not-fetched.invalid","nested":{"$dynamicRef":"#"}});
    changed(&mut example, |v| {
        v["steps"]["start"]["with"]["output_schema"] = json!({"literal":{"const":data}});
        v["steps"]["start"]["with"]["value"] = json!({"literal":data});
    });
    compile_example(&example)
        .unwrap()
        .validate_step_output(&step(), &data)
        .unwrap();
}
#[test]
fn workflow_registry_schema_budget_retains_external_recursion_depth_node_byte_rejection() {
    for schema in [
        json!({"$ref":"https://not-fetched.invalid"}),
        json!({"$ref":"#"}),
        json!({"$dynamicRef":"#"}),
        json!({"$defs":{"nested":{"$id":"https://schemas.invalid/local","$ref":"#"}},"type":"string"}),
    ] {
        let mut example = example("data.map").unwrap();
        changed(&mut example, |v| {
            v["steps"]["start"]["with"]["output_schema"] = json!({"literal":schema})
        });
        assert_eq!(
            compile_example(&example).err().unwrap().code,
            "schema.resource"
        );
    }
    let mut example = example("tool.call").unwrap();
    example.facts.tools[0].output_schema = json!({"description":"x".repeat(65537)});
    assert_eq!(
        compile_example(&example).err().unwrap().code,
        "schema.resource"
    );
    let mut nested = json!(true);
    for _ in 0..34 {
        nested = json!({"not":nested});
    }
    example.facts.tools[0].output_schema = nested;
    assert!(compile_example(&example).is_err());
    example.facts.tools[0].output_schema = json!({"enum":(0..4097).collect::<Vec<_>>()});
    assert_eq!(
        compile_example(&example).err().unwrap().code,
        "schema.resource"
    );
}
#[test]
fn workflow_registry_metadata_matches_effect_families_and_never_claims_authorization() {
    for (name, effect, recovery, shared) in [
        ("data.map", "pure", "recompute", false),
        (
            "context.load",
            "bounded_read",
            "reuse_committed_result",
            false,
        ),
        (
            "ai.classify",
            "model_invocation",
            "reuse_committed_result",
            false,
        ),
        (
            "agent.run",
            "agent_actions",
            "logical_idempotency_receipt_reconciliation",
            true,
        ),
        (
            "memory.save",
            "durable_write",
            "logical_idempotency_receipt_reconciliation",
            false,
        ),
        (
            "http.request",
            "shared_action",
            "logical_idempotency_receipt_reconciliation",
            true,
        ),
        (
            "wait.timer",
            "durable_suspension",
            "resume_durable_identity",
            false,
        ),
        (
            "flow.repeat",
            "child_control",
            "resume_durable_identity",
            false,
        ),
    ] {
        let compiled = compile_example(&example(name).unwrap()).unwrap();
        let constraints =
            &compiled.representation()["descriptors"]["start"]["registration"]["constraints"];
        assert_eq!(constraints["effect"], effect);
        assert_eq!(constraints["recovery"], recovery);
        assert_eq!(constraints["shared_action_required"], shared);
        assert_eq!(
            constraints["capabilities"].as_array().unwrap().is_empty(),
            name == "data.map"
        );
        assert!(constraints.get("authorized").is_none());
    }
}
#[test]
fn workflow_registry_semantic_errors_locate_literal_values_without_leaking_them() {
    let mut example = example("http.request").unwrap();
    changed(&mut example, |v| {
        v["steps"]["start"]["with"]["headers"] = json!({"literal":{"Authorization":"secret-value"}})
    });
    let error = compile_example(&example).err().unwrap();
    assert_eq!(error.code, "registry.http_header");
    assert_eq!(
        error.field_path.as_ref(),
        "/steps/start/with/headers/Authorization"
    );
    assert_eq!(
        error.span.start,
        example.source.find("\"secret-value\"").unwrap()
    );
    assert!(!error.message.contains("secret-value"));
}
