use super::*;

#[test]
fn documented_company_agent_update_decodes_with_explicit_native_permissions() {
    let guide = include_str!("../../../../docs/agent-authoring.md");
    let (_, update) = guide.split_once("Update an existing draft with").unwrap();
    let (_, example) = update.split_once("```json\n").unwrap();
    let (json, _) = example.split_once("```").unwrap();
    let payload: crate::adapters::http::routes::agent::CompanyAgentJsonUpdate =
        serde_json::from_str(json).unwrap();
    assert_eq!(
        payload.agent.native_permissions,
        crate::domain::local_agents::LocalPermissions::Full
    );
}

/// The no-JS path: a native urlencoded submission builds the same write the scripted form does.
#[tokio::test]
async fn a_native_urlencoded_submission_builds_the_write() {
    use axum::{body::Body, extract::FromRequest, http::Request};
    let request = Request::builder()
        .method("POST")
        .uri("/ui/agents")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "name=NoJs&description=Answers+billing&native_permissions=accept_edits",
        ))
        .unwrap();
    let Form(form) = Form::<AgentForm>::from_request(request, &()).await.unwrap();
    let submitted = SubmittedAgent::new(form);
    let mut write = submitted.agent_write().unwrap();
    write.normalize().unwrap();
    assert_eq!(
        write.native_permissions,
        crate::domain::local_agents::LocalPermissions::AcceptEdits
    );
    assert_eq!(write.name, "NoJs");
    assert_eq!(write.slug, "nojs");
    assert_eq!(write.description.as_deref(), Some("Answers billing"));
    let html = pages::library_agent_fields(&submitted.draft(), None);
    assert!(html.contains("value=\"NoJs\""));
}
