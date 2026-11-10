use super::*;
#[test]
fn mcp_detail_escapes_drafts_and_keeps_tokens_write_only() {
    let company = Uuid::new_v4();
    let c = CompanyMcpConnection {
        access_level: crate::entities::access_level::AccessLevel::PUBLIC,
        id: Uuid::new_v4(),
        company_id: company,
        slug: "crm".into(),
        endpoint: "https://example.com/mcp".to_string().try_into().unwrap(),
        enabled: false,
        auth: McpAuth::Bearer,
        secret_set: true,
        revision: 3,
        credential_revision: 2,
        discovered_tools: vec![],
        tool_grants: vec![],
    };
    let mut draft = DefinitionDraft::stored(&c);
    let attack = format!("<{0}>alert(1)</{0}>", "script");
    draft.slug = attack.clone();
    let html = detail(company, &c, &draft);
    assert!(!html.contains(&attack));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("type=\"password\"") && html.contains("Secret set"));
    assert!(html.contains("data-submit=\"busy-once\""));
    assert!(!html.contains("requires_approval"));
}

#[test]
fn doop_catalog_review_escapes_both_schemas_and_labels_undeclared_output() {
    let input_attack = concat!("<script>", "input_attack()", "</script>");
    let output_attack = concat!("<script>", "output_attack()", "</script>");
    let mut tool = McpDiscoveredTool {
        name: "get_canvas".to_owned().try_into().unwrap(),
        description: String::new(),
        input_schema: serde_json::json!({"description":input_attack}),
        output_schema: Some(serde_json::json!({"description":output_attack})),
    };
    let rendered = schema_review(&tool);
    for attack in [input_attack, output_attack] {
        assert!(!rendered.contains(attack));
        assert!(rendered.contains(&escape_html_text(attack)));
    }
    assert!(rendered.contains(&schema_fingerprint(&tool.input_schema)));
    assert!(rendered.contains(&schema_fingerprint(tool.output_schema.as_ref().unwrap())));
    tool.output_schema = None;
    let rendered = schema_review(&tool);
    assert!(rendered.contains("Not declared by the server"));
    assert!(!rendered.contains("output_attack"));
}
