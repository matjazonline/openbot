use super::*;
use crate::application::workflow::{
    compiler::{Schema, SourceSpan},
    provisioning::is_reserved_tool_name,
};
use crate::domain::workflow::ResourceName;
use uuid::Uuid;

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    }
}

#[test]
fn knowledge_search_schema_rejects_level_extra_fields_and_exact_term_bounds() {
    let schema = Schema::compile(knowledge_tool_schemas().input, "/input", span()).unwrap();
    for value in [
        json!({"query":"price", "level":5}),
        json!({"query":"price", "extra":true}),
        json!({"query":"price", "exact_terms":vec!["a";9]}),
        json!({"query":"price", "exact_terms":["x".repeat(65)]}),
        json!({"query":"x".repeat(1001)}),
    ] {
        assert!(
            schema.validate(&value, "/input", span()).is_err(),
            "{value}"
        );
    }
    assert!(
        schema
            .validate(
                &json!({"query":"界".repeat(1000),"exact_terms":["界".repeat(64)]}),
                "/input",
                span()
            )
            .is_ok()
    );
}

#[test]
fn knowledge_search_snapshot_is_reserved_and_exactly_trusted_without_external_targets() {
    assert!(is_reserved_tool_name(SEARCH_KNOWLEDGE));
    let snapshot = builtin_knowledge_snapshot(CompanyId::new(Uuid::new_v4())).unwrap();
    assert_eq!(snapshot.kind(), ToolKind::Knowledge);
    assert!(is_trusted_knowledge_snapshot(&snapshot).unwrap());
    let mut external = snapshot.clone();
    external.contract.connection = Some(ResourceName::parse("external").unwrap());
    assert!(matches!(external.kind(), ToolKind::Slot(_)));
    assert!(!is_trusted_knowledge_snapshot(&external).unwrap());
    let mut changed = snapshot;
    changed.contract.input_schema = json!({"type":"object"});
    assert!(!is_trusted_knowledge_snapshot(&changed).unwrap());
}

#[test]
fn knowledge_search_description_requires_exact_terms_citations_and_dont_have_that() {
    for required in [
        "every code, SKU, order number or exact product name",
        "exact_terms",
        "only from the returned passages",
        "cite the file name",
        "I don't have that information",
        "Never say that information is restricted or hidden",
    ] {
        assert!(KNOWLEDGE_TOOL_DESCRIPTION.contains(required), "{required}");
    }
}
