use super::*;

/// The no-JS path: a native urlencoded submission builds the same write the scripted form does.
#[tokio::test]
async fn a_native_urlencoded_submission_builds_the_write() {
    use axum::{body::Body, extract::FromRequest, http::Request};
    let request = Request::builder()
        .method("POST")
        .uri("/ui/agents")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("name=NoJs&description=Answers+billing"))
        .unwrap();
    let Form(form) = Form::<AgentForm>::from_request(request, &()).await.unwrap();
    let submitted = SubmittedAgent::new(form);
    let mut write = submitted.agent_write().unwrap();
    write.normalize().unwrap();
    assert_eq!(write.name, "NoJs");
    assert_eq!(write.slug, "nojs");
    assert_eq!(write.description.as_deref(), Some("Answers billing"));
    let html = pages::library_agent_fields(&submitted.draft(), None);
    assert!(html.contains("value=\"NoJs\""));
}
