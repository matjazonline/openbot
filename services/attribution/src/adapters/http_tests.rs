use super::*;
use axum::{body::Bytes, http::Request, routing::post};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Notify;
use tower::ServiceExt;

fn state() -> HttpState {
    HttpState::new(
        PgPoolOptions::new()
            .connect_lazy("postgres://unused@localhost/unused")
            .unwrap(),
        Arc::new(AtomicBool::new(true)),
        true,
    )
}

#[tokio::test]
async fn attribution_startup_safe_html_errors_preserve_redaction_and_headers() {
    let app = with_boundary(
        Router::new()
            .route(
                "/safe",
                get(|| async {
                    super::super::html::error(
                        StatusCode::BAD_REQUEST,
                        "Report unavailable",
                        "Invalid <script> filter",
                    )
                }),
            )
            .route(
                "/unsafe",
                get(|| async {
                    (
                        StatusCode::BAD_REQUEST,
                        axum::response::Html("internal secret failure"),
                    )
                }),
            ),
        state(),
    );
    let safe = app
        .clone()
        .oneshot(Request::builder().uri("/safe").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(safe.status(), StatusCode::BAD_REQUEST);
    assert!(
        safe.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert_eq!(safe.headers()["x-content-type-options"], "nosniff");
    assert!(safe.headers().contains_key("x-request-id"));
    let bytes = axum::body::to_bytes(safe.into_body(), 16384).await.unwrap();
    let body = std::str::from_utf8(&bytes).unwrap();
    assert!(body.contains("role=\"alert\""));
    assert!(body.contains("&lt;script&gt;"));
    assert!(!body.contains("<script>"));
    let unsafe_response = app
        .oneshot(
            Request::builder()
                .uri("/unsafe")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        unsafe_response.headers()["content-type"],
        "application/json"
    );
    let bytes = axum::body::to_bytes(unsafe_response.into_body(), 16384)
        .await
        .unwrap();
    assert!(
        !std::str::from_utf8(&bytes)
            .unwrap()
            .contains("internal secret")
    );
}

#[tokio::test]
async fn attribution_startup_competing_requests_are_bounded_and_health_stays_cheap() {
    let mut state = state();
    state.admission = Arc::new(Semaphore::new(1));
    let slots = state.admission.clone();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let hold = {
        let entered = entered.clone();
        let release = release.clone();
        move || {
            let entered = entered.clone();
            let release = release.clone();
            async move {
                entered.notify_one();
                release.notified().await;
                StatusCode::NO_CONTENT
            }
        }
    };
    let app = with_boundary(
        Router::new()
            .route("/hold", get(hold))
            .route("/healthz", get(|| async { StatusCode::NO_CONTENT }))
            .route("/ok", get(|| async { StatusCode::NO_CONTENT })),
        state,
    );
    let first = tokio::spawn(
        app.clone()
            .oneshot(Request::builder().uri("/hold").body(Body::empty()).unwrap()),
    );
    entered.notified().await;
    let rejected = app
        .clone()
        .oneshot(Request::builder().uri("/ok").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(rejected.headers()["retry-after"], "1");
    let healthy = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(healthy.status(), StatusCode::NO_CONTENT);
    release.notify_one();
    assert_eq!(
        first.await.unwrap().unwrap().status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(slots.available_permits(), 1);
    assert_eq!(
        app.oneshot(Request::builder().uri("/ok").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn attribution_startup_body_and_error_boundaries_are_safe_and_correlated() {
    let app = with_boundary(
        Router::new().route("/body", post(|body: Bytes| async move { body })),
        state(),
    );
    let correlation = Uuid::new_v4();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/body?secret=do-not-echo")
                .header("x-request-id", correlation.to_string())
                .body(Body::from(vec![0; MAX_BODY_BYTES + 1]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.headers()["x-request-id"], correlation.to_string());
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert!(response.headers().contains_key("content-security-policy"));
    assert!(response.headers().contains_key("strict-transport-security"));
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["correlation_id"], correlation.to_string());
    assert!(!String::from_utf8_lossy(&body).contains("do-not-echo"));
}

#[tokio::test(start_paused = true)]
async fn attribution_startup_request_deadline_cancels_handler_and_returns_slot() {
    let state = state();
    let slots = state.admission.clone();
    let entered = Arc::new(Notify::new());
    let route = {
        let entered = entered.clone();
        move || {
            let entered = entered.clone();
            async move {
                entered.notify_one();
                std::future::pending::<()>().await;
                StatusCode::NO_CONTENT
            }
        }
    };
    let app = with_boundary(Router::new().route("/hold", get(route)), state);
    let request =
        tokio::spawn(app.oneshot(Request::builder().uri("/hold").body(Body::empty()).unwrap()));
    entered.notified().await;
    tokio::time::advance(Duration::from_secs(11)).await;
    let response = request.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(slots.available_permits(), MAX_REQUESTS);
}

#[tokio::test]
async fn attribution_startup_readiness_assets_and_shutdown_use_the_own_database() {
    let db = crate::test_support::Database::new().await;
    postgres::migrate(&db.pool).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stop = tokio_util::sync::CancellationToken::new();
    let server = tokio::spawn({
        let stop = stop.clone();
        let config = db.config();
        let pool = db.pool.clone();
        async move {
            let identity = crate::test_support::identity_http(pool.clone(), &config.public_origin);
            crate::infra::startup::run(&config, pool, listener, stop, identity).await
        }
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    for endpoint in ["healthz", "readyz"] {
        assert_eq!(
            client
                .get(format!("http://{address}/{endpoint}"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
    let asset = client
        .get(format!("http://{address}/assets/app.css"))
        .send()
        .await
        .unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert!(asset.text().await.unwrap().contains("focus-visible"));
    db.pool.close().await;
    assert_eq!(
        client
            .get(format!("http://{address}/readyz"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        client
            .get(format!("http://{address}/healthz"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(8), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}
