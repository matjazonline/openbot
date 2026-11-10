//! Counted loopback protocol peer; no live provider or production endpoint bypass.
use crate::entities::mcp::McpDiscoveredTool;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Counts {
    pub initialize: usize,
    pub initialized: usize,
    pub catalogs: usize,
    pub calls: usize,
    pub deletes: usize,
}
pub(crate) struct Stage {
    entered: std::sync::atomic::AtomicUsize,
    started: Notify,
    release: Semaphore,
}
impl Stage {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Default::default(),
            started: Notify::new(),
            release: Semaphore::new(0),
        })
    }
    pub fn arrive(&self) {
        self.entered
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.started.notify_waiters();
    }
    pub async fn wait(&self) {
        self.arrive();
        self.release.acquire().await.unwrap().forget();
    }
    pub async fn observed(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let ready = self.started.notified();
                if self.entered.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                    return;
                }
                ready.await;
            }
        })
        .await
        .unwrap();
    }
    pub fn release(&self) {
        self.release.add_permits(64);
    }
}
#[derive(Clone)]
struct Peer {
    counts: Arc<Mutex<Counts>>,
    hold: Option<Arc<Stage>>,
    name: String,
    sse: bool,
    tools: Vec<McpDiscoveredTool>,
    result: Value,
    expected: Option<SecretString>,
}
pub(crate) struct Server {
    pub url: String,
    counts: Arc<Mutex<Counts>>,
    hold: Option<Arc<Stage>>,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Server {
    pub async fn new(hold: Option<Arc<Stage>>) -> Self {
        // Keep the existing fixture entry point small on stock test-thread stacks.
        Box::pin(Self::configured(
            hold,
            "counted-fixture".into(),
            false,
            vec![],
            json!({"content":[],"isError":false}),
            None,
        ))
        .await
    }
    pub async fn with_server_info(name: &str, sse: bool) -> Self {
        Box::pin(Self::configured(
            None,
            name.into(),
            sse,
            vec![],
            json!({"content":[],"isError":false}),
            None,
        ))
        .await
    }
    pub async fn business(
        expected: SecretString,
        tools: Vec<McpDiscoveredTool>,
        result: Value,
    ) -> Self {
        Box::pin(Self::configured(
            None,
            "counted-business".into(),
            false,
            tools,
            result,
            Some(expected),
        ))
        .await
    }
    async fn configured(
        hold: Option<Arc<Stage>>,
        name: String,
        sse: bool,
        tools: Vec<McpDiscoveredTool>,
        result: Value,
        expected: Option<SecretString>,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let counts = Arc::new(Mutex::new(Counts::default()));
        let stop = CancellationToken::new();
        let token = stop.clone();
        let router = Router::new()
            .route("/mcp", post(message).delete(delete))
            .with_state(Peer {
                counts: counts.clone(),
                hold: hold.clone(),
                name,
                sse,
                tools,
                result,
                expected,
            });
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(token.cancelled_owned())
                .await
                .unwrap();
        });
        Self {
            url,
            counts,
            hold,
            stop,
            task: Some(task),
        }
    }
    pub fn counts(&self) -> Counts {
        *self.counts.lock().unwrap()
    }
    pub async fn close(mut self) {
        if let Some(stage) = &self.hold {
            stage.release();
        }
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            tokio::time::timeout(std::time::Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stage) = &self.hold {
            stage.release();
        }
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
async fn message(
    State(peer): State<Peer>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    // Deliberately never retain or print authorization values.
    if headers.get("authorization").is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(expected) = &peer.expected
        && headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            != Some(expected.expose_secret())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let result = match body["method"].as_str().unwrap_or("") {
        "initialize" => {
            peer.counts.lock().unwrap().initialize += 1;
            if let Some(stage) = &peer.hold {
                stage.wait().await;
            }
            json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":peer.name,"version":"1"}})
        }
        "notifications/initialized" => {
            peer.counts.lock().unwrap().initialized += 1;
            return StatusCode::ACCEPTED.into_response();
        }
        "notifications/cancelled" => return StatusCode::ACCEPTED.into_response(),
        "tools/list" => {
            peer.counts.lock().unwrap().catalogs += 1;
            let tools=peer.tools.iter().map(|tool|json!({"name":tool.name,"description":tool.description,"inputSchema":tool.input_schema,"outputSchema":tool.output_schema})).collect::<Vec<_>>();
            json!({"tools":tools})
        }
        "tools/call" => {
            if !peer.tools.is_empty()
                && !peer
                    .tools
                    .iter()
                    .any(|tool| body["params"]["name"].as_str() == Some(tool.name.as_str()))
            {
                return StatusCode::BAD_REQUEST.into_response();
            }
            peer.counts.lock().unwrap().calls += 1;
            peer.result.clone()
        }
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let envelope = json!({"jsonrpc":"2.0","id":body["id"],"result":result});
    if peer.sse {
        return (
            [
                ("mcp-session-id", "fixture-session"),
                ("content-type", "text/event-stream"),
            ],
            format!("event: message\ndata: {envelope}\n\n"),
        )
            .into_response();
    }
    ([("mcp-session-id", "fixture-session")], Json(envelope)).into_response()
}
async fn delete(State(peer): State<Peer>) -> StatusCode {
    peer.counts.lock().unwrap().deletes += 1;
    StatusCode::NO_CONTENT
}
