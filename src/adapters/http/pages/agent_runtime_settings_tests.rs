use super::*;
#[test]
fn runtime_and_schema_drafts_are_escaped_without_rewriting_values() {
    let opening = "<script>";
    let schema = format!(r#"{{"const":"</textarea>{opening}alert(1)</script>"}}"#);
    let html = fields(&AgentDraft {
        harness_kind: Some(HarnessKind::Rig),
        response_format: "json_schema",
        response_schema: schema.clone(),
        config_json: "{broken",
        ..Default::default()
    });
    assert!(html.contains("value=\"rig\" selected"));
    assert!(html.contains("value=\"json_schema\" selected"));
    assert!(html.contains(&escape_html_text(&schema)));
    assert!(!html.contains(opening));
    assert!(html.contains("{broken"));
    assert!(html.contains("two repair calls"));
}
#[test]
fn runtime_summary_never_exposes_compiled_config_or_relabels_history() {
    let legacy = execution_summary(
        &serde_json::json!({"execution_parameters":{"config":{"api_key":"hidden"}}}),
    );
    assert!(legacy.contains("not recorded"));
    assert!(!legacy.contains("Rig"));
    let html = execution_summary(
        &serde_json::json!({"execution_parameters":{"harness_kind":"rig","config":{"api_key":"hidden"}}}),
    );
    assert!(html.contains("Reviewed settings unavailable"));
    assert!(!html.contains("hidden"));
}
