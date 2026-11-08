//! Owned, counted business endpoint. HTTPS metadata is mapped only inside this test fixture;
//! the production guarded HTTP client, injection, cancellation and response path are reused.
use crate::{
    adapters::{
        guarded_http::HttpDestinationPolicy, integrations::execution::IntegrationTransports,
        mcp::preparation_test_server::Stage,
    },
    app_error::{AppError, AppResult},
    application::{
        integrations::{
            execute::AuthenticatedTransports, private::TypedInjection, renewal::ResolutionResult, *,
        },
        services::mcp_client::*,
        workflow::http::HttpOrigin,
    },
    domain::integrations::{authority::ApprovedDestination, *},
    entities::mcp::McpEndpoint,
};
use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;

pub(crate) enum CredentialExpectation {
    Bearer(SecretString),
    ApiKey(SecretString),
}
struct PeerState {
    expected: CredentialExpectation,
    calls: AtomicUsize,
    bodies: Mutex<Vec<[u8; 32]>>,
    requests: Mutex<Vec<(Method, Uri)>>,
    reply: Value,
    hold: Option<Arc<Stage>>,
}
pub(crate) struct BusinessPeer {
    pub origin: HttpOrigin,
    state: Arc<PeerState>,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl BusinessPeer {
    pub async fn new(
        expected: CredentialExpectation,
        reply: Value,
        hold: Option<Arc<Stage>>,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin =
            HttpOrigin::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let state = Arc::new(PeerState {
            expected,
            calls: AtomicUsize::new(0),
            bodies: Mutex::new(vec![]),
            requests: Mutex::new(vec![]),
            reply,
            hold,
        });
        let app = Router::new()
            .route("/", any(business))
            .route("/{*path}", any(business))
            .with_state(state.clone());
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(cancellation.cancelled_owned())
                .await
                .unwrap();
        });
        Self {
            origin,
            state,
            stop,
            task: Some(task),
        }
    }
    pub fn calls(&self) -> usize {
        self.state.calls.load(Ordering::SeqCst)
    }
    pub fn body_digests(&self) -> Vec<[u8; 32]> {
        self.state.bodies.lock().unwrap().clone()
    }
    pub fn requests(&self) -> Vec<(Method, Uri)> {
        self.state.requests.lock().unwrap().clone()
    }
    pub fn transports(&self, metadata: ApprovedDestination) -> Arc<dyn AuthenticatedTransports> {
        Arc::new(Mapped {
            metadata,
            origin: self.origin.clone(),
            native: IntegrationTransports {
                http: HttpDestinationPolicy::new(vec![self.origin.as_str().to_owned()]).unwrap(),
                mcp: Arc::new(NoMcp),
            },
        })
    }
    pub async fn close(mut self) {
        if let Some(hold) = &self.state.hold {
            hold.release();
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
impl Drop for BusinessPeer {
    fn drop(&mut self) {
        if let Some(hold) = &self.state.hold {
            hold.release();
        }
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
async fn business(
    State(state): State<Arc<PeerState>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    body: axum::body::Bytes,
) -> Response {
    state.requests.lock().unwrap().push((method, uri));
    let valid = match &state.expected {
        CredentialExpectation::Bearer(secret) => {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|value| value.strip_prefix("Bearer ") == Some(secret.expose_secret()))
                && headers.get("x-api-key").is_none()
        }
        CredentialExpectation::ApiKey(secret) => {
            headers.get("x-api-key").and_then(|v| v.to_str().ok()) == Some(secret.expose_secret())
                && headers.get("authorization").is_none()
        }
    };
    if !valid {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state.calls.fetch_add(1, Ordering::SeqCst);
    state
        .bodies
        .lock()
        .unwrap()
        .push(Sha256::digest(&body).into());
    if let Some(hold) = &state.hold {
        hold.wait().await;
    }
    Json(state.reply.clone()).into_response()
}

struct Mapped {
    native: IntegrationTransports,
    metadata: ApprovedDestination,
    origin: HttpOrigin,
}
#[async_trait]
impl AuthenticatedTransports for Mapped {
    fn http_url(&self, i: &AuthorizedInvocation) -> IntegrationResult<url::Url> {
        if i.authorization.destination != self.metadata {
            return Err(IntegrationError::Denied);
        }
        self.native.http_url(i)
    }
    async fn prepare_http(
        &self,
        i: &AuthorizedInvocation,
        c: &ExecutionContext,
        injection: TypedInjection,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
        self.http_url(i)?;
        Box::pin(
            self.native
                .prepare_http_loopback(i, c, injection, self.origin.clone()),
        )
        .await
    }
    fn mcp_destination(&self, _: &ApprovedDestination) -> IntegrationResult<()> {
        Err(IntegrationError::Denied)
    }
    async fn prepare_mcp(
        &self,
        _: &AuthorizedInvocation,
        _: &ExecutionContext,
        _: TypedInjection,
        _: McpPreparation,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
        Err(IntegrationError::Denied.into())
    }
    async fn discover_mcp(
        &self,
        _: &OwnerDiscoveryContext,
        _: TypedInjection,
        _: McpPreparation,
    ) -> ResolutionResult<Box<dyn discovery::PreparedCatalog>> {
        Err(IntegrationError::Denied.into())
    }
}
struct NoMcp;
#[async_trait]
impl McpClient for NoMcp {
    fn validate_endpoint(&self, _: &McpEndpoint) -> AppResult<()> {
        Err(AppError::Forbidden(
            "No MCP endpoint in HTTP fixture".into(),
        ))
    }
    async fn connect(
        &self,
        _: &McpEndpoint,
        _: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>> {
        Err(AppError::Forbidden(
            "No MCP connection in HTTP fixture".into(),
        ))
    }
    async fn connect_prepared(
        &self,
        _: &McpEndpoint,
        _: SecretString,
        _: McpPreparation,
    ) -> McpPreparationResult<Box<dyn McpSession>> {
        Err(McpPreparationFailure::Denied)
    }
    async fn shutdown(&self) -> AppResult<()> {
        Ok(())
    }
}

pub(crate) fn with_mcp(
    http: Arc<dyn AuthenticatedTransports>,
    client: Arc<dyn McpClient>,
) -> Arc<dyn AuthenticatedTransports> {
    Arc::new(Combined {
        http,
        mcp: IntegrationTransports {
            http: HttpDestinationPolicy::default(),
            mcp: client,
        },
    })
}
struct Combined {
    http: Arc<dyn AuthenticatedTransports>,
    mcp: IntegrationTransports,
}
#[async_trait]
impl AuthenticatedTransports for Combined {
    fn http_url(&self, i: &AuthorizedInvocation) -> IntegrationResult<url::Url> {
        self.http.http_url(i)
    }
    async fn prepare_http(
        &self,
        i: &AuthorizedInvocation,
        c: &ExecutionContext,
        injection: TypedInjection,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
        self.http.prepare_http(i, c, injection).await
    }
    fn mcp_destination(&self, d: &ApprovedDestination) -> IntegrationResult<()> {
        self.mcp.mcp_destination(d)
    }
    async fn prepare_mcp(
        &self,
        i: &AuthorizedInvocation,
        c: &ExecutionContext,
        injection: TypedInjection,
        work: McpPreparation,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
        self.mcp.prepare_mcp(i, c, injection, work).await
    }
    async fn discover_mcp(
        &self,
        c: &OwnerDiscoveryContext,
        injection: TypedInjection,
        work: McpPreparation,
    ) -> ResolutionResult<Box<dyn discovery::PreparedCatalog>> {
        self.mcp.discover_mcp(c, injection, work).await
    }
}
