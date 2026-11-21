use super::*;

#[derive(Clone, Default)]
pub(super) struct LogOutput(pub(super) Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for LogOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn attribution_company_lifecycle_operator_signal_survives_restart_and_repair() {
    use crate::adapters::identity::worker;
    let f = Fixture::new().await;
    let output = LogOutput::default();
    let writer = output.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    f.store
        .observe(&f.upstream.status(), None, None, None)
        .await
        .unwrap();
    f.upstream.mode.store(1, Ordering::Release);
    worker::iteration(f.service.clone(), CancellationToken::new()).await;
    let restarted = Arc::new(IdentityService::new(
        f.service.authority.clone(),
        f.store.clone(),
        f.store.clone(),
    ));
    let failed = worker::probe(&restarted).await.unwrap();
    assert!(failed.expired && failed.reconciliation_failed);
    f.upstream.mode.store(0, Ordering::Release);
    sqlx::query("UPDATE attribution_tenants SET reconcile_at = clock_timestamp()")
        .execute(&f.store.pool)
        .await
        .unwrap();
    worker::iteration(restarted.clone(), CancellationToken::new()).await;
    assert_eq!(
        worker::probe(&restarted).await.unwrap(),
        CompanyStatusHealth {
            expired: false,
            reconciliation_failed: false
        }
    );
    let log = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
    assert!(
        log.contains("attribution_company_status_health")
            && log.contains("expired=true reconciliation_failed=true")
    );
    assert!(
        log.contains("company status healthy")
            && log.contains("expired=false reconciliation_failed=false")
    );
    f.close().await;
}

#[tokio::test]
async fn attribution_session_html_recovers_expiry_and_revoke_but_preserves_api_and_outage_errors() {
    for upstream_revoke in [false, true] {
        let f = Fixture::new().await;
        if upstream_revoke {
            f.upstream.mode.store(1, Ordering::Release);
        } else {
            sqlx::query("UPDATE attribution_sessions SET idle_expires_at = clock_timestamp() - interval '1 second'")
                .execute(&f.store.pool).await.unwrap();
        }
        let api = f
            .app
            .clone()
            .oneshot(f.request("/v1/session").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(api.status(), StatusCode::UNAUTHORIZED);
        for path in ["/", "/settings"] {
            let html = f
                .app
                .clone()
                .oneshot(f.request(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(html.status(), StatusCode::OK);
            assert!(
                html.headers()["set-cookie"]
                    .to_str()
                    .unwrap()
                    .contains("Max-Age=0")
            );
            let body = axum::body::to_bytes(html.into_body(), 4096).await.unwrap();
            assert!(
                std::str::from_utf8(&body)
                    .unwrap()
                    .contains("href=\"/auth/login\"")
            );
        }
        f.close().await;
    }
    let f = Fixture::new().await;
    f.upstream.mode.store(2, Ordering::Release);
    let response = f
        .app
        .clone()
        .oneshot(f.request("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!response.headers().contains_key("set-cookie"));
    f.upstream.mode.store(0, Ordering::Release);
    f.service.authenticate(&f.token).await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn attribution_company_lifecycle_cold_expired_evicted_and_concurrent_access() {
    let f = Fixture::new().await;
    let company = f.upstream.company;
    let (first, second) = tokio::join!(f.service.tenant(company), f.service.tenant(company));
    assert!(first.is_ok() && second.is_ok());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM attribution_tenants WHERE company_id = $1")
            .bind(company.0)
            .fetch_one(&f.store.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let calls = f.upstream.calls.load(Ordering::Acquire);
    f.service.tenant(company).await.unwrap();
    assert_eq!(
        f.upstream.calls.load(Ordering::Acquire),
        calls,
        "warm tenant access does not call Openbot"
    );
    f.upstream.mode.store(1, Ordering::Release);
    let cold = IdentityService::new(
        f.service.authority.clone(),
        f.store.clone(),
        f.store.clone(),
    );
    assert!(
        cold.tenant(company).await.is_err(),
        "another process must not trust persisted freshness"
    );
    assert!(f.upstream.calls.load(Ordering::Acquire) > calls);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(301)).await;
    tokio::time::resume();
    f.upstream.mode.store(2, Ordering::Release);
    assert!(matches!(
        f.service.tenant(company).await,
        Err(Failure::Unavailable)
    ));
    f.upstream.mode.store(0, Ordering::Release);
    f.service.tenant(company).await.unwrap();
    // Make the original ticket strictly older, then exceed the fixed cache bound.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::time::resume();
    for _ in 0..1024 {
        let mut status = f.upstream.status();
        status.company_id = CompanyId(Uuid::new_v4());
        f.service
            .freshness
            .record(&status, tokio::time::Instant::now())
            .unwrap();
    }
    assert!(f.service.freshness.ticket(company).is_none());
    let before = f.upstream.calls.load(Ordering::Acquire);
    f.service.tenant(company).await.unwrap();
    assert_eq!(f.upstream.calls.load(Ordering::Acquire), before + 1);
    f.close().await;
}

#[tokio::test]
async fn attribution_sso_callback_rejects_unbound_browser_and_limits_login() {
    let f = Fixture::new().await;
    let login = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::FOUND);
    let cookie = login.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let location = url::Url::parse(login.headers()["location"].to_str().unwrap()).unwrap();
    let state = location
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    for (state, cookie) in [
        (state.clone(), None),
        (crypto::random(), Some(cookie.clone())),
        (
            state,
            Some(format!("__Host-attribution-login={}", crypto::random())),
        ),
    ] {
        let mut request = Request::builder().uri(format!(
            "/auth/callback?code={}&state={state}",
            crypto::random()
        ));
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        let response = f
            .app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!response.headers().contains_key("set-cookie"));
    }
    assert_eq!(f.upstream.calls.load(Ordering::Acquire), 0);
    let mut limited = false;
    for _ in 0..40 {
        let response = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/auth/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            assert!(response.headers().contains_key("retry-after"));
            limited = true;
            break;
        }
    }
    assert!(
        limited,
        "login admission must reject a burst before unlimited database work"
    );
    f.close().await;
}
