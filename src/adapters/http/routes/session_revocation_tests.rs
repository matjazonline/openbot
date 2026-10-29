use super::*;
use crate::{
    adapters::{
        http::auth::require_auth,
        persistence::{
            PostgresPersistence,
            test_support::{OwnDatabase, own_database},
        },
    },
    application::session::{SessionCleanup, SessionPersistence, SessionRecord, VerifiedSession},
    entities::user::User,
    infra::config::AppConfig,
    use_cases::user::UserPersistence,
};
use axum::{
    Router, middleware,
    routing::{get, post},
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tower::ServiceExt;

struct ObservedStore {
    inner: Arc<PostgresPersistence>,
    lookups: AtomicUsize,
    reject_revoke: AtomicBool,
}
#[async_trait::async_trait]
impl SessionPersistence for ObservedStore {
    async fn create_session(&self, row: &SessionRecord) -> AppResult<()> {
        self.inner.create_session(row).await
    }
    async fn session_is_live(&self, row: &SessionRecord) -> AppResult<bool> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        self.inner.session_is_live(row).await
    }
    async fn revoke_session(&self, row: VerifiedSession) -> AppResult<()> {
        if self.reject_revoke.load(Ordering::SeqCst) {
            return Err(AppError::Database("injected revoke outage".into()));
        }
        self.inner.revoke_session(row).await
    }
    async fn cleanup_sessions(&self) -> AppResult<SessionCleanup> {
        self.inner.cleanup_sessions().await
    }
}

struct Fixture {
    database: OwnDatabase,
    sessions: Arc<SessionAuthority>,
    store: Arc<ObservedStore>,
    user: User,
}
impl Fixture {
    async fn new() -> Option<Self> {
        let database = own_database().await?;
        let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let user = persistence
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap();
        let store = Arc::new(ObservedStore {
            inner: persistence,
            lookups: AtomicUsize::new(0),
            reject_revoke: AtomicBool::new(false),
        });
        let sessions = Arc::new(SessionAuthority::new(&AppConfig::for_test(), store.clone()));
        Some(Self {
            database,
            sessions,
            store,
            user,
        })
    }
    fn router(&self) -> Router {
        Router::new()
            .route("/api/session", get(identity))
            .route("/ui/session", get(identity))
            .route("/logout", post(logout))
            .route_layer(middleware::from_fn_with_state(
                self.sessions.clone(),
                require_auth,
            ))
            .with_state(self.sessions.clone())
    }
    async fn cookie(&self) -> String {
        self.sessions.cookie(&self.user).await.unwrap().to_string()
    }
}
async fn identity(
    first: AuthenticatedUser,
    second: AuthenticatedUser,
    Extension(session): Extension<VerifiedSession>,
) -> String {
    assert_eq!(first.id, second.id);
    assert_eq!(first.id, session.user_id);
    first.id.to_string()
}
fn request(path: &str, cookie: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .uri(path)
        .header("cookie", cookie)
        .body(axum::body::Body::empty())
        .unwrap()
}
fn logout_request(cookie: &str) -> axum::http::Request<axum::body::Body> {
    let mut request = request("/logout", cookie);
    *request.method_mut() = axum::http::Method::POST;
    request
}

#[tokio::test]
async fn security_session_logout_rejects_copied_cookie_preserves_second_and_validates_once() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let first = f.cookie().await;
    let second = f.cookie().await;
    let app = f.router();
    let before = app
        .clone()
        .oneshot(request("/api/session", &first))
        .await
        .unwrap();
    assert_eq!(before.status(), StatusCode::OK);
    assert_eq!(
        f.store.lookups.load(Ordering::SeqCst),
        1,
        "two extractors share admission"
    );
    let response = app.clone().oneshot(logout_request(&first)).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .any(|c| c.to_str().unwrap().contains("Max-Age=0"))
    );
    for (path, htmx, expected) in [
        ("/api/session", false, StatusCode::UNAUTHORIZED),
        ("/ui/session", false, StatusCode::SEE_OTHER),
        ("/ui/session", true, StatusCode::OK),
    ] {
        let mut req = request(path, &first);
        if htmx {
            req.headers_mut()
                .insert("HX-Request", "true".parse().unwrap());
        }
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), expected);
        if htmx {
            assert_eq!(response.headers()["HX-Redirect"], "/login");
        }
    }
    let replay = app.clone().oneshot(logout_request(&first)).await.unwrap();
    assert!(
        replay.headers().get("set-cookie").is_none(),
        "replay is rejected at admission"
    );
    assert_eq!(
        app.oneshot(request("/api/session", &second))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(f.store.lookups.load(Ordering::SeqCst), 7);
}

#[tokio::test]
async fn security_session_store_outage_and_revoke_failure_keep_cookie() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let cookie = f.cookie().await;
    let app = f.router();
    f.store.reject_revoke.store(true, Ordering::SeqCst);
    let failed = app.clone().oneshot(logout_request(&cookie)).await.unwrap();
    assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(failed.headers().get("set-cookie").is_none());
    assert!(failed.headers().get("location").is_none());
    assert_eq!(
        app.clone()
            .oneshot(request("/api/session", &cookie))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    f.database.pool.close().await;
    for path in ["/api/session", "/ui/session", "/logout"] {
        let mut req = if path == "/logout" {
            logout_request(&cookie)
        } else {
            request(path, &cookie)
        };
        req.headers_mut()
            .insert("HX-Request", "true".parse().unwrap());
        let failed = app.clone().oneshot(req).await.unwrap();
        assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(failed.headers().get("set-cookie").is_none());
        assert!(failed.headers().get("HX-Redirect").is_none());
    }
}

#[tokio::test]
async fn security_session_admitted_request_finishes_after_revoke_commit() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let route = {
        let entered = entered.clone();
        let release = release.clone();
        get(move |_user: AuthenticatedUser| async move {
            entered.notify_one();
            release.notified().await;
            StatusCode::OK
        })
    };
    let held = Router::new()
        .route("/api/held", route)
        .route_layer(middleware::from_fn_with_state(
            f.sessions.clone(),
            require_auth,
        ));
    let cookie = f.cookie().await;
    let app = f.router();
    let admitted = tokio::spawn(held.oneshot(request("/api/held", &cookie)));
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(logout_request(&cookie))
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        app.oneshot(request("/api/session", &cookie))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    release.notify_one();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), admitted)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn security_session_competing_revocation_and_validation_observe_commit() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let token = f.sessions.cookie(&f.user).await.unwrap();
    let verified = f.sessions.verify(token.value()).await.unwrap().unwrap();
    let (revoked, admission) = tokio::join!(
        f.sessions.revoke(verified),
        f.sessions.verify(token.value())
    );
    revoked.unwrap();
    admission.unwrap(); // Either admission outcome is valid before the commit.
    assert!(f.sessions.verify(token.value()).await.unwrap().is_none());
}
