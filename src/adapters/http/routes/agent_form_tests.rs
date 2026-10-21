use super::*;
use serde_json::json;

#[test]
fn failed_schema_and_config_keep_the_draft() {
    let form: AgentForm = serde_json::from_value(json!({
        "name":"Draft", "config_json":"{broken",
        "response_format":"json_schema", "response_schema":"{bad schema"
    }))
    .unwrap();
    let submitted = SubmittedAgent::new(form);
    assert!(submitted.agent_write().is_err());
    let draft = submitted.draft();
    assert_eq!(draft.response_schema, "{bad schema");
    assert_eq!(draft.config_json, "{broken");
    let html = pages::library_agent_fields(&draft, None);
    assert!(html.contains("{bad schema"));
    assert!(html.contains("{broken"));
}

#[test]
fn channel_step_carries_the_shared_response_contract_and_rig_config() {
    let carried: CarriedAgent = serde_json::from_value(json!({
        "agent_name":"Draft", "agent_config_json":"{\"version\":1,\"max_turns\":4}",
        "agent_response_format":"json_schema", "agent_response_schema":"{\"type\":\"object\"}"
    }))
    .unwrap();
    let submitted = SubmittedAgent::new(carried.into());
    let write = submitted.agent_write().unwrap();
    assert_eq!(write.config_json.unwrap()["max_turns"], 4);
    assert_eq!(
        write.response_contract.0.unwrap().unwrap().schema(),
        &json!({"type":"object"})
    );
}

#[tokio::test]
async fn native_urlencoded_submission_retains_the_schema_and_ignores_retired_fields() {
    use axum::{body::Body, extract::FromRequest, http::Request};
    let request = Request::builder().method("POST").uri("/ui/agents")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("name=NoJs&harness_kind=ai_agents&granted_tool_ids=calculator&config_json=%7B%22version%22%3A1%7D&response_format=json_schema&response_schema=%7B%22type%22%3A%22object%22%7D")).unwrap();
    let Form(form) = Form::<AgentForm>::from_request(request, &()).await.unwrap();
    let submitted = SubmittedAgent::new(form);
    let mut write = submitted.agent_write().unwrap();
    write.normalize().unwrap();
    assert_eq!(
        write.response_contract.0.unwrap().unwrap().schema(),
        &json!({"type":"object"})
    );
    let html = pages::library_agent_fields(&submitted.draft(), None);
    assert!(!html.contains("ai_agents"));
    assert!(html.contains("value=\"json_schema\" selected"));
    assert!(html.contains(&pages::escape_html_text(r#"{"type":"object"}"#)));
}
