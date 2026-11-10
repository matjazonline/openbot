//! DB-backed `memory_operations_` observability cases (06A.3; p06 gate A2): the operator-only
//! metrics export, request and correlation ids through HTTP, MCP, audit and the generation
//! worker, and logs that never carry content or secrets. Require MEMORY_TEST_DATABASE_URL.
use super::test_support::test_db;
use crate::{
    adapters::http::{HttpSettings, router},
    application::{
        generation_worker::{GenerationSettings, GenerationWorker},
        operations::Surface,
        ports::GenerationProvider,
        service::{App, Principal},
        test_provider::{CountedProvider, Step, answer},
    },
    domain::credential::{self, TokenKind},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::{Duration as Days, Utc};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use uuid::Uuid;

/// A marker placed in every text field: it must never reach a log line.
const MARKER: &str = "ZEBRA-MARKER-7f3a";

const USEFUL: &str = r#"{"outcome":"save","observations":[
    {"kind":"fact","title":"ZEBRA-MARKER-7f3a title","body":"ZEBRA-MARKER-7f3a body","event_ids":["e1"]}],"summary":null}"#;

struct Fixture {
    app: App,
    db: super::test_support::TestDb,
    provider: Arc<CountedProvider>,
    operator_token: String,
    operator: Principal,
}

async fn fixture() -> Fixture {
    let db = test_db().await;
    let provider = Arc::new(CountedProvider::new(vec![]).then_always(Step::Answer(answer(USEFUL))));
    let app = App::new(
        Arc::new(db.store.clone()),
        crate::application::test_embedder::fake_pool(),
    )
    .with_generation(provider.clone());
    let issued = credential::issue(TokenKind::Admin).unwrap();
    app.store()
        .bootstrap_operator(issued.id, "operator", issued.verifier)
        .await
        .unwrap();
    let operator = app
        .authenticate(Some(&issued.token), Surface::Admin, Utc::now())
        .await
        .unwrap();
    Fixture {
        app,
        db,
        provider,
        operator_token: issued.token,
        operator,
    }
}

fn routes(f: &Fixture) -> axum::Router {
    router(
        f.app.clone(),
        Arc::new(AtomicBool::new(true)),
        &HttpSettings {
            max_in_flight: 64,
            request_deadline: Duration::from_secs(10),
            mcp_allowed_hosts: vec!["localhost".into()],
            mcp_allowed_origins: vec![],
        },
        CancellationToken::new(),
    )
}

struct Answer {
    status: StatusCode,
    request_id: Option<String>,
    body: String,
}

