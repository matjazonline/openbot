//! Early, bounded admission for public authentication work. The TCP peer is the only trusted
//! source unless that peer is an explicitly configured proxy which overwrites X-Forwarded-For.

use std::{
    collections::{HashMap, HashSet},
    env,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    body::{Body, to_bytes},
    extract::MatchedPath,
    extract::connect_info::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{
    app_error::{AppError, AppResult},
    use_cases::user::AuthWorkLifetime,
};

const WINDOW: Duration = Duration::from_secs(60);
const BODY_TIMEOUT: Duration = Duration::from_secs(10);
const GLOBAL_LIMIT: u32 = 600;
const MAX_SOURCE_BUCKETS: usize = 4096;
const MAX_ACTIVE_PER_SOURCE: u8 = 2;
const UNKNOWN_SOURCE: IpAddr = IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);

/// Resolved from the TCP peer, or from a forwarded IP only when that peer is trusted.
#[derive(Clone, Copy)]
pub struct AuthClientSource(pub IpAddr);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Kind {
    Login,
    Registration,
    Confirmation,
    Oauth,
    Credential,
}

impl Kind {
    const ALL: [Self; 5] = [
        Self::Login,
        Self::Registration,
        Self::Confirmation,
        Self::Oauth,
        Self::Credential,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Login => 0,
            Self::Registration => 1,
            Self::Confirmation => 2,
            Self::Oauth => 3,
            Self::Credential => 4,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Registration => "registration",
            Self::Confirmation => "confirmation",
            Self::Oauth => "oauth",
            Self::Credential => "credential",
        }
    }

    const fn source_limit(self) -> u32 {
        match self {
            Self::Login => 30,
            Self::Registration | Self::Confirmation | Self::Credential => 12,
            Self::Oauth => 30,
        }
    }

    const fn body_limit(self) -> usize {
        match self {
            Self::Oauth => 32 * 1024,
            _ => 8 * 1024,
        }
    }
}

