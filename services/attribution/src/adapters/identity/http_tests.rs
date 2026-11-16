use super::crypto;
use crate::{
    adapters::{
        http::{HttpState, router},
        identity::postgres::IdentityPostgres,
    },
    application::identity::*,
    infra::identity::{IdentityConfig, compose},
    test_support::Database,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use base64::Engine;
use chrono::Utc;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use uuid::Uuid;
#[path = "http_boundary_tests.rs"]
mod boundary_tests;
#[derive(Clone)]
struct AuthorityFixture {
    mode: Arc<AtomicU8>,
    calls: Arc<AtomicUsize>,
    user: UserId,
    company: CompanyId,
}
impl AuthorityFixture {
    async fn boundary(&self) -> Option<StatusCode> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match self.mode.load(Ordering::Acquire) {
            1 => Some(StatusCode::UNAUTHORIZED),
            2 => {
                tokio::time::sleep(Duration::from_secs(4)).await;
                Some(StatusCode::SERVICE_UNAVAILABLE)
            }
            6 => Some(StatusCode::FORBIDDEN),
            _ => None,
        }
    }
    fn status(&self) -> CompanyStatus {
        CompanyStatus {
            company_id: self.company,
            version: if self.mode.load(Ordering::Acquire) == 5 {
                2
            } else {
                1
            },
            state: if self.mode.load(Ordering::Acquire) == 5 {
                LifecycleState::Deleted
            } else {
                LifecycleState::Active
            },
            changed_at: Utc::now(),
        }
    }
    fn access(&self) -> CompanyAccess {
        CompanyAccess {
            company_id: self.company,
            name: "Company <script>".into(),
            permissions: if self.mode.load(Ordering::Acquire) == 3 {
                vec![Permission::Read]
            } else {
                vec![
                    Permission::Read,
                    Permission::ManageLinks,
                    Permission::ManageIntegrations,
                ]
            },
            lifecycle: self.status(),
        }
    }
}
async fn identity(State(state): State<AuthorityFixture>) -> axum::response::Response {
    if let Some(error) = state.boundary().await {
        return error.into_response();
    }
    Json(Identity {
        user_id: state.user,
        display_name: "User <script>".into(),
        expires_at: Utc::now() + chrono::Duration::hours(1),
    })
    .into_response()
}
async fn access(
    State(mut state): State<AuthorityFixture>,
    Path(id): Path<Uuid>,
) -> axum::response::Response {
    if let Some(error) = state.boundary().await {
        return error.into_response();
    }
    // Mode 7 is a multi-company member, used to test object scoping independently
    // from upstream membership denial.
    if state.mode.load(Ordering::Acquire) == 7 {
        state.company = CompanyId(id);
    }
    if id != state.company.0 || state.mode.load(Ordering::Acquire) == 4 {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(state.access()).into_response()
}
async fn status(
    State(mut state): State<AuthorityFixture>,
    Path(id): Path<Uuid>,
) -> axum::response::Response {
    if let Some(error) = state.boundary().await {
        return error.into_response();
    }
    if state.mode.load(Ordering::Acquire) == 7 {
        state.company = CompanyId(id);
    }
    Json(state.status()).into_response()
}
async fn companies(State(state): State<AuthorityFixture>) -> axum::response::Response {
    if let Some(error) = state.boundary().await {
        return error.into_response();
    }
    Json(CompanyPage {
        items: vec![state.access()],
        next_cursor: None,
    })
    .into_response()
}
#[path = "../catalog/http_tests.rs"]
mod catalog_tests;
#[path = "../commerce/http_tests.rs"]
mod commerce_tests;
#[path = "../setup/http_tests.rs"]
mod setup_tests;
#[path = "../website/http_tests.rs"]
mod website_tests;

struct Fixture {
    setup: crate::adapters::setup::http::SetupHttp,
    _database: Database,
    store: Arc<IdentityPostgres>,
    service: Arc<IdentityService>,
    app: Router,
    upstream: AuthorityFixture,
    token: SessionToken,
    csrf: String,
    shutdown: CancellationToken,
    server: JoinHandle<std::io::Result<()>>,
}
impl Fixture {
    fn commerce_http(
        setup: &crate::adapters::setup::http::SetupHttp,
    ) -> crate::adapters::commerce::http::CommerceHttp {
        crate::adapters::commerce::http::CommerceHttp {
            setup: setup.clone(),
            policy: crate::application::commerce::CommercePolicy::new(7, 365).unwrap(),
        }
    }

    async fn new() -> Self {
        Self::with_origin("http://localhost:8091").await
    }
    async fn with_origin(public_origin: &str) -> Self {
        let database = Database::new().await;
        crate::adapters::postgres::migrate(&database.pool)
            .await
            .unwrap();
        let upstream = AuthorityFixture {
            mode: Arc::new(AtomicU8::new(0)),
            calls: Arc::new(AtomicUsize::new(0)),
            user: UserId(Uuid::new_v4()),
            company: CompanyId(Uuid::new_v4()),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let routes = Router::new()
            .route("/api/attribution/v1/identity", get(identity))
            .route("/api/attribution/v1/companies/{id}/access", get(access))
            .route("/api/attribution/v1/companies/{id}/status", get(status))
            .route("/api/attribution/v1/companies", get(companies))
            .route(
                "/api/attribution/v1/revoke",
                post(|| async { StatusCode::SERVICE_UNAVAILABLE }),
            )
            .with_state(upstream.clone());
        let shutdown = CancellationToken::new();
        let stop = shutdown.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, routes)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await
        });
        let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7u8; 32]);
        let keys = serde_json::json!({"active":1,"keys":[{"version":1,"key":secret}]}).to_string();
        let config = Arc::new(
            IdentityConfig::from_lookup(
                &url::Url::parse(public_origin).unwrap(),
                |name| match name {
                    "ATTRIBUTION_OPENBOT_ORIGIN" => Some(origin.clone()),
                    "ATTRIBUTION_CLIENT_SECRET" => {
                        Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([8u8; 32]))
                    }
                    "ATTRIBUTION_LIFECYCLE_SECRET" => Some(secret.clone()),
                    "ATTRIBUTION_SESSION_KEYS" => Some(keys.clone()),
                    _ => None,
                },
            )
            .unwrap(),
        );
        let http = compose(database.pool.clone(), config.clone()).unwrap();
        let service = http.service.clone();
        let setup = crate::adapters::setup::http::SetupHttp::production(
            database.pool.clone(),
            http.clone(),
        )
        .unwrap();
        let app = router(
            HttpState::new(
                database.pool.clone(),
                Arc::new(std::sync::atomic::AtomicBool::new(true)),
                false,
            )
            .with_identity(http)
            .with_setup(setup.clone())
            .with_commerce(Self::commerce_http(&setup)),
        );
        let store = Arc::new(IdentityPostgres {
            pool: database.pool.clone(),
            cipher: config.cipher.clone(),
        });
        let session = NewSession {
            id: SessionId(Uuid::new_v4()),
            token: SessionToken(crypto::random()),
            user_id: upstream.user,
            grant: IdentityGrant(crypto::random()),
            csrf: CsrfToken(crypto::random()),
            expires_at: Utc::now() + chrono::Duration::hours(1),
        };
        store.create_session(&session, None).await.unwrap();
        Self {
            setup,
            _database: database,
            store,
            service,
            app,
            upstream,
            token: session.token,
            csrf: session.csrf.0,
            shutdown,
            server,
        }
    }
    fn request(&self, path: &str) -> axum::http::request::Builder {
        Request::builder()
            .uri(path)
            .header("cookie", format!("__Host-attribution={}", self.token.0))
    }
    async fn close(self) {
        self.shutdown.cancel();
        self.server.await.unwrap().unwrap();
    }
}
#[tokio::test]
async fn attribution_company_access_live_roles_membership_and_foreign_ids() {
    let f = Fixture::new().await;
    let company = f.upstream.company;
    f.service
        .company(&f.token, company, Permission::ManageLinks)
        .await
        .unwrap();
    let first = f.upstream.calls.load(Ordering::Relaxed);
    f.upstream.mode.store(3, Ordering::Release);
    assert!(matches!(
        f.service
            .company(&f.token, company, Permission::ManageLinks)
            .await,
        Err(Failure::Forbidden)
    ));
    assert!(
        f.upstream.calls.load(Ordering::Relaxed) >= first + 2,
        "both identity and membership are checked again"
    );
    f.service
        .company(&f.token, company, Permission::Read)
        .await
        .unwrap();
    let foreign = format!("/?company_id={}", Uuid::new_v4());
    let response = f
        .app
        .clone()
        .oneshot(f.request(&foreign).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    f.upstream.mode.store(4, Ordering::Release);
    assert!(matches!(
        f.service.company(&f.token, company, Permission::Read).await,
        Err(Failure::NotFound)
    ));
    f.close().await;
}
#[tokio::test]
async fn attribution_session_revocation_outage_csrf_and_local_logout() {
    let f = Fixture::new().await;
    f.upstream.mode.store(1, Ordering::Release);
    assert_eq!(
        f.app
            .clone()
            .oneshot(f.request("/v1/session").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    f.upstream.mode.store(2, Ordering::Release);
    let start = Instant::now();
    let unavailable = f
        .app
        .clone()
        .oneshot(f.request("/v1/session").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(unavailable.headers().contains_key("retry-after"));
    let forbidden = f
        .app
        .clone()
        .oneshot(
            f.request("/auth/logout")
                .method("POST")
                .header("x-csrf-token", &f.csrf)
                .header("origin", "https://foreign.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    let local = f
        .app
        .clone()
        .oneshot(
            f.request("/auth/logout")
                .method("POST")
                .header("x-csrf-token", &f.csrf)
                .header("origin", "http://localhost:8091")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(local.status(), StatusCode::FOUND);
    assert_eq!(local.headers()["location"], "/");
    assert!(
        local.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert!(matches!(
        f.store.session(&f.token).await,
        Err(Failure::Unauthorized)
    ));
    for (csrf, expected) in [
        (&f.csrf[..], StatusCode::FOUND),
        ("invalid", StatusCode::FORBIDDEN),
    ] {
        let retry = f
            .app
            .clone()
            .oneshot(
                f.request("/auth/logout")
                    .method("POST")
                    .header("x-csrf-token", csrf)
                    .header("origin", "http://localhost:8091")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(retry.status(), expected);
        if expected == StatusCode::FOUND {
            assert_eq!(retry.headers()["location"], "/");
            assert!(
                retry.headers()["set-cookie"]
                    .to_str()
                    .unwrap()
                    .contains("Max-Age=0")
            );
        }
    }
    f.close().await;
}
#[tokio::test]
async fn attribution_session_cookie_classes_headers_and_escaped_navigation() {
    let f = Fixture::new().await;
    let response = f
        .app
        .clone()
        .oneshot(f.request("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    let html = String::from_utf8(
        axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("User &lt;script&gt;") && !html.contains("User <script>"));
    let forged = f
        .app
        .clone()
        .oneshot(
            f.request("/v1/session")
                .header("authorization", "Bearer store-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::UNAUTHORIZED);
    let login = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/login?return_to=%2Fsettings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::FOUND);
    let cookie = login.headers()["set-cookie"].to_str().unwrap();
    for required in [
        "__Host-attribution-login=",
        "Secure",
        "HttpOnly",
        "SameSite=Lax",
        "Path=/",
    ] {
        assert!(cookie.contains(required));
    }
    assert!(!cookie.contains("Domain="));
    let redirect = url::Url::parse(login.headers()["location"].to_str().unwrap()).unwrap();
    assert_eq!(redirect.path(), "/api/attribution/v1/authorize");
    assert!(
        redirect
            .query_pairs()
            .any(|(key, value)| key == "code_challenge_method" && value == "S256")
    );
    let denied = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/internal/v1/company-lifecycle")
                .method("POST")
                .header("authorization", "Bearer wrong")
                .body(Body::from("not-json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        denied.status(),
        StatusCode::UNAUTHORIZED,
        "authenticate before body parsing"
    );
    f.close().await;
}
use tokio::time::Instant;

#[tokio::test]
async fn attribution_company_lifecycle_body_limit_follows_header_authentication() {
    let f = Fixture::new().await;
    let oversized = "x".repeat(4097);
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7u8; 32]);
    for (credential, expected) in [
        ("invalid".to_string(), StatusCode::UNAUTHORIZED),
        (secret, StatusCode::PAYLOAD_TOO_LARGE),
    ] {
        let response = f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/internal/v1/company-lifecycle")
                    .method("POST")
                    .header("authorization", format!("Bearer {credential}"))
                    .header("content-type", "application/json")
                    .body(Body::from(oversized.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    f.close().await;
}

#[tokio::test]
async fn attribution_company_lifecycle_reconciliation_repairs_missed_delete_and_records_denial() {
    let f = Fixture::new().await;
    let original = f.upstream.status();
    f.store
        .observe(&original, Some("Example"), None, None)
        .await
        .unwrap();
    f.upstream.mode.store(6, Ordering::Release);
    super::worker::iteration(f.service.clone(), CancellationToken::new()).await;
    let failed: bool = sqlx::query_scalar(
        "SELECT last_reconcile_failure IS NOT NULL FROM attribution_tenants WHERE company_id = $1",
    )
    .bind(original.company_id.0)
    .fetch_one(&f.store.pool)
    .await
    .unwrap();
    assert!(
        failed,
        "authority denial must not be classified as a stored suspension"
    );
    sqlx::query(
        "UPDATE attribution_tenants SET reconcile_at = clock_timestamp() WHERE company_id = $1",
    )
    .bind(original.company_id.0)
    .execute(&f.store.pool)
    .await
    .unwrap();
    f.upstream.mode.store(5, Ordering::Release);
    super::worker::iteration(f.service.clone(), CancellationToken::new()).await;
    let repaired: bool = sqlx::query_scalar("SELECT state = 'deleted' AND version = 2 AND purge_requested AND display_name IS NULL AND reconcile_execution IS NULL AND last_reconcile_failure IS NULL FROM attribution_tenants WHERE company_id = $1")
        .bind(original.company_id.0).fetch_one(&f.store.pool).await.unwrap();
    assert!(repaired);
    assert!(f.store.claim_reconciliation().await.unwrap().is_empty());
    f.close().await;
}

#[tokio::test]
async fn attribution_company_lifecycle_shutdown_cancels_inflight_status_and_joins() {
    let f = Fixture::new().await;
    f.store
        .observe(&f.upstream.status(), None, None, None)
        .await
        .unwrap();
    f.upstream.mode.store(2, Ordering::Release);
    let shutdown = CancellationToken::new();
    let work = tokio::spawn(super::worker::iteration(
        f.service.clone(),
        shutdown.clone(),
    ));
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.upstream.calls.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(1), work)
        .await
        .unwrap()
        .unwrap();
    let stored: bool = sqlx::query_scalar("SELECT status_valid_until IS NULL AND reconcile_execution IS NOT NULL AND reconcile_lease_until <= clock_timestamp() + interval '30 seconds' FROM attribution_tenants WHERE company_id = $1")
        .bind(f.upstream.company.0).fetch_one(&f.store.pool).await.unwrap();
    assert!(
        stored,
        "shutdown leaves a short recoverable lease and no detached status write"
    );
    f.close().await;
}
