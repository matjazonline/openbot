use super::*;
#[test]
fn mcp_forms_keep_company_configuration_out_of_agent_selections() {
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
    let selected = AgentMcpSelection {
        company_id: company,
        agent_id: Uuid::new_v4(),
        revision: 4,
        connection_ids: vec![c.id],
    };
    let html = selection(company, &selected, std::slice::from_ref(&c), None);
    assert!(html.contains(" multiple ") && html.contains("(unavailable)"));
    assert!(html.contains(&format!("value=\"{}\" selected", c.id)));
    for field in [
        "endpoint_url",
        "auth_type",
        "name=\"token\"",
        "name=\"grants\"",
    ] {
        assert!(!html.contains(field));
    }
    let mut draft = DefinitionDraft::stored(&c);
    let attack = format!("<{0}>alert(1)</{0}>", "script");
    draft.slug = attack.clone();
    let html = detail(company, &c, &[], &draft);
    assert!(!html.contains(&attack));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("type=\"password\"") && html.contains("Secret set"));
    assert!(html.contains("data-submit=\"busy-once\""));
    assert!(!html.contains("requires_approval"));
}
