use axum::{
    Form, Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{Notify, Semaphore};
#[derive(Clone)]
pub(crate) enum Reply {
    Json(Value),
    Raw(String),
    Lost,
}
pub(crate) struct Observation {
    pub grant: String,
    pub client_valid: bool,
    pub refresh_digest: [u8; 32],
    pub client_identity: Option<crate::domain::integrations::ExternalSubject>,
    pub code_digest: Option<[u8; 32]>,
    pub verifier_digest: Option<[u8; 32]>,
}
pub(crate) struct StateData {
    pub calls: AtomicUsize,
    pub identities: AtomicUsize,
    pub reply: Mutex<Reply>,
    pub status: Mutex<StatusCode>,
    pub gate: Arc<Semaphore>,
    pub observation: Mutex<Option<Observation>>,
    entered: Notify,
}
pub(crate) struct TokenServer {
    pub state: Arc<StateData>,
    pub url: String,
    worker: tokio::task::JoinHandle<()>,
}
impl Drop for TokenServer {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl TokenServer {
    pub async fn new() -> Self {
        let state = Arc::new(StateData {
            calls: AtomicUsize::new(0),
            identities: AtomicUsize::new(0),
            reply: Mutex::new(Reply::Json(
                json!({"access_token":"renewed-access-sentinel","expires_in":3600,"token_type":"Bearer","scope":"openid email https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.send"}),
            )),
            status: Mutex::new(StatusCode::OK),
            gate: Arc::new(Semaphore::new(1)),
            observation: Mutex::new(None),
            entered: Notify::new(),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/token", post(token))
            .route("/identity", get(identity))
            .with_state(state.clone());
        let worker = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { state, url, worker }
    }
    pub async fn wait_calls(&self, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let notified = self.state.entered.notified();
                if self.calls() >= expected {
                    break;
                }
                notified.await;
            }
        })
        .await
        .unwrap();
    }
    pub fn calls(&self) -> usize {
        self.state.calls.load(Ordering::SeqCst)
    }
    pub fn google_entry(&self) -> crate::application::integrations::registry::Registration {
        crate::adapters::integrations::google_authorization::registration(
            crate::domain::integrations::ExternalSubject::parse("google-client-fixture").unwrap(),
            secrecy::SecretString::from("client-secret-sentinel"),
            url::Url::parse("http://localhost/auth/integrations/google/callback").unwrap(),
            Arc::new(
                crate::adapters::integrations::google_http::GoogleHttp::scripted(
                    self.url.clone(),
                    self.url.replace("/token", "/identity"),
                ),
            ),
        )
        .unwrap()
    }
}
async fn token(
    State(state): State<Arc<StateData>>,
    Form(form): Form<BTreeMap<String, String>>,
) -> Response {
    *state.observation.lock().unwrap() = Some(Observation {
        client_identity: form
            .get("client_id")
            .and_then(|value| crate::domain::integrations::ExternalSubject::parse(value).ok()),
        code_digest: form
            .get("code")
            .map(|value| Sha256::digest(value.as_bytes()).into()),
        verifier_digest: form
            .get("code_verifier")
            .map(|value| Sha256::digest(value.as_bytes()).into()),
        grant: form.get("grant_type").cloned().unwrap_or_default(),
        client_valid: form
            .get("client_id")
            .is_some_and(|s| s == "google-client-fixture" || s == "strict-client")
            && form
                .get("client_secret")
                .is_none_or(|s| s == "client-secret-sentinel"),
        refresh_digest: Sha256::digest(
            form.get("refresh_token")
                .map(String::as_bytes)
                .unwrap_or_default(),
        )
        .into(),
    });
    state.calls.fetch_add(1, Ordering::SeqCst);
    state.entered.notify_one();
    let held = state.gate.acquire().await.unwrap();
    drop(held);
    let status = *state.status.lock().unwrap();
    let reply = state.reply.lock().unwrap().clone();
    match reply {
        Reply::Json(value) => (status, Json(value)).into_response(),
        Reply::Raw(value) => {
            (status, [("Content-Type", "application/json")], value).into_response()
        }
        Reply::Lost => Response::new(axum::body::Body::from_stream(futures::stream::once(
            async {
                Err::<String, _>(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "scripted response lost",
                ))
            },
        ))),
    }
}

async fn identity(State(state): State<Arc<StateData>>) -> Json<Value> {
    state.identities.fetch_add(1, Ordering::SeqCst);
    Json(json!({"sub":"google-private","email":"vault@gmail.com","email_verified":true}))
}
