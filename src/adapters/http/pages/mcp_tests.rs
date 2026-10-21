use super::*;
#[test]
fn mcp_detail_escapes_drafts_and_keeps_tokens_write_only() {
    let company = Uuid::new_v4();
    let c = CompanyMcpConnection {
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
