//! Real HTTP router and PostgreSQL source authority; no remote effect fixture.
use super::*;
use crate::adapters::http::routes::local_machine_connection::{LocalMachineConnections, router};
use crate::application::workflow::WorkflowInspection;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;

struct Http {
    f: McpSetup,
    service: Arc<LocalMachineConnections>,
    app: axum::Router,
}

impl Http {
    async fn new() -> Self {
        let f = Box::pin(McpSetup::new()).await;
        let service = Arc::new(LocalMachineConnections::new(f.s.f.persistence.clone()));
        let app = router::<Arc<LocalMachineConnections>>().with_state(service.clone());
        Self { f, service, app }
    }

    async fn post(&self, token: &str, fence: ConnectionFence, body: Body) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::post(format!(
                    "/v1/local-machines/operations/{}/mcp",
                    self.f.operation,
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("x-openbot-protocol-version", "2")
                .header(
                    "x-openbot-installation-id",
                    fence.machine.installation_id.to_string(),
                )
                .header(
                    "x-openbot-credential-generation",
                    fence.machine.credential_generation.get(),
                )
                .header("x-openbot-connection-generation", fence.generation.get())
                .body(body)
                .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn own(&self, body: Vec<u8>) -> Response {
        self.post(
            self.f.s.paired.candidate.credential.expose_secret(),
            self.f.fence,
            Body::from(body),
        )
        .await
    }

    async fn empty(&self) {
        let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM native_mcp_calls),(SELECT count(*) FROM workflow_runs),(SELECT count(*) FROM workflow_admissions)")
            .fetch_one(&self.f.s.f.database.pool).await.unwrap();
        assert_eq!(counts, (0, 0, 0));
    }
}

fn observed_body(reads: Arc<AtomicUsize>) -> Body {
    Body::from_stream(futures::stream::once(async move {
        reads.fetch_add(1, Ordering::SeqCst);
        Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(b"not-json"))
    }))
}

#[tokio::test]
async fn native_mcp_http_authenticates_before_body_and_refuses_foreign_or_stale_connections() {
    let h = Box::pin(Http::new()).await;
    let reads = Arc::new(AtomicUsize::new(0));
    let response = h
        .post(&"0".repeat(64), h.f.fence, observed_body(reads.clone()))
        .await;
    assert!(!response.status().is_success());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let foreign = h.f.s.f.pair(h.f.s.f.foreign).await;
    let fence =
        h.f.s
            .f
            .persistence
            .acquire_connection(
                foreign.candidate.credential.verifier(),
                foreign.receipt.machine.installation_id,
                Uuid::new_v4(),
            )
            .await
            .unwrap()
            .unwrap();
    let response = h
        .post(
            foreign.candidate.credential.expose_secret(),
            fence,
            observed_body(reads.clone()),
        )
        .await;
    assert!(!response.status().is_success());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    h.f.s
        .f
        .persistence
        .acquire_connection(
            h.f.s.paired.candidate.credential.verifier(),
            h.f.fence.machine.installation_id,
            Uuid::new_v4(),
        )
        .await
        .unwrap()
        .unwrap();
    let response = h
        .post(
            h.f.s.paired.candidate.credential.expose_secret(),
            h.f.fence,
            observed_body(reads.clone()),
        )
        .await;
    assert!(!response.status().is_success());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    h.empty().await;
    h.service.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_mcp_http_bounds_untrusted_input_then_publishes_one_visible_no_store_run() {
    let h = Box::pin(Http::new()).await;
    let body = json!({
        "invocation":h.f.request(json!({})).body.invocation,
        "action":{"kind":"call","alias":"crm.lookup","arguments":{"query":"http exact"}},
    });
    let mut injected = body.clone();
    injected["operation_id"] = json!(Uuid::new_v4());
    for bytes in [
        b"not-json".to_vec(),
        vec![b' '; MAX_REQUEST_BYTES + 1],
        serde_json::to_vec(&injected).unwrap(),
    ] {
        let response = h.own(bytes).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            !response
                .headers()
                .contains_key("x-openbot-mcp-not-admitted")
        );
    }
    let mut invalid = body.clone();
    invalid["action"]["arguments"] = json!({});
    let rejected = h.own(serde_json::to_vec(&invalid).unwrap()).await;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        rejected.headers()["x-openbot-mcp-not-admitted"],
        "invalid_arguments"
    );
    h.empty().await;
    let response = h.own(serde_json::to_vec(&body).unwrap()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let bytes = axum::body::to_bytes(response.into_body(), MAX_RESPONSE_BYTES)
        .await
        .unwrap();
    let admitted: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(admitted["kind"], "pending");
    let run = crate::domain::workflow::RunId::new(
        Uuid::parse_str(admitted["run_id"].as_str().unwrap()).unwrap(),
    );
    let head =
        h.f.s
            .f
            .persistence
            .head(CompanyId::new(h.f.s.f.company), run)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(head.knowledge.content().as_i16(), 9);
    let replay = h.own(serde_json::to_vec(&body).unwrap()).await;
    assert_eq!(replay.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(replay.into_body(), MAX_RESPONSE_BYTES)
        .await
        .unwrap();
    let replay: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        (&replay["call_id"], &replay["run_id"]),
        (&admitted["call_id"], &admitted["run_id"])
    );
    // A prior call, even with different input, must never acquire the empty-history proof.
    let conflict = h.own(serde_json::to_vec(&invalid).unwrap()).await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert!(
        !conflict
            .headers()
            .contains_key("x-openbot-mcp-not-admitted")
    );
    h.service.shutdown().await.unwrap();
}
