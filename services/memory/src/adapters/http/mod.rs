//! Axum transport: routes generated from the operation registry, bearer authentication,
//! body/concurrency/deadline bounds, health and readiness. MCP is mounted at `/mcp` behind the
//! same authentication, concurrency bound, body bound and deadline.
pub mod openapi;

use crate::{
    adapters::mcp::McpHandler,
    application::{
        metrics::{GaugeRow, Gauges, LAST_MAINTENANCE_UNIX, METRICS},
        operations::{BodyLimit, Method, OPERATIONS, OperationSpec, Surface},
        ports::Actor,
        service::{App, Principal, RequestIds},
    },
    domain::{
        admin_policy::operator_only,
        correlation::CorrelationId,
        error::{ErrorCode, ServiceError},
    },
};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct HttpSettings {
    pub max_in_flight: usize,
    pub request_deadline: Duration,
    pub mcp_allowed_hosts: Vec<String>,
    pub mcp_allowed_origins: Vec<String>,
}

#[derive(Clone)]
pub struct HttpState {
    app: App,
    ready: Arc<AtomicBool>,
    permits: Arc<Semaphore>,
    deadline: Duration,
}

pub fn error_response(error: &ServiceError, request_id: Uuid) -> Response {
    let status =
        StatusCode::from_u16(error.code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    with_request_id(
        (status, Json(error.body(request_id.to_string()))).into_response(),
        request_id,
    )
}

/// Every answer carries the service-minted `x-request-id` (06A A2-2), success included.
pub fn with_request_id(mut response: Response, request_id: Uuid) -> Response {
    if let Ok(value) = header::HeaderValue::from_str(&request_id.to_string()) {
        response.headers_mut().insert(REQUEST_ID, value);
    }
    response
}

pub const REQUEST_ID: &str = "x-request-id";
pub const CORRELATION_ID: &str = "x-correlation-id";

/// The caller's optional `x-correlation-id` (06A A2-2): 1–128 bytes of `[A-Za-z0-9._:-]`. A
/// malformed one is `invalid_request`, never silently dropped.
pub fn correlation(headers: &HeaderMap) -> Result<Option<CorrelationId>, ServiceError> {
    let Some(value) = headers.get(CORRELATION_ID) else {
        return Ok(None);
    };
    value
        .to_str()
        .ok()
        .and_then(CorrelationId::parse)
        .map(Some)
        .ok_or_else(|| {
            ServiceError::invalid("x-correlation-id must be 1-128 bytes of [A-Za-z0-9._:-]")
        })
}

fn code_label(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

async fn read_json(request: Request, limit: usize) -> Result<Value, ServiceError> {
    let bytes = to_bytes(request.into_body(), limit).await.map_err(|_| {
        ServiceError::new(
            ErrorCode::PayloadTooLarge,
            format!("The request body exceeds {limit} bytes"),
        )
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ServiceError::invalid("The request body is not valid JSON"))
}

async fn handle(state: HttpState, spec: &'static OperationSpec, request: Request) -> Response {
    let request_id = Uuid::now_v7();
    let now = Utc::now();
    let correlation = correlation(request.headers());
    let correlation_label = correlation
        .as_ref()
        .ok()
        .copied()
        .flatten()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let run = async {
        let correlation = correlation?;
        let principal = state
            .app
            .authenticate(bearer(request.headers()), spec.surface, now)
            .await?;
        let input = match spec.method {
            Method::Get => json!({}),
            Method::Post => read_json(request, spec.body_limit.bytes()).await?,
        };
        let ids = RequestIds {
            request: request_id,
            correlation,
        };
        state.app.execute(spec, &principal, input, now, ids).await
    };
    let outcome = match tokio::time::timeout(state.deadline, run).await {
        Ok(result) => result,
        Err(_) => Err(deadline_exceeded()),
    };
    let code = match &outcome {
        Ok(_) => "ok".to_owned(),
        Err(error) => code_label(error.code),
    };
    METRICS.inc(
        "memory_requests_total",
        &[("operation", spec.id), ("code", &code)],
        1,
    );
    // Ids and codes only: never a body, a query, memory text or a token (06A A2-3).
    tracing::info!(operation = spec.id, code = %code, %request_id,
        correlation_id = %correlation_label, "request");
    match outcome {
        Ok(value) => with_request_id((StatusCode::OK, Json(value)).into_response(), request_id),
        Err(error) => error_response(&error, request_id),
    }
}

/// `GET /admin/v1/metrics` (06A A2-1): Prometheus text for an operator credential only;
/// provisioners get 403 and clients 401. Outside the operation registry (it is not JSON and has
/// no MCP tool).
async fn metrics(State(state): State<HttpState>, request: Request) -> Response {
    let request_id = Uuid::now_v7();
    let principal = state
        .app
        .authenticate(bearer(request.headers()), Surface::Admin, Utc::now())
        .await;
    let operator = match principal {
        Ok(Principal::Admin(Actor { scope, .. })) if operator_only(scope) => true,
        Ok(_) => false,
        Err(error) => return error_response(&error, request_id),
    };
    if !operator {
        return error_response(&ServiceError::forbidden(), request_id);
    }
    let ready = state.ready.load(Ordering::Acquire)
        && tokio::time::timeout(Duration::from_secs(1), state.app.store().ping())
            .await
            .is_ok_and(|result| result.is_ok());
    let gauges = tokio::time::timeout(Duration::from_secs(3), state.app.store().metrics_gauges())
        .await
        .ok()
        .and_then(Result::ok);
    let last = LAST_MAINTENANCE_UNIX.load(Ordering::Relaxed);
    let since = if last == 0 {
        -1.0
    } else {
        (Utc::now().timestamp() - last) as f64
    };
    let g = gauges.unwrap_or_default();
    let mut rows: Vec<GaugeRow<'_>> = vec![
        (
            "memory_ready",
            "1 when the service is ready",
            vec![],
            f64::from(u8::from(ready)),
        ),
        (
            "memory_metrics_query_ok",
            "1 when the database gauges below were read",
            vec![],
            f64::from(u8::from(gauges.is_some())),
        ),
        (
            "memory_maintenance_seconds_since_pass",
            "Seconds since this process's last maintenance pass (-1 before the first)",
            vec![],
            since,
        ),
        (
            "memory_provider_auth_breaker_open",
            "1 while this process has paused generation because the provider refused its credentials",
            vec![],
            f64::from(u8::from(state.app.generation_paused())),
        ),
    ];
    if gauges.is_some() {
        let Gauges {
            accepted,
            dispatched,
            oldest_accepted_seconds,
            unknown_last_day,
            tenants_waiting,
            max_tenant_queue,
            index_pending,
            oldest_index_pending_seconds,
            index_failed,
        } = g;
        let operations = "Generation operations by state";
        rows.extend([
            (
                "memory_generation_operations",
                operations,
                vec![("state", "accepted".into())],
                accepted as f64,
            ),
            (
                "memory_generation_operations",
                operations,
                vec![("state", "dispatched".into())],
                dispatched as f64,
            ),
            (
                "memory_generation_oldest_accepted_seconds",
                "Age of the oldest accepted operation",
                vec![],
                oldest_accepted_seconds,
            ),
            (
                "memory_generation_unknown_last_day",
                "Operations that ended unknown in the last 24 h",
                vec![],
                unknown_last_day as f64,
            ),
            (
                "memory_generation_tenants_waiting",
                "Tenants with accepted operations",
                vec![],
                tenants_waiting as f64,
            ),
            (
                "memory_generation_max_tenant_queue",
                "Most accepted operations queued by one tenant",
                vec![],
                max_tenant_queue as f64,
            ),
            (
                "memory_index_pending",
                "Observations waiting to be indexed",
                vec![],
                index_pending as f64,
            ),
            (
                "memory_index_oldest_pending_seconds",
                "Age of the oldest pending observation",
                vec![],
                oldest_index_pending_seconds,
            ),
            (
                "memory_index_failed",
                "Observations whose indexing failed",
                vec![],
                index_failed as f64,
            ),
        ]);
    }
    let body = METRICS.render(&rows);
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/plain; version=0.0.4"),
    );
    with_request_id(response, request_id)
}

pub fn deadline_exceeded() -> ServiceError {
    ServiceError::new(ErrorCode::Timeout, "The request exceeded its deadline")
}

/// The `/mcp` body bound: the largest tool argument object (the 4 MiB `Large` limit) plus room
/// for the JSON-RPC envelope around it. Each tool call is then held to its own operation's
/// limit (`McpHandler::call_tool`), as on `/v1`.
pub const MCP_BODY_LIMIT: usize = BodyLimit::Large.bytes() + 64 * 1024;

/// Bound concurrent work: beyond the permit count requests are refused, not queued.
async fn limit(State(state): State<HttpState>, request: Request, next: Next) -> Response {
    match state.permits.clone().try_acquire_owned() {
        Ok(_permit) => next.run(request).await,
        Err(_) => {
            METRICS.inc(
                "memory_requests_total",
                &[("operation", "unrouted"), ("code", "capacity_exceeded")],
                1,
            );
            let mut response = error_response(
                &ServiceError::new(ErrorCode::CapacityExceeded, "The service is at capacity"),
                Uuid::now_v7(),
            );
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, header::HeaderValue::from_static("1"));
            response
        }
    }
}

