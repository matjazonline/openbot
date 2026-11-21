use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Extension, Request, State},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tower_http::limit::RequestBodyLimitLayer;
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    adapters::postgres,
    infra::config::{MAX_BODY_BYTES, MAX_REQUESTS},
};

#[derive(Clone)]
pub struct HttpState {
    pub pool: PgPool,
    pub accepting: Arc<AtomicBool>,
    admission: Arc<Semaphore>,
    secure_origin: bool,
    identity: Option<super::identity::http::IdentityHttp>,
    setup: Option<super::setup::http::SetupHttp>,
    commerce: Option<super::commerce::http::CommerceHttp>,
}

impl HttpState {
    pub(crate) fn with_commerce(mut self, commerce: super::commerce::http::CommerceHttp) -> Self {
        self.commerce = Some(commerce);
        self
    }
    pub(crate) fn with_setup(mut self, setup: super::setup::http::SetupHttp) -> Self {
        self.setup = Some(setup);
        self
    }
    pub(crate) fn with_identity(mut self, identity: super::identity::http::IdentityHttp) -> Self {
        self.identity = Some(identity);
        self
    }

    pub fn new(pool: PgPool, accepting: Arc<AtomicBool>, secure_origin: bool) -> Self {
        Self {
            pool,
            accepting,
            admission: Arc::new(Semaphore::new(MAX_REQUESTS)),
            secure_origin,
            identity: None,
            setup: None,
            commerce: None,
        }
    }
}

pub fn router(state: HttpState) -> Router {
    let identity = state.identity.clone();
    let setup = state.setup.clone();
    let commerce = state.commerce.clone();
    let routes = Router::new()
        .route("/healthz", get(|| async { StatusCode::NO_CONTENT }))
        .route("/readyz", get(readiness))
        .route("/assets/app.css", get(stylesheet))
        .fallback(
            |Extension(correlation): Extension<RequestCorrelation>| async move {
                problem(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "Resource not found",
                    correlation.0,
                )
            },
        );
    let routes = match identity {
        Some(identity) => routes.merge(super::identity::http::router(identity)),
        None => routes,
    };
    let routes = match setup {
        Some(setup) => routes
            .merge(super::catalog::http::router(setup.clone()))
            .merge(super::analytics::http::router(setup.clone()))
            .merge(super::website::http::router(setup.clone()))
            .merge(super::setup::http::router(setup)),
        None => routes,
    };
    let routes = match commerce {
        Some(commerce) => routes.merge(super::commerce::http::router(commerce)),
        None => routes,
    };
    with_boundary(routes, state)
}

fn with_boundary(router: Router<HttpState>, state: HttpState) -> Router {
    router
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(state.clone(), boundary))
        .with_state(state)
}

async fn readiness(State(state): State<HttpState>) -> StatusCode {
    if state.accepting.load(Ordering::Acquire) && postgres::ready(&state.pool).await {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn stylesheet() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../../assets/app.css"),
    )
}

#[derive(Clone, Copy)]
struct RequestCorrelation(Uuid);

async fn boundary(State(state): State<HttpState>, mut request: Request, next: Next) -> Response {
    let correlation = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::try_parse(value).ok())
        .unwrap_or_else(Uuid::new_v4);
    request
        .extensions_mut()
        .insert(RequestCorrelation(correlation));
    let operation = match request.uri().path() {
        p if p.starts_with("/r/") => "redirect",
        "/v1/conversions" => "merchant_ingestion",
        "/v1/checkouts" => "checkout",
        "/v1/events" | "/pixel.gif" => "observation",
        p if p.starts_with("/v1/analytics") || p.starts_with("/reports") => "report",
        p if p.starts_with("/auth/") || p.starts_with("/v1/session") => "identity",
        "/healthz" | "/readyz" => "health",
        _ => "management",
    };
    let started = std::time::Instant::now();
    let health = request.uri().path() == "/healthz";
    let permit = if health {
        None
    } else {
        match state.admission.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                tracing::warn!(operation, request_id=%correlation, status=429, outcome="admission_rejected");
                return secure(
                    problem(
                        StatusCode::TOO_MANY_REQUESTS,
                        "busy",
                        "Retry later",
                        correlation,
                    ),
                    correlation,
                    state.secure_origin,
                );
            }
        }
    };
    let span = tracing::info_span!("attribution_request", request_id=%correlation, operation);
    let response =
        match tokio::time::timeout(Duration::from_secs(10), next.run(request).instrument(span))
            .await
        {
            Ok(response) => response,
            Err(_) => problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "timeout",
                "Retry later",
                correlation,
            ),
        };
    drop(permit);
    // Deliberately no URL, query, body, credentials or peer address in traces.
    tracing::info!(operation, request_id = %correlation, status = response.status().as_u16(), elapsed_ms = started.elapsed().as_millis(), "request completed");
    secure(
        normalize_error(response, correlation),
        correlation,
        state.secure_origin,
    )
}

fn normalize_error(mut response: Response, correlation: Uuid) -> Response {
    if response
        .extensions_mut()
        .remove::<super::html::SafeError>()
        .is_some()
    {
        return response;
    }
    if !(response.status().is_client_error() || response.status().is_server_error())
        || response
            .headers()
            .get("content-type")
            .is_some_and(|v| v.as_bytes().starts_with(b"application/json"))
    {
        return response;
    }
    let code = match response.status() {
        StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
        StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
        StatusCode::SERVICE_UNAVAILABLE => "unavailable",
        _ => "request_failed",
    };
    let bytes = serde_json::to_vec(&Problem {
        code,
        message: "Request could not be completed",
        correlation_id: correlation,
    })
    .expect("bounded error serialization");
    *response.body_mut() = Body::from(bytes);
    response.headers_mut().remove("content-length");
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    response
}

fn secure(mut response: Response, correlation: Uuid, https: bool) -> Response {
    let headers = response.headers_mut();
    for (name, value) in [
        ("cache-control", "no-store"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    if https {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    headers.insert(
        "x-request-id",
        HeaderValue::from_str(&correlation.to_string()).expect("UUID header"),
    );
    if matches!(
        response.status(),
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
    ) {
        response
            .headers_mut()
            .insert("retry-after", HeaderValue::from_static("1"));
    }
    response
}

#[derive(Serialize)]
struct Problem {
    code: &'static str,
    message: &'static str,
    correlation_id: Uuid,
}

pub fn problem(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    correlation_id: Uuid,
) -> Response<Body> {
    (
        status,
        Json(Problem {
            code,
            message,
            correlation_id,
        }),
    )
        .into_response()
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