async fn send(
    router: &axum::Router,
    method: &str,
    path: &str,
    token: &str,
    correlation: Option<&str>,
    body: Option<Value>,
) -> Answer {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(correlation) = correlation {
        request = request.header("x-correlation-id", correlation);
    }
    let request = request
        .body(body.map_or(Body::empty(), |body| Body::from(body.to_string())))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = to_bytes(response.into_body(), 8 << 20).await.unwrap();
    Answer {
        status,
        request_id,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

impl Fixture {
    async fn admin(&self, path: &str, body: Value) -> Value {
        let answer = send(
            &routes(self),
            "POST",
            path,
            &self.operator_token,
            None,
            Some(body),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
        serde_json::from_str(&answer.body).unwrap()
    }
    /// Tenant `t`, namespace `ns` with a generation cap, and a client token allowed everything.
    async fn client(&self) -> String {
        self.admin("/admin/v1/tenants/create", json!({"external_id": "t"}))
            .await;
        self.admin(
            "/admin/v1/namespaces/create",
            json!({"tenant": "t", "namespace": "ns"}),
        )
        .await;
        self.admin(
            "/admin/v1/tenants/settings",
            json!({"tenant": "t", "generation_daily_token_cap": 100_000_000}),
        )
        .await;
        let issued = self
            .admin(
                "/admin/v1/credentials/issue",
                json!({"tenant": "t", "label": "client", "expires_at": null, "grants": [
                    {"namespace": "ns", "read_max": 9, "write_max": 9,
                     "operations": ["save", "search", "fetch", "context", "generate",
                                    "generation_status", "sources"]}]}),
            )
            .await;
        issued["token"].as_str().unwrap().to_owned()
    }
    fn worker(&self) -> GenerationWorker {
        let provider: Arc<dyn GenerationProvider> = self.provider.clone();
        GenerationWorker::new(
            self.app.store().clone(),
            provider,
            GenerationSettings {
                workers: 1,
                lease: Duration::from_secs(5),
                heartbeat: Duration::from_secs(2),
                margin: Duration::from_millis(500),
                reaper_grace: Duration::ZERO,
                idle: Duration::from_millis(10),
                maintenance_tick: Duration::from_millis(50),
                batch: 500,
                shutdown_grace: Duration::from_millis(300),
                ..GenerationSettings::default()
            },
        )
    }
}

fn later() -> String {
    (Utc::now() + Days::hours(2)).to_rfc3339()
}

/// A2-1: the export answers an operator only, carries the closed-label counters and the
/// database gauges, and a provisioner gets 403 and a client 401.
#[tokio::test]
async fn memory_operations_metrics_export_is_operator_only_and_counts_requests() {
    let f = fixture().await;
    let client = f.client().await;
    let router = routes(&f);
    let saved = send(
        &router,
        "POST",
        "/v1/observations/save",
        &client,
        None,
        Some(
            json!({"idempotency_key": "s", "expires_at": later(), "namespace": "ns",
                    "session_id": null, "context": {"producing_ceiling": 3,
                    "consumed": {"kind": "exact", "level": 3}, "sources": [],
                    "external_sources": [], "expected_access_level": null},
                    "items": [{"kind": "fact", "title": "t", "body": "b",
                               "occurred_at": "2026-11-08T10:00:00Z", "expires_at": null}]}),
        ),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let provisioner = f
        .admin(
            "/admin/v1/admin-credentials/issue",
            json!({"scope": "provisioner", "label": "p", "expires_at": null}),
        )
        .await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        send(
            &router,
            "GET",
            "/admin/v1/metrics",
            &provisioner,
            None,
            None
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&router, "GET", "/admin/v1/metrics", &client, None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let export = send(
        &router,
        "GET",
        "/admin/v1/metrics",
        &f.operator_token,
        None,
        None,
    )
    .await;
    assert_eq!(export.status, StatusCode::OK);
    assert!(export.request_id.is_some());
    for line in [
        "# TYPE memory_requests_total counter",
        "memory_requests_total{operation=\"observations.save\",code=\"ok\"}",
        "memory_ready 1",
        "memory_metrics_query_ok 1",
        "memory_index_pending 1",
        "memory_generation_operations{state=\"accepted\"} 0",
        // This app holds no breaker (no generation): never paused.
        "memory_provider_auth_breaker_open 0",
    ] {
        assert!(export.body.contains(line), "{line} in\n{}", export.body);
    }
    assert!(!export.body.contains("\"t\""), "no tenant label");
    // A gauge query that cannot finish (a lock held elsewhere) is cut by its statement timeout;
    // the export still answers, with the counters and `memory_metrics_query_ok 0`.
    let mut blocker = f.db.pool().begin().await.unwrap();
    sqlx::query("LOCK TABLE generation_operations IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let degraded = send(
        &router,
        "GET",
        "/admin/v1/metrics",
        &f.operator_token,
        None,
        None,
    )
    .await;
    blocker.rollback().await.unwrap();
    assert_eq!(degraded.status, StatusCode::OK);
    assert!(
        degraded.body.contains("memory_metrics_query_ok 0"),
        "{}",
        degraded.body
    );
    assert!(
        degraded.body.contains("memory_requests_total{"),
        "counters still answer"
    );
    assert!(
        !degraded.body.contains("memory_index_pending"),
        "no stale gauges"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

/// A2-2: every answer carries `x-request-id`; a malformed correlation id is 400; a valid one is
/// recorded on the audit row and on the accepted operation, and the MCP call refuses a bad one.
#[tokio::test]
async fn memory_operations_correlation_ids_reach_audit_and_operations() {
    let f = fixture().await;
    let client = f.client().await;
    let router = routes(&f);
    let bad = send(
        &router,
        "GET",
        "/v1/credential",
        &client,
        Some("has space"),
        None,
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{}", bad.body);
    assert!(bad.request_id.is_some(), "errors carry the request id");
    let request = json!({"idempotency_key": "g", "expires_at": later(), "namespace": "ns",
        "session_id": null, "producing_ceiling": 6, "consumed": {"kind": "unknown"},
        "description": "capture", "allowance_tokens": 100_000, "max_output_tokens": 1024,
        "summary": false, "events": [{"event_id": "e1", "source_ref": "chat:1", "revision": "1",
        "occurred_at": "2026-11-08T10:00:00Z", "access_level": 2, "role": "user",
        "text": "hello", "exclude": false}]});
    let accepted = send(
        &router,
        "POST",
        "/v1/generations/request",
        &client,
        Some("ob.g.exchange-1"),
        Some(request),
    )
    .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.body);
    let request_id = accepted.request_id.unwrap();
    let (audited_request, audited_correlation): (Uuid, String) = sqlx::query_as(
        "SELECT request_id, correlation_id FROM audit_events WHERE action = 'generation.request'",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(audited_request.to_string(), request_id);
    assert_eq!(audited_correlation, "ob.g.exchange-1");
    let on_operation: String =
        sqlx::query_scalar("SELECT correlation_id FROM generation_operations")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(on_operation, "ob.g.exchange-1");
    // The settle audit row carries the operation's correlation too.
    f.worker().pass(&CancellationToken::new()).await.unwrap();
    let settled: Option<String> = sqlx::query_scalar(
        "SELECT correlation_id FROM audit_events WHERE action = 'generation.settle'",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(settled.as_deref(), Some("ob.g.exchange-1"));
    // MCP: a malformed header on the HTTP request is refused for the call it carries.
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "memory_credential", "arguments": {}}});
    let mcp = send(
        &router,
        "POST",
        "/mcp",
        &client,
        Some("bad id!"),
        Some(body),
    )
    .await;
    assert!(mcp.body.contains("invalid_request"), "{}", mcp.body);
}

/// The tracing output of one test, captured on its own thread (the runtime is current-thread,
/// so every task logs to this subscriber).
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// One process-wide subscriber for this test binary (review A2 F1): a thread-local one misses
/// worker lines whose callsites another test's thread registered first. Nothing else in the
/// crate's tests installs a global subscriber; if something did, the capture stays empty and
/// the positive control below fails instead of passing vacuously.
fn capture() -> &'static Captured {
    static CAPTURE: std::sync::OnceLock<Captured> = std::sync::OnceLock::new();
    CAPTURE.get_or_init(|| {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(captured.clone())
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        captured
    })
}

/// A2-3: a save, search, pack, generation and refused requests with a marker in every text
/// field leave no marker and no bearer token in the logs. The positive control first proves that
/// request, operation and worker lines were captured, each with the correlation id.
#[tokio::test]
async fn memory_operations_logs_never_carry_content_or_secrets() {
    let captured = capture();
    let f = fixture().await;
    let client = f.client().await;
    let router = routes(&f);
    let correlation = Some("ob.r.hygiene-1");
    let context = json!({"producing_ceiling": 3, "consumed": {"kind": "exact", "level": 3},
        "sources": [], "external_sources": [], "expected_access_level": null});
    let calls = [
        (
            "/v1/observations/save",
            json!({"idempotency_key": "s", "expires_at": later(),
            "namespace": "ns", "session_id": null, "context": context,
            "items": [{"kind": "fact", "title": format!("{MARKER} title"),
                       "body": format!("{MARKER} body"), "occurred_at": "2026-11-08T10:00:00Z",
                       "expires_at": null}]}),
        ),
        (
            "/v1/observations/search",
            json!({"namespaces": ["ns"], "read_ceiling": 9,
            "query": MARKER, "limit": 5}),
        ),
        (
            "/v1/context/pack",
            json!({"namespaces": ["ns"], "read_ceiling": 9, "query": MARKER,
            "max_tokens": 2000, "max_bytes": 20000}),
        ),
        (
            "/v1/generations/request",
            json!({"idempotency_key": "g", "expires_at": later(),
            "namespace": "ns", "session_id": null, "producing_ceiling": 6,
            "consumed": {"kind": "unknown"}, "description": MARKER,
            "allowance_tokens": 100_000, "max_output_tokens": 1024, "summary": false,
            "events": [{"event_id": "e1", "source_ref": "chat:1", "revision": "1",
                        "occurred_at": "2026-11-08T10:00:00Z", "access_level": 2,
                        "role": "user", "text": format!("{MARKER} event"), "exclude": false}]}),
        ),
        // Refused: serde quotes an unknown field and a wrongly typed value in its message.
        (
            "/v1/observations/search",
            json!({"namespaces": ["ns"], "read_ceiling": 9,
            "query": "x", MARKER: 1}),
        ),
        (
            "/v1/observations/search",
            json!({"namespaces": ["ns"], "read_ceiling": MARKER,
            "query": "x"}),
        ),
    ];
    for (path, body) in calls {
        send(&router, "POST", path, &client, correlation, Some(body)).await;
    }
    f.worker().pass(&CancellationToken::new()).await.unwrap();
    let logs = String::from_utf8_lossy(&captured.0.lock().unwrap()).into_owned();
    // Positive control: this test's own lines (the capture is process-wide) exist and carry
    // its correlation id: a request, the pack, the accepted generation and the worker's settle.
    for needle in [
        "operation=\"observations.save\"",
        "operation=\"context.pack\" code=ok",
        "operation=\"generations.request\"",
        "generation settled",
    ] {
        assert!(
            logs.lines()
                .any(|line| line.contains(needle) && line.contains("correlation_id=ob.r.hygiene-1")),
            "captured {needle} with the correlation id:\n{logs}"
        );
    }
    assert!(!logs.contains(MARKER), "a marker leaked:\n{logs}");
    assert!(!logs.contains(&client), "a token leaked");
    assert!(!logs.contains(&f.operator_token), "a token leaked");
    let _ = &f.operator;
}

/// A4-5: overload is refused fast and recovers. With two permits and twelve saves that each hold
/// theirs for about 400 ms, the excess answers 429 `capacity_exceeded` with `Retry-After` and
/// its own request id, nothing answers 5xx, the refusals are counted, and every refused save
/// succeeds when retried.
#[tokio::test]
async fn memory_operations_overload_is_refused_fast_and_recovers() {
    let f = fixture().await;
    let client = f.client().await;
    let router = router(
        f.app.clone(),
        Arc::new(AtomicBool::new(true)),
        &HttpSettings {
            max_in_flight: 2,
            request_deadline: Duration::from_secs(10),
            mcp_allowed_hosts: vec!["localhost".into()],
            mcp_allowed_origins: vec![],
        },
        CancellationToken::new(),
    );
    let body = |i: usize| {
        json!({"idempotency_key": format!("hold-{i}"), "expires_at": later(), "namespace": "ns",
               "session_id": null, "context": {"producing_ceiling": 3,
               "consumed": {"kind": "exact", "level": 3}, "sources": [], "external_sources": [],
               "expected_access_level": null},
               "items": [{"kind": "fact", "title": format!("fact {i}"), "body": "b",
                          "occurred_at": "2026-11-08T10:00:00Z", "expires_at": null}]})
    };
    let refused_before = crate::application::metrics::METRICS.get(
        "memory_requests_total",
        &[("operation", "unrouted"), ("code", "capacity_exceeded")],
    );
    let started = std::time::Instant::now();
    let handles: Vec<_> = (0..12)
        .map(|i| {
            let router = router.clone();
            let client = client.clone();
            let body = body(i);
            tokio::spawn(async move {
                let request = Request::builder()
                    .method("POST")
                    .uri("/v1/observations/save")
                    .header(header::HOST, "localhost")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {client}"))
                    .body(Body::from(body.to_string()))
                    .unwrap();
                let response = router.oneshot(request).await.unwrap();
                (
                    i,
                    response.status(),
                    response.headers().contains_key(header::RETRY_AFTER),
                    response.headers().contains_key("x-request-id"),
                )
            })
        })
        .collect();
    let mut answers = Vec::new();
    for handle in handles {
        answers.push(handle.await.unwrap());
    }
    let refused: Vec<usize> = answers
        .iter()
        .filter(|(_, status, ..)| *status == StatusCode::TOO_MANY_REQUESTS)
        .map(|(i, ..)| *i)
        .collect();
    assert!(!refused.is_empty(), "the excess was refused: {answers:?}");
    for (i, status, retry_after, request_id) in &answers {
        assert!(!status.is_server_error(), "{i}: {status}");
        assert!(request_id, "{i}: x-request-id");
        if *status == StatusCode::TOO_MANY_REQUESTS {
            assert!(retry_after, "{i}: Retry-After");
        } else {
            assert_eq!(*status, StatusCode::OK, "{i}");
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "refusals do not wait: {:?}",
        started.elapsed()
    );
    assert!(
        crate::application::metrics::METRICS.get(
            "memory_requests_total",
            &[("operation", "unrouted"), ("code", "capacity_exceeded")],
        ) >= refused_before + refused.len() as u64
    );
    for i in refused {
        let retried = send(
            &router,
            "POST",
            "/v1/observations/save",
            &client,
            None,
            Some(body(i)),
        )
        .await;
        assert_eq!(retried.status, StatusCode::OK, "{i}: {}", retried.body);
    }
    let saved: i64 = sqlx::query_scalar("SELECT count(*) FROM observations")
        .fetch_one(f.db.pool())
        .await
        .unwrap();
    assert_eq!(saved, 12);
}