/// MCP requests authenticate exactly like `/v1`, and the principal travels in the request
/// parts. The body is read here under `MCP_BODY_LIMIT` (rmcp itself collects bodies without a
/// limit), and the whole exchange runs under the request deadline. rmcp runs the handler on a
/// task of its own, so `McpHandler` also bounds each tool call by the same deadline.
async fn mcp_gate(State(state): State<HttpState>, request: Request, next: Next) -> Response {
    let request_id = Uuid::now_v7();
    let run = async {
        let principal = state
            .app
            .authenticate(bearer(request.headers()), Surface::Client, Utc::now())
            .await?;
        let (mut parts, body) = request.into_parts();
        let bytes = to_bytes(body, MCP_BODY_LIMIT).await.map_err(|_| {
            ServiceError::new(
                ErrorCode::PayloadTooLarge,
                format!("The request body exceeds {MCP_BODY_LIMIT} bytes"),
            )
        })?;
        parts.extensions.insert::<Principal>(principal);
        Ok::<_, ServiceError>(
            next.run(Request::from_parts(parts, Body::from(bytes)))
                .await,
        )
    };
    match tokio::time::timeout(state.deadline, run).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => error_response(&error, request_id),
        Err(_) => error_response(&deadline_exceeded(), request_id),
    }
}

