use super::*;
use crate::{
    adapters::http::{
        attribution::{self, Bridge},
        security,
        session::SessionAuthority,
    },
    infra::{attribution::AttributionConfig, config::AppConfig},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    middleware,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use std::sync::Arc;
use tower::ServiceExt;
#[path = "access_tests.rs"]
mod access_tests;
struct HttpFixture {
    f: Fixture,
    app: Router,
    cookie: String,
    client: String,
    verifier: String,
    authorize: String,
}
impl HttpFixture {
    async fn new() -> Self {
        let f = Fixture::new().await;
        let mut config = AppConfig::for_test();
        config.secure_cookies = false;
        let config = Arc::new(config);
        let sessions = Arc::new(SessionAuthority::new(
            &config,
            Arc::new(PostgresPersistence::new(f.store.pool.clone())),
        ));
        let account = UserPersistence::get_by_id(&f.store, f.user)
            .await
            .unwrap()
            .unwrap();
        let cookie = sessions.cookie(&account).await.unwrap().to_string();
        let secret = URL_SAFE_NO_PAD.encode([4u8; 32]);
        let lifecycle = URL_SAFE_NO_PAD.encode([5u8; 32]);
        let registration = AttributionConfig::from_lookup(|name| match name {
            "OPENBOT_ATTRIBUTION_ORIGIN" => Some("https://attribution.example.test".into()),
            "OPENBOT_ATTRIBUTION_CLIENT_SECRET" => Some(secret.clone()),
            "OPENBOT_ATTRIBUTION_LIFECYCLE_SECRET" => Some(lifecycle.clone()),
            _ => None,
        })
        .unwrap()
        .unwrap();
        let store = Arc::new(PostgresPersistence::new(f.store.pool.clone()));
        let bridge =
            Arc::new(Bridge::new(Arc::new(registration), store.clone(), store, sessions).unwrap());
        let app = attribution::router(Some(bridge)).layer(middleware::from_fn_with_state(
            config,
            security::browser_security,
        ));
        let (challenge, verifier) = oauth2::PkceCodeChallenge::new_random_sha256();
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("client_id", "attribution")
            .append_pair(
                "redirect_uri",
                "https://attribution.example.test/auth/callback",
            )
            .append_pair("response_type", "code")
            .append_pair("state", &"s".repeat(43))
            .append_pair("code_challenge", challenge.as_str())
            .append_pair("code_challenge_method", "S256")
            .finish();
        Self {
            f,
            app,
            cookie,
            client: format!("Basic {}", STANDARD.encode(format!("attribution:{secret}"))),
            verifier: verifier.secret().clone(),
            authorize: format!("/api/attribution/v1/authorize?{query}"),
        }
    }
    async fn code(&self) -> String {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&self.authorize)
                    .header(header::COOKIE, &self.cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let callback =
            url::Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        assert_eq!(
            callback.origin().ascii_serialization(),
            "https://attribution.example.test"
        );
        callback
            .query_pairs()
            .find(|(name, _)| name == "code")
            .unwrap()
            .1
            .into_owned()
    }
    async fn redeem(&self, code: &str, verifier: &str) -> axum::response::Response {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "authorization_code")
            .append_pair("code", code)
            .append_pair(
                "redirect_uri",
                "https://attribution.example.test/auth/callback",
            )
            .append_pair("code_verifier", verifier)
            .finish();
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/attribution/v1/token")
                    .method("POST")
                    .header(header::AUTHORIZATION, &self.client)
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
}
#[tokio::test]
async fn attribution_sso_http_authentication_precedes_parsing_and_rejects_callback_injection() {
    let f = HttpFixture::new().await;
    for (authorization, body, expected) in [
        (
            "Bearer lifecycle".to_owned(),
            "malformed".to_owned(),
            StatusCode::UNAUTHORIZED,
        ),
        (
            f.client.clone(),
            "x".repeat(4097),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let response = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/attribution/v1/token")
                    .method("POST")
                    .header(header::AUTHORIZATION, authorization)
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    for query in [
        format!("{}&user_id={}", f.authorize, Uuid::new_v4()),
        f.authorize
            .replace("client_id=attribution", "client_id=other"),
        f.authorize
            .replace("attribution.example.test", "evil.example.test"),
    ] {
        let response = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(query)
                    .header(header::COOKIE, &f.cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!response.headers().contains_key(header::LOCATION));
    }
}
#[tokio::test]
async fn attribution_sso_http_pkce_replay_expiry_and_credential_classes() {
    let f = HttpFixture::new().await;
    let code = f.code().await;
    assert_eq!(
        f.redeem(&code, &"w".repeat(43)).await.status(),
        StatusCode::BAD_REQUEST
    );
    let token = f.redeem(&code, &f.verifier).await;
    assert_eq!(token.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(token.into_body(), 4096).await.unwrap()).unwrap();
    let bearer = format!("Bearer {}", payload["access_token"].as_str().unwrap());
    let identity = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/attribution/v1/identity")
                .header(header::AUTHORIZATION, &bearer)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(identity.status(), StatusCode::OK);
    assert_eq!(
        f.redeem(&code, &f.verifier).await.status(),
        StatusCode::BAD_REQUEST
    );
    let expired = f.code().await;
    sqlx::query("UPDATE attribution_identity_codes SET expires_at = clock_timestamp() - interval '1 second' WHERE verifier = $1").bind(hash(&expired).0).execute(&f.f.store.pool).await.unwrap();
    assert_eq!(
        f.redeem(&expired, &f.verifier).await.status(),
        StatusCode::BAD_REQUEST
    );
    let wrong = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/attribution/v1/identity")
                .header(header::AUTHORIZATION, &f.client)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn attribution_company_lifecycle_http_cookie_csrf_and_version_guard() {
    let f = HttpFixture::new().await;
    let company = f.f.company().await;
    let path = format!("/api/attribution/v1/companies/{company}/status");
    let body = r#"{"state":"suspended","expected_version":1}"#;
    for origin in [None, Some("http://evil.example.test")] {
        let mut request = Request::builder()
            .uri(&path)
            .method("POST")
            .header(header::HOST, "localhost:3099")
            .header(header::COOKIE, &f.cookie)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let response = f
            .app
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let bypass = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&path)
                .method("POST")
                .header(header::COOKIE, &f.cookie)
                .header(header::AUTHORIZATION, &f.client)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bypass.status(), StatusCode::UNAUTHORIZED);
    for expected in [StatusCode::OK, StatusCode::CONFLICT] {
        let response = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&path)
                    .method("POST")
                    .header(header::HOST, "localhost:3099")
                    .header(header::ORIGIN, "http://localhost:3099")
                    .header(header::COOKIE, &f.cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(
        f.f.store.status(company).await.unwrap().state,
        CompanyState::Suspended
    );
}
