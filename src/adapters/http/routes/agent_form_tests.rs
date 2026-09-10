use super::*;
use crate::entities::harness::HarnessKind;
use serde_json::json;

#[test]
fn failed_schema_and_unknown_harness_keep_the_draft_and_grants() {
    let form: AgentForm = serde_json::from_value(json!({
        "name":"Draft", "harness_kind":"future_runtime", "config_json":"{broken",
        "response_format":"json_schema", "response_schema":"{bad schema",
        "granted_tool_ids":"calculator"
    }))
    .unwrap();
    let submitted = SubmittedAgent::new(form);
    assert!(submitted.agent_write().is_err());
    let draft = submitted.draft();
    assert_eq!(draft.harness_kind_raw, Some("future_runtime"));
    assert_eq!(draft.response_schema, "{bad schema");
    assert_eq!(draft.config_json, "{broken");
    assert_eq!(draft.granted_tool_ids[0].as_str(), "calculator");
    let html = pages::library_agent_fields(&draft, None);
    assert!(html.contains("Unavailable harness: future_runtime"));
    assert!(html.contains("{bad schema"));
}

#[test]
fn channel_step_carries_the_shared_response_contract_and_explicit_harness() {
    let carried: CarriedAgent = serde_json::from_value(json!({
        "agent_name":"Draft", "agent_harness_kind":"rig", "agent_config_json":"{\"version\":1,\"max_turns\":4}",
        "agent_response_format":"json_schema", "agent_response_schema":"{\"type\":\"object\"}"
    })).unwrap();
    let submitted = SubmittedAgent::new(carried.into());
    let write = submitted.agent_write().unwrap();
    assert_eq!(write.harness_kind, Some(HarnessKind::Rig));
    assert_eq!(write.config_json.unwrap()["max_turns"], 4);
    assert_eq!(
        write.response_contract.0.unwrap().unwrap().schema(),
        &json!({"type":"object"})
    );
}

#[tokio::test]
async fn native_urlencoded_submission_rejects_incompatible_contracts_and_retains_schema() {
    use axum::{body::Body, extract::FromRequest, http::Request};
    let request = Request::builder().method("POST").uri("/ui/agents")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("name=NoJs&harness_kind=ai_agents&config_json=%7B%22version%22%3A1%7D&response_format=json_schema&response_schema=%7B%22type%22%3A%22object%22%7D")).unwrap();
    let Form(form) = Form::<AgentForm>::from_request(request, &()).await.unwrap();
    let submitted = SubmittedAgent::new(form);
    let mut write = submitted.agent_write().unwrap();
    assert!(
        write
            .normalize()
            .unwrap_err()
            .to_string()
            .contains("response_contract requires the rig harness")
    );
    let html = pages::library_agent_fields(&submitted.draft(), None);
    assert!(html.contains("value=\"ai_agents\" selected"));
    assert!(html.contains("value=\"json_schema\" selected"));
    assert!(html.contains(&pages::escape_html_text(r#"{"type":"object"}"#)));
}
