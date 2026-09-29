use super::test_support::*;
use super::*;
use crate::domain::workflow::{BudgetResource, RootBudgetLimits};

fn with_budget(budget: &str) -> String {
    single("{literal: hello}").replace("limits: {", &format!("limits: {{root_budget: {budget}, "))
}

#[test]
fn workflow_root_budget_compiler_freezes_defaults_and_authored_limits() {
    let default = compile_one(&single("{literal: hello}")).unwrap();
    assert_eq!(
        default.graph().definition().limits.root_budget,
        RootBudgetLimits::default()
    );
    assert_eq!(
        default.representation()["root_budget"],
        json!({"activations":100000,"model_calls":1000,"repetitions":1000})
    );
    let source = with_budget("{activations: 7, model_calls: 3, repetitions: 2}");
    let lower = compile_one(&source).unwrap();
    let limits = lower.graph().definition().limits;
    assert_eq!(limits.max_steps, 20);
    assert_eq!(limits.max_context_bytes, 4096);
    assert_eq!(limits.root_budget, RootBudgetLimits::new(7, 3, 2).unwrap());
    assert_eq!(
        lower.representation()["root_budget"],
        json!({"activations":7,"model_calls":3,"repetitions":2})
    );
    assert_ne!(default.content_hash(), lower.content_hash());
    assert_eq!(
        lower.content_hash(),
        compile_one(&source).unwrap().content_hash()
    );
}

#[test]
fn workflow_root_budget_compiler_boundary_and_invalid_values_are_located() {
    let exact = "{activations: 100000, model_calls: 1000, repetitions: 1000}";
    assert!(compile_one(&with_budget(exact)).is_ok());
    for (field, resource) in [
        ("activations", BudgetResource::Activation),
        ("model_calls", BudgetResource::ModelCall),
        ("repetitions", BudgetResource::Repetition),
    ] {
        let maximum = resource.maximum().to_string();
        for bad in [
            "0".to_owned(),
            (resource.maximum() + 1).to_string(),
            "-1".to_owned(),
            "1.5".to_owned(),
            "4294967296".to_owned(),
            "null".to_owned(),
        ] {
            let value = exact.replace(&format!("{field}: {maximum}"), &format!("{field}: {bad}"));
            let error = compile_one(&with_budget(&value)).err().unwrap();
            assert_eq!(
                error.field_path.as_ref(),
                format!("/limits/root_budget/{field}")
            );
            assert!(error.span.line > 0);
        }
    }
    for malformed in [
        "null",
        "{}",
        "[]",
        "{activations: 1, model_calls: 1, repetitions: 1, extra: 1}",
    ] {
        assert!(compile_one(&with_budget(malformed)).is_err());
    }
}
