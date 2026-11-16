//! Bounded manual browser journey over real decision handlers and an isolated fixture session.
use super::*;
use axum::{
    Router,
    response::{IntoResponse, Response},
    routing::{get, post},
};

async fn asset(Path(name): Path<String>) -> Response {
    let kind = match name.as_str() {
        "app.css" => "text/css",
        "app.js" | "theme-init.js" | "htmx-2.0.4.min.js" | "htmx-ext-sse-2.2.3.js" => {
            "text/javascript"
        }
        "busybots-logo-dark-hor.png" | "busybots-logo-light.png" => "image/png",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    match tokio::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(name),
    )
    .await
    {
        Ok(bytes) => ([(axum::http::header::CONTENT_TYPE, kind)], bytes).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn router(s: &Escalating, done: Arc<tokio::sync::Notify>) -> Router {
    let user = s.f.owner;
    Router::new()
        .route("/assets/{name}", get(asset))
        .route(
            "/ui/workflow-decisions/escalations/{decision}",
            get(move |state: State<Engine>, path: Path<Uuid>| async move {
                escalations::page(state, by(user), path).await
            }),
        )
        .route(
            "/ui/workflow-decisions/escalations/{decision}/submit",
            post(
                move |state: State<Engine>,
                      path: Path<Uuid>,
                      form: Form<escalations::SubmitForm>| async move {
                    escalations::ui_submit(state, by(user), path, form).await
                },
            ),
        )
        .route(
            "/__fixture/finish",
            post(move || {
                let done = done.clone();
                async move {
                    done.notify_one();
                    StatusCode::OK
                }
            }),
        )
        .layer(axum::extract::DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(s.f.engine.clone())
}

#[tokio::test]
#[ignore = "manual browser journey; isolated fixture server with a three minute deadline"]
async fn workflow_escalation_browser_actual_submissions() {
    let s = Box::pin(Escalating::new()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let file = std::env::var("OPENBOT_ESCALATION_BROWSER_FIXTURE").expect("fixture artifact path");
    let url = format!(
        "http://{address}/ui/workflow-decisions/escalations/{}",
        s.decision
    );
    std::fs::write(
        file,
        serde_json::to_vec(
            &json!({"url":url,"finish_url":format!("http://{address}/__fixture/finish")}),
        )
        .unwrap(),
    )
    .unwrap();
    let done = Arc::new(tokio::sync::Notify::new());
    let app = router(&s, done.clone());
    axum::serve(listener,app).with_graceful_shutdown(async move {
        tokio::select! { _=done.notified()=>{}, _=tokio::time::sleep(Duration::from_secs(180))=>{} }
    }).await.unwrap();
    let result:Value=sqlx::query_scalar("SELECT jsonb_build_object('commands',count(*),'reassign',count(*) FILTER (WHERE settlement='reassigned'), \
        'ack',count(*) FILTER (WHERE settlement='acknowledged')) FROM workflow_escalation_commands WHERE company_id=$1")
        .bind(s.f.company).fetch_one(s.f.persistence.pool()).await.unwrap();
    assert_eq!(
        result,
        json!({"commands":2,"reassign":1,"ack":1}),
        "the browser must submit both real commands"
    );
}