fn kind(method: &Method, path: &str) -> Option<Kind> {
    // Axum GET routes also invoke their handler for HEAD. Count that method before it can
    // reach an OAuth provider exchange.
    if *method == Method::HEAD {
        return kind(&Method::GET, path);
    }
    match (method, path) {
        (&Method::POST, "/login" | "/api/user/login" | "/api/json/user/login") => Some(Kind::Login),
        (&Method::POST, "/register" | "/api/user/register" | "/api/json/user/register") => {
            Some(Kind::Registration)
        }
        (&Method::POST, "/api/user/register/confirm" | "/api/json/user/register/confirm") => {
            Some(Kind::Confirmation)
        }
        (
            &Method::GET,
            "/auth/google/register"
            | "/auth/google/login"
            | "/auth/google/connect"
            | "/auth/google/callback",
        )
        | (&Method::GET, "/auth/apple/register" | "/auth/apple/login" | "/auth/apple/connect")
        | (&Method::POST, "/auth/apple/callback") => Some(Kind::Oauth),
        (&Method::PUT, "/ui/profile" | "/ui/profile/password" | "/ui/profile/password/setup")
        | (&Method::POST | &Method::DELETE, "/ui/profile/changes/{kind}") => Some(Kind::Credential),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct Window {
    opened: Instant,
    count: u32,
}

impl Window {
    fn new(now: Instant) -> Self {
        Self {
            opened: now,
            count: 0,
        }
    }
    fn refresh(&mut self, now: Instant) {
        if now.duration_since(self.opened) >= WINDOW {
            *self = Self::new(now);
        }
    }
}

struct Budget {
    global: Window,
    sources: HashMap<(IpAddr, Kind), Window>,
    active: HashMap<IpAddr, u8>,
    metrics: MetricWindow,
}

#[derive(Clone, Copy)]
struct MetricSnapshot {
    accepted: [u64; 5],
    limited: [u64; 5],
}

struct MetricWindow {
    opened: Instant,
    counts: MetricSnapshot,
}

impl MetricWindow {
    fn new(now: Instant) -> Self {
        Self {
            opened: now,
            counts: MetricSnapshot {
                accepted: [0; 5],
                limited: [0; 5],
            },
        }
    }

    fn record(&mut self, kind: Kind, accepted: bool, now: Instant) -> Option<MetricSnapshot> {
        let previous = if now.duration_since(self.opened) >= WINDOW {
            let previous = self.counts;
            *self = Self::new(now);
            Some(previous)
        } else {
            None
        };
        let counter = if accepted {
            &mut self.counts.accepted[kind.index()]
        } else {
            &mut self.counts.limited[kind.index()]
        };
        *counter = counter.saturating_add(1);
        previous
    }
}

impl Budget {
    fn record(&mut self, kind: Kind, accepted: bool, now: Instant) {
        let Some(previous) = self.metrics.record(kind, accepted, now) else {
            return;
        };
        // At most five fixed-dimension events per minute, even during a denied-request flood.
        // Platform log collection combines instances; no client identity enters a label.
        for kind in Kind::ALL {
            let accepted_count = previous.accepted[kind.index()];
            let limited_count = previous.limited[kind.index()];
            if accepted_count != 0 || limited_count != 0 {
                tracing::info!(
                    auth_route_kind = kind.as_str(),
                    accepted_count,
                    limited_count,
                    window_seconds = 60_u64,
                    "authentication admission rollup"
                );
            }
        }
    }
}

/// The last opaque lifetime clone releases one source's in-flight capacity. A blocking hash
/// keeps a clone even if its awaiting HTTP request has been cancelled.
struct ActiveSourceGuard {
    admission: Arc<AuthAdmission>,
    source: IpAddr,
}

impl Drop for ActiveSourceGuard {
    fn drop(&mut self) {
        let mut budget = self
            .admission
            .budget
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(active) = budget.active.get_mut(&self.source) {
            *active -= 1;
            if *active == 0 {
                budget.active.remove(&self.source);
            }
        }
    }
}

/// One shared limiter per server instance. Durable identifier/code budgets are separate (phase 02).
pub struct AuthAdmission {
    trusted_proxies: Vec<IpAddr>,
    budget: Mutex<Budget>,
    body_timeout: Duration,
}

impl AuthAdmission {
    pub fn from_env() -> AppResult<Self> {
        let trusted_proxies = env::var("AUTH_TRUSTED_PROXY_IPS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                value.parse::<IpAddr>().map_err(|_| {
                    AppError::BadRequest(
                        "AUTH_TRUSTED_PROXY_IPS must contain only IP addresses".into(),
                    )
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok(Self::new(trusted_proxies))
    }

    pub fn new(trusted_proxies: Vec<IpAddr>) -> Self {
        Self {
            trusted_proxies,
            budget: Mutex::new(Budget {
                global: Window::new(Instant::now()),
                sources: HashMap::new(),
                active: HashMap::new(),
                metrics: MetricWindow::new(Instant::now()),
            }),
            body_timeout: BODY_TIMEOUT,
        }
    }

    fn source(&self, headers: &HeaderMap, peer: Option<SocketAddr>) -> IpAddr {
        let Some(peer) = peer else {
            return UNKNOWN_SOURCE;
        };
        if !self.trusted_proxies.contains(&peer.ip()) {
            return peer.ip();
        }
        let forwarded = headers.get_all("x-forwarded-for");
        let mut values = forwarded.iter();
        match (values.next(), values.next()) {
            (Some(value), None) => value
                .to_str()
                .ok()
                .and_then(|text| text.trim().parse::<IpAddr>().ok())
                .unwrap_or(peer.ip()),
            _ => peer.ip(),
        }
    }

    fn admit(
        self: &Arc<Self>,
        source: IpAddr,
        kind: Kind,
        now: Instant,
    ) -> AppResult<Option<AuthWorkLifetime>> {
        let mut budget = self.budget.lock().map_err(|_| {
            AppError::ServiceUnavailable("Authentication admission unavailable".into())
        })?;
        budget.global.refresh(now);
        if budget.global.count >= GLOBAL_LIMIT {
            budget.record(kind, false, now);
            return Ok(None);
        }
        if budget.active.get(&source).copied().unwrap_or(0) >= MAX_ACTIVE_PER_SOURCE {
            budget.record(kind, false, now);
            return Ok(None);
        }
        if !budget.sources.contains_key(&(source, kind))
            && budget.sources.len() >= MAX_SOURCE_BUCKETS
        {
            let active = budget.active.keys().copied().collect::<HashSet<_>>();
            budget.sources.retain(|(ip, _), window| {
                now.duration_since(window.opened) < WINDOW || active.contains(ip)
            });
            if budget.sources.len() >= MAX_SOURCE_BUCKETS {
                budget.record(kind, false, now);
                return Ok(None);
            }
        }
        let source_window = budget
            .sources
            .entry((source, kind))
            .or_insert_with(|| Window::new(now));
        source_window.refresh(now);
        if source_window.count >= kind.source_limit() {
            budget.record(kind, false, now);
            return Ok(None);
        }
        source_window.count += 1;
        budget.global.count += 1;
        *budget.active.entry(source).or_default() += 1;
        budget.record(kind, true, now);
        drop(budget);
        Ok(Some(AuthWorkLifetime::new(ActiveSourceGuard {
            admission: self.clone(),
            source,
        })))
    }
}

pub async fn admit(
    axum::extract::State(admission): axum::extract::State<Arc<AuthAdmission>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // Axum has already matched the route before this layer runs. Use its static template:
    // Path<String> decodes dynamic segments, so a raw-path comparison would miss aliases such
    // as %65mail even though they reach the same profile-change handler.
    let Some(matched_path) = request.extensions().get::<MatchedPath>() else {
        return next.run(request).await;
    };
    let Some(kind) = kind(request.method(), matched_path.as_str()) else {
        return next.run(request).await;
    };
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|value| value.0);
    let source = admission.source(request.headers(), peer);
    let work_lifetime = match admission.admit(source, kind, Instant::now()) {
        Ok(Some(lifetime)) => lifetime,
        Ok(None) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "Authentication requests are temporarily limited",
            )
                .into_response();
        }
        Err(error) => return error.into_response(),
    };
    request.extensions_mut().insert(AuthClientSource(source));
    request.extensions_mut().insert(work_lifetime.clone());
    // Keep the request's clone through the handler. Blocking password work retains another clone
    // until its worker completes, even when this future is cancelled.
    let _work_lifetime = work_lifetime;
    if request.method() == Method::GET || request.method() == Method::HEAD {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let bytes =
        match tokio::time::timeout(admission.body_timeout, to_bytes(body, kind.body_limit())).await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(_)) => {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Authentication request is too large",
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    StatusCode::REQUEST_TIMEOUT,
                    "Authentication request timed out",
                )
                    .into_response();
            }
        };
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        routing::{get, post},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    fn app(admission: Arc<AuthAdmission>) -> Router {
        Router::new()
            .route("/api/json/user/login", post(|| async { StatusCode::OK }))
            .layer(axum::middleware::from_fn_with_state(admission, admit))
    }

    fn request(peer: SocketAddr, forwarded: &str, bytes: usize) -> Request<Body> {
        let mut request = Request::post("/api/json/user/login")
            .header("x-forwarded-for", forwarded)
            .body(Body::from("x".repeat(bytes)))
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(peer));
        request
    }

    #[tokio::test]
    async fn security_auth_limits_body_is_rejected_before_handler() {
        let app = app(Arc::new(AuthAdmission::new(vec![])));
        let response = app
            .oneshot(request("127.0.0.1:1234".parse().unwrap(), "1.2.3.4", 8193))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn security_auth_limits_untrusted_forwarded_source_cannot_escape_budget() {
        let app = app(Arc::new(AuthAdmission::new(vec![])));
        let peer = "192.0.2.10:1234".parse().unwrap();
        for n in 0..30 {
            let response = app
                .clone()
                .oneshot(request(peer, &format!("198.51.100.{n}"), 1))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = app
            .oneshot(request(peer, "203.0.113.200", 1))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn security_auth_limits_head_oauth_requests_consume_the_budget() {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/auth/google/callback",
                get({
                    let hits = hits.clone();
                    move || {
                        let hits = hits.clone();
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            StatusCode::OK
                        }
                    }
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                Arc::new(AuthAdmission::new(vec![])),
                admit,
            ));
        let peer: SocketAddr = "192.0.2.10:1234".parse().unwrap();
        for _ in 0..30 {
            let mut request = Request::head("/auth/google/callback?code=secret")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(ConnectInfo(peer));
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::OK
            );
        }
        let mut denied = Request::head("/auth/google/callback?code=secret")
            .body(Body::empty())
            .unwrap();
        denied.extensions_mut().insert(ConnectInfo(peer));
        assert_eq!(
            app.oneshot(denied).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(hits.load(Ordering::SeqCst), 30);
    }

    #[tokio::test]
    async fn security_auth_limits_stalled_body_times_out_without_reaching_handler() {
        let hits = Arc::new(AtomicUsize::new(0));
        let mut admission = AuthAdmission::new(vec![]);
        admission.body_timeout = Duration::from_millis(20);
        let app = Router::new()
            .route(
                "/api/json/user/login",
                post({
                    let hits = hits.clone();
                    move || {
                        let hits = hits.clone();
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            StatusCode::OK
                        }
                    }
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                Arc::new(admission),
                admit,
            ));
        let peer: SocketAddr = "192.0.2.10:1234".parse().unwrap();
        let mut stalled = Request::post("/api/json/user/login")
            .body(Body::from_stream(futures::stream::pending::<
                Result<bytes::Bytes, std::io::Error>,
            >()))
            .unwrap();
        stalled.extensions_mut().insert(ConnectInfo(peer));
        assert_eq!(
            app.clone().oneshot(stalled).await.unwrap().status(),
            StatusCode::REQUEST_TIMEOUT
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert_eq!(
            app.oneshot(request(peer, "1.2.3.4", 1))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn security_auth_limits_trusted_proxy_requires_one_ip() {
        let proxy: IpAddr = "192.0.2.1".parse().unwrap();
        let admission = AuthAdmission::new(vec![proxy]);
        let peer: SocketAddr = "192.0.2.1:1234".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "198.51.100.7".parse().unwrap());
        assert_eq!(
            admission.source(&headers, Some(peer)),
            "198.51.100.7".parse::<IpAddr>().unwrap()
        );
        headers.insert(
            "x-forwarded-for",
            "203.0.113.1, 198.51.100.7".parse().unwrap(),
        );
        assert_eq!(admission.source(&headers, Some(peer)), proxy);
    }

    #[test]
    fn security_auth_limits_every_costly_route_is_classified() {
        for (method, path, expected) in [
            (Method::POST, "/login", Kind::Login),
            (Method::POST, "/api/user/login", Kind::Login),
            (Method::POST, "/api/json/user/login", Kind::Login),
            (Method::POST, "/register", Kind::Registration),
            (Method::POST, "/api/user/register", Kind::Registration),
            (Method::POST, "/api/json/user/register", Kind::Registration),
            (
                Method::POST,
                "/api/user/register/confirm",
                Kind::Confirmation,
            ),
            (
                Method::POST,
                "/api/json/user/register/confirm",
                Kind::Confirmation,
            ),
            (Method::PUT, "/ui/profile", Kind::Credential),
            (Method::PUT, "/ui/profile/password", Kind::Credential),
            (Method::PUT, "/ui/profile/password/setup", Kind::Credential),
            (Method::POST, "/ui/profile/changes/{kind}", Kind::Credential),
            (
                Method::DELETE,
                "/ui/profile/changes/{kind}",
                Kind::Credential,
            ),
            (Method::GET, "/auth/google/register", Kind::Oauth),
            (Method::GET, "/auth/google/login", Kind::Oauth),
            (Method::GET, "/auth/google/connect", Kind::Oauth),
            (Method::GET, "/auth/google/callback", Kind::Oauth),
            (Method::HEAD, "/auth/google/callback", Kind::Oauth),
            (Method::GET, "/auth/apple/register", Kind::Oauth),
            (Method::GET, "/auth/apple/login", Kind::Oauth),
            (Method::GET, "/auth/apple/connect", Kind::Oauth),
            (Method::POST, "/auth/apple/callback", Kind::Oauth),
        ] {
            assert_eq!(kind(&method, path), Some(expected), "{method} {path}");
        }
        assert_eq!(kind(&Method::GET, "/login"), None);
    }

    #[tokio::test]
    async fn security_auth_limits_encoded_profile_kind_uses_matched_route_budget() {
        let admission = Arc::new(AuthAdmission::new(vec![]));
        let reached = Arc::new(AtomicUsize::new(0));
        let handler_reached = reached.clone();
        let route = post(
            move |axum::extract::Path(kind): axum::extract::Path<String>| {
                let reached = handler_reached.clone();
                async move {
                    assert_eq!(kind, "email");
                    reached.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            },
        );
        let handler_reached = reached.clone();
        let app = Router::new()
            .route(
                "/ui/profile/changes/{kind}",
                route.delete(
                    move |axum::extract::Path(kind): axum::extract::Path<String>| {
                        let reached = handler_reached.clone();
                        async move {
                            assert_eq!(kind, "email");
                            reached.fetch_add(1, Ordering::SeqCst);
                            StatusCode::OK
                        }
                    },
                ),
            )
            .layer(axum::middleware::from_fn_with_state(admission, admit));
        let send = |method: Method, path: &'static str, size: usize| {
            let app = app.clone();
            async move {
                let mut request = Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::from("x".repeat(size)))
                    .unwrap();
                request.extensions_mut().insert(ConnectInfo(
                    "192.0.2.10:1234".parse::<SocketAddr>().unwrap(),
                ));
                app.oneshot(request).await.unwrap().status()
            }
        };

        assert_eq!(
            send(Method::POST, "/ui/profile/changes/%65mail", 8193).await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(reached.load(Ordering::SeqCst), 0);
        for _ in 0..11 {
            assert_eq!(
                send(Method::POST, "/ui/profile/changes/email", 1).await,
                StatusCode::OK
            );
        }
        assert_eq!(reached.load(Ordering::SeqCst), 11);
        for method in [Method::POST, Method::DELETE] {
            assert_eq!(
                send(method, "/ui/profile/changes/%65mail", 1).await,
                StatusCode::TOO_MANY_REQUESTS
            );
        }
        assert_eq!(reached.load(Ordering::SeqCst), 11);
    }

    #[tokio::test]
    async fn security_auth_limits_competing_claimants_share_last_active_slot() {
        let admission = Arc::new(AuthAdmission::new(vec![]));
        let source: IpAddr = "192.0.2.10".parse().unwrap();
        let held = admission
            .admit(source, Kind::Login, Instant::now())
            .unwrap()
            .unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let tasks: Vec<_> = (0..2)
            .map(|_| {
                let admission = admission.clone();
                let barrier = barrier.clone();
                tokio::spawn(async move {
                    barrier.wait().await;
                    let granted = admission
                        .admit(source, Kind::Login, Instant::now())
                        .unwrap();
                    barrier.wait().await;
                    granted.is_some()
                })
            })
            .collect();
        barrier.wait().await;
        barrier.wait().await;
        let mut granted = 0;
        for task in tasks {
            granted += usize::from(task.await.unwrap());
        }
        assert_eq!(granted, 1);
        drop(held);
    }

    #[test]
    fn security_auth_limits_metric_rollup_uses_fixed_kinds_and_saturating_counts() {
        let now = Instant::now();
        let mut window = MetricWindow::new(now);
        assert!(window.record(Kind::Login, true, now).is_none());
        assert!(window.record(Kind::Login, false, now).is_none());
        let previous = window
            .record(Kind::Oauth, true, now + WINDOW)
            .expect("one finished window");
        assert_eq!(previous.accepted[Kind::Login.index()], 1);
        assert_eq!(previous.limited[Kind::Login.index()], 1);
        assert_eq!(window.counts.accepted[Kind::Oauth.index()], 1);
        window.counts.limited[Kind::Login.index()] = u64::MAX;
        window.record(Kind::Login, false, now + WINDOW);
        assert_eq!(window.counts.limited[Kind::Login.index()], u64::MAX);
    }
}