async fn healthz() -> Response {
    (StatusCode::OK, Json(json!({"status": "ok"}))).into_response()
}

async fn readyz(State(state): State<HttpState>) -> Response {
    let ready = state.ready.load(Ordering::Acquire)
        && tokio::time::timeout(Duration::from_secs(1), state.app.store().ping())
            .await
            .is_ok_and(|result| result.is_ok());
    if ready {
        (StatusCode::OK, Json(json!({"status": "ready"}))).into_response()
    } else {
        error_response(
            &ServiceError::unavailable("The service is not ready"),
            Uuid::now_v7(),
        )
    }
}

pub fn router(
    app: App,
    ready: Arc<AtomicBool>,
    settings: &HttpSettings,
    shutdown: CancellationToken,
) -> Router {
    let state = HttpState {
        app: app.clone(),
        ready,
        permits: Arc::new(Semaphore::new(settings.max_in_flight)),
        deadline: settings.request_deadline,
    };
    let mut api = Router::new();
    for spec in OPERATIONS {
        let handler = move |State(state): State<HttpState>, request: Request<Body>| {
            handle(state, spec, request)
        };
        api = match spec.method {
            Method::Get => api.route(spec.path, get(handler)),
            Method::Post => api.route(spec.path, post(handler)),
        };
    }
    let mut config = StreamableHttpServerConfig::default();
    config.stateful_mode = false;
    config.json_response = true;
    config.allowed_hosts = settings.mcp_allowed_hosts.clone();
    config.allowed_origins = settings.mcp_allowed_origins.clone();
    config.cancellation_token = shutdown.child_token();
    let deadline = settings.request_deadline;
    let mcp = StreamableHttpService::new(
        move || Ok(McpHandler::new(app.clone(), deadline)),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let mcp = Router::new()
        .nest_service("/mcp", mcp)
        .layer(middleware::from_fn_with_state(state.clone(), mcp_gate));
    let bounded = api
        .route("/admin/v1/metrics", get(metrics))
        .merge(mcp)
        .layer(middleware::from_fn_with_state(state.clone(), limit));
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .merge(bounded)
        .with_state(state)
}

#[cfg(test)]
#[path = "contract_tests.rs"]
mod contract_tests;
