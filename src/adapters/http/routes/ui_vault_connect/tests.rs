use super::*;
use crate::adapters::{
    http::{auth, auth_admission, security},
    persistence::integrations::connect_tests::Fixture,
};
use crate::infra::config::AppConfig;
use axum::{
    body::{Body, to_bytes},
    extract::FromRef,
    http::{Request, StatusCode},
    middleware,
};
use tower::ServiceExt;
#[derive(Clone)]
struct TestState {
    gateway: Arc<dyn IntegrationGateway>,
    sessions: Arc<SessionAuthority>,
}
impl FromRef<TestState> for Arc<dyn IntegrationGateway> {
    fn from_ref(s: &TestState) -> Self {
        s.gateway.clone()
    }
}
impl FromRef<TestState> for Arc<SessionAuthority> {
    fn from_ref(s: &TestState) -> Self {
        s.sessions.clone()
    }
}
fn app(f: &Fixture) -> Router {
    let state = TestState {
        gateway: f.gateway(),
        sessions: f.sessions.clone(),
    };
    let mut config = AppConfig::for_test();
    config.app_domain_name = "localhost:3000".into();
    router()
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            f.sessions.clone(),
            auth::require_auth,
        ))
        .layer(middleware::from_fn_with_state(
            Arc::new(config),
            security::browser_security,
        ))
        .layer(middleware::from_fn_with_state(
            Arc::new(auth_admission::AuthAdmission::new(vec![])),
            auth_admission::admit,
        ))
}
fn request(
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: &str,
    origin: Option<&str>,
) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost:3000")
        .header("content-type", "application/x-www-form-urlencoded");
    if let Some(cookie) = cookie {
        b = b.header("cookie", format!("session={cookie}"));
    }
    if let Some(origin) = origin {
        b = b.header("origin", origin);
    }
    b.body(Body::from(body.to_owned())).unwrap()
}
#[tokio::test]
async fn integration_connect_browser_start_callback_status_and_replay_are_sanitized() {
    let f = Box::pin(Fixture::new()).await;
    let app = app(&f);
    let start = app
        .clone()
        .oneshot(request(
            "POST",
            "/ui/profile/vault/apps/gmail/connect",
            Some(&f.cookie),
            "",
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::OK);
    assert!(start.headers().get("location").is_none());
    let url = url::Url::parse(start.headers()["hx-redirect"].to_str().unwrap()).unwrap();
    assert_eq!(url.host_str(), Some("accounts.google.com"));
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let mut callback =
        url::Url::parse("http://localhost:3000/auth/integrations/google/callback").unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &state)
        .append_pair("code", "code-sentinel")
        .append_pair("scope", "openid email");
    let path = format!("{}?{}", callback.path(), callback.query().unwrap());
    let response = app
        .clone()
        .oneshot(request("GET", &path, Some(&f.cookie), "", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert_eq!(response.headers()["cache-control"], "no-store");
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    assert!(!location.contains("sentinel") && !location.contains(&state));
    let response = app
        .clone()
        .oneshot(request("GET", &location, Some(&f.cookie), "", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = String::from_utf8(
        to_bytes(response.into_body(), 65536)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("Private account connected") && html.contains("Company access"));
    for secret in [
        "code-sentinel",
        "access-sentinel",
        "refresh-sentinel",
        "client-secret-sentinel",
        "enc:v2",
    ] {
        assert!(!html.contains(secret));
    }
    app.oneshot(request("GET", &path, Some(&f.cookie), "", None))
        .await
        .unwrap();
    assert_eq!(f.script.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
#[tokio::test]
async fn integration_connect_browser_auth_csrf_form_limits_and_switched_session_deny() {
    let f = Box::pin(Fixture::new()).await;
    let app = app(&f);
    let path = "/ui/profile/vault/apps/gmail/connect";
    let denied = app
        .clone()
        .oneshot(request(
            "POST",
            path,
            None,
            "",
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::SEE_OTHER);
    assert_eq!(denied.headers()["location"], "/login");
    let denied = app
        .clone()
        .oneshot(request(
            "POST",
            path,
            Some(&f.cookie),
            "",
            Some("https://foreign.example"),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let huge = app
        .clone()
        .oneshot(request(
            "POST",
            path,
            Some(&f.cookie),
            &format!("account={}", "x".repeat(9000)),
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert_eq!(huge.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let forged = app
        .clone()
        .oneshot(request(
            "POST",
            path,
            Some(&f.cookie),
            "owner_subject=foreign",
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let start = f.begin(None).await;
    let callback = f.callback(&start);
    let other = f.new_cookie().await;
    let mut url =
        url::Url::parse("http://localhost:3000/auth/integrations/google/callback").unwrap();
    url.query_pairs_mut()
        .append_pair("state", &callback.state)
        .append_pair("code", "code-sentinel");
    let response = app
        .oneshot(request(
            "GET",
            &format!("{}?{}", url.path(), url.query().unwrap()),
            Some(&other),
            "",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(
        response.headers()["location"]
            .to_str()
            .unwrap()
            .contains("invalid_state")
    );
    assert_eq!(f.script.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn integration_connect_browser_rejects_a_different_authenticated_user() {
    let f = Box::pin(Fixture::new()).await;
    let app = app(&f);
    let start = f.begin(None).await;
    let callback = f.callback(&start);
    let other = f.other_cookie().await;
    let mut url =
        url::Url::parse("http://localhost:3000/auth/integrations/google/callback").unwrap();
    url.query_pairs_mut()
        .append_pair("state", &callback.state)
        .append_pair("code", "code-sentinel");
    let response = app
        .oneshot(request(
            "GET",
            &format!("{}?{}", url.path(), url.query().unwrap()),
            Some(&other),
            "",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(
        response.headers()["location"]
            .to_str()
            .unwrap()
            .contains("invalid_state")
    );
    assert_eq!(f.script.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn integration_connect_browser_unauthenticated_callback_does_not_forward_secrets() {
    let f = Box::pin(Fixture::new()).await;
    let response = app(&f)
        .oneshot(request(
            "GET",
            "/auth/integrations/google/callback?code=code-sentinel&state=state-sentinel",
            None,
            "",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()["location"], "/login");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(f.script.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

mod admission;
mod navigation;
