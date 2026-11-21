use super::session::SessionAuthority;
use crate::{
    application::attribution::*,
    infra::attribution::{AttributionConfig, hash},
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;
use uuid::Uuid;
pub mod worker;
const PENDING: &str = "__Host-openbot-attribution";
const PREFIX: &str = "/api/attribution/v1";
pub struct Bridge {
    pub config: Arc<AttributionConfig>,
    pub store: Arc<dyn AttributionIdentity>,
    pub outbox: Arc<dyn AttributionOutbox>,
    pub sessions: Arc<SessionAuthority>,
    pub delivery: reqwest::Client,
    admission: Mutex<Admission>,
}
struct Admission {
    tokens: f64,
    updated: Instant,
}
impl Bridge {
    pub fn new(
        config: Arc<AttributionConfig>,
        store: Arc<dyn AttributionIdentity>,
        outbox: Arc<dyn AttributionOutbox>,
        sessions: Arc<SessionAuthority>,
    ) -> anyhow::Result<Self> {
        let delivery = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(2))
            .pool_max_idle_per_host(4)
            .build()?;
        Ok(Self {
            config,
            store,
            outbox,
            sessions,
            delivery,
            admission: Mutex::new(Admission {
                tokens: 16.0,
                updated: Instant::now(),
            }),
        })
    }
}
/// A focused router with the same production handlers can be mounted by browser fixtures.
pub fn router<S: Clone + Send + Sync + 'static>(bridge: Option<Arc<Bridge>>) -> Router<S> {
    let Some(bridge) = bridge else {
        return Router::new();
    };
    Router::new()
        .nest(
            PREFIX,
            Router::new()
                .route("/authorize", get(authorize))
                .route("/resume", get(resume))
                .route("/token", post(token))
                .route("/identity", get(identity))
                .route("/companies", get(companies))
                .route("/companies/{company_id}/access", get(company_access))
                .route(
                    "/companies/{company_id}/status",
                    get(company_status).post(set_status),
                )
                .route("/revoke", post(revoke)),
        )
        .layer(middleware::from_fn_with_state(bridge.clone(), boundary))
        .with_state(bridge)
}
async fn boundary(State(bridge): State<Arc<Bridge>>, request: Request, next: Next) -> Response {
    if matches!(
        request.uri().path().rsplit('/').next(),
        Some("authorize" | "resume" | "token")
    ) {
        let permitted = {
            let mut budget = bridge.admission.lock().unwrap();
            let now = Instant::now();
            budget.tokens =
                (budget.tokens + now.duration_since(budget.updated).as_secs_f64()).min(16.0);
            budget.updated = now;
            if budget.tokens >= 1.0 {
                budget.tokens -= 1.0;
                true
            } else {
                false
            }
        };
        if !permitted {
            let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            return secure(response);
        }
    }
    if request.uri().query().is_some_and(|s| s.len() > 4096) {
        return secure(IdentityError::Invalid.into_response());
    }
    match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await {
        Ok(response) => secure(response),
        Err(_) => secure(IdentityError::Unavailable.into_response()),
    }
}
fn secure(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'; base-uri 'none'"),
    );
    response
        .headers_mut()
        .insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}
impl IntoResponse for IdentityError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
            Self::Invalid => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Conflict => (StatusCode::CONFLICT, "conflict"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        };
        let mut response=(status,Json(serde_json::json!({"code":code,"message":"Identity request could not be completed","correlation_id":Uuid::new_v4()}))).into_response();
        if status == StatusCode::SERVICE_UNAVAILABLE {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        }
        response
    }
}
fn opaque(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn random() -> String {
    oauth2::CsrfToken::new_random_len(32).secret().to_owned()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizeQuery {
    client_id: String,
    redirect_uri: String,
    response_type: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
}
async fn authorize(
    State(bridge): State<Arc<Bridge>>,
    request: Request,
) -> IdentityResult<Response> {
    let query = Query::<AuthorizeQuery>::try_from_uri(request.uri())
        .map_err(|_| IdentityError::Invalid)?
        .0;
    if query.client_id != "attribution"
        || query.redirect_uri != bridge.config.callback
        || query.response_type != "code"
        || !opaque(&query.state)
        || query.code_challenge.len() != 43
        || !opaque(&query.code_challenge)
        || query.code_challenge_method != "S256"
    {
        return Err(IdentityError::Invalid);
    }
    if request.headers().contains_key(header::AUTHORIZATION) {
        return Err(IdentityError::Unauthorized);
    }
    let binding = AuthorizationBinding {
        callback: Callback(query.redirect_uri),
        challenge: Challenge(query.code_challenge),
        state: RequestState(query.state),
    };
    if let Some(actor) = bridge
        .sessions
        .user_from_headers(request.headers())
        .await
        .map_err(|_| IdentityError::Unavailable)?
    {
        let code = random();
        let binding = bridge
            .store
            .issue_code(IssueCode {
                actor,
                source: AuthorizationSource::Direct(binding),
                code: hash(&code),
            })
            .await?;
        return callback(&bridge, binding, &code);
    }
    let correlation = random();
    bridge
        .store
        .begin_continuation(&hash(&correlation), &binding)
        .await?;
    let mut response = redirect("/login")?;
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie(&correlation, 600)?);
    Ok(response)
}
pub fn pending(headers: &HeaderMap) -> Option<String> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        let value = value.to_str().ok()?;
        if value.len() > 8192 {
            return None;
        }
        for item in value.split(';') {
            if let Some((name, value)) = item.trim().split_once('=')
                && name == PENDING
            {
                if found.is_some() || !opaque(value) {
                    return None;
                }
                found = Some(value.to_owned());
            }
        }
    }
    found
}
async fn resume(State(bridge): State<Arc<Bridge>>, request: Request) -> IdentityResult<Response> {
    if request.headers().contains_key(header::AUTHORIZATION) {
        return Err(IdentityError::Unauthorized);
    }
    let actor = bridge
        .sessions
        .user_from_headers(request.headers())
        .await
        .map_err(|_| IdentityError::Unavailable)?
        .ok_or(IdentityError::Unauthorized)?;
    let pending = pending(request.headers()).ok_or(IdentityError::Unauthorized)?;
    let code = random();
    let binding = bridge
        .store
        .issue_code(IssueCode {
            actor,
            source: AuthorizationSource::Continuation(hash(&pending)),
            code: hash(&code),
        })
        .await?;
    let mut response = callback(&bridge, binding, &code)?;
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie("", 0)?);
    Ok(response)
}
fn callback(
    bridge: &Bridge,
    binding: AuthorizationBinding,
    code: &str,
) -> IdentityResult<Response> {
    if binding.callback.0 != bridge.config.callback {
        return Err(IdentityError::Invalid);
    }
    let mut url =
        url::Url::parse(&bridge.config.callback).map_err(|_| IdentityError::Unavailable)?;
    url.query_pairs_mut()
        .append_pair("code", code)
        .append_pair("state", &binding.state.0);
    redirect(url.as_str())
}
fn redirect(location: &str) -> IdentityResult<Response> {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(location).map_err(|_| IdentityError::Invalid)?,
    );
    Ok(response)
}
fn cookie(value: &str, age: i64) -> IdentityResult<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{PENDING}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={age}"
    ))
    .map_err(|_| IdentityError::Unavailable)
}
fn authorization(headers: &HeaderMap) -> IdentityResult<&str> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values
        .next()
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() <= 1024)
        .ok_or(IdentityError::Unauthorized)?;
    if values.next().is_some() {
        return Err(IdentityError::Unauthorized);
    }
    Ok(value)
}
fn client(bridge: &Bridge, headers: &HeaderMap) -> IdentityResult<()> {
    let raw = authorization(headers)?
        .strip_prefix("Basic ")
        .ok_or(IdentityError::Unauthorized)?;
    let decoded = STANDARD
        .decode(raw)
        .map_err(|_| IdentityError::Unauthorized)?;
    let decoded = std::str::from_utf8(&decoded).map_err(|_| IdentityError::Unauthorized)?;
    let (name, secret) = decoded.split_once(':').ok_or(IdentityError::Unauthorized)?;
    if name != "attribution" || !bridge.config.accepts(secret) {
        return Err(IdentityError::Unauthorized);
    }
    Ok(())
}
fn bearer(headers: &HeaderMap) -> IdentityResult<SecretHash> {
    let value = authorization(headers)?
        .strip_prefix("Bearer ")
        .filter(|v| opaque(v))
        .ok_or(IdentityError::Unauthorized)?;
    Ok(hash(value))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenForm {
    grant_type: String,
    code: String,
    redirect_uri: String,
    code_verifier: String,
}
async fn token(State(bridge): State<Arc<Bridge>>, request: Request) -> Response {
    let result = exchange(&bridge, request).await;
    match result {
        Ok(response) => response,
        Err(IdentityError::TooLarge) => IdentityError::TooLarge.into_response(),
        Err(error) => {
            let (status, code) = match error {
                IdentityError::Unavailable => {
                    (StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
                }
                IdentityError::Unauthorized => (StatusCode::BAD_REQUEST, "invalid_grant"),
                IdentityError::Forbidden => (StatusCode::UNAUTHORIZED, "invalid_client"),
                _ => (StatusCode::BAD_REQUEST, "invalid_request"),
            };
            let mut response = (
                status,
                Json(serde_json::json!({"error":code,"error_description":"Code exchange failed"})),
            )
                .into_response();
            if status == StatusCode::SERVICE_UNAVAILABLE {
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            }
            response
        }
    }
}
async fn exchange(bridge: &Bridge, request: Request) -> IdentityResult<Response> {
    client(bridge, request.headers()).map_err(|_| IdentityError::Forbidden)?;
    if !request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').next() == Some("application/x-www-form-urlencoded"))
    {
        return Err(IdentityError::Invalid);
    }
    let bytes = to_bytes(request.into_body(), 4096)
        .await
        .map_err(|_| IdentityError::TooLarge)?;
    let form: TokenForm =
        serde_urlencoded::from_bytes(&bytes).map_err(|_| IdentityError::Invalid)?;
    if form.grant_type != "authorization_code"
        || !opaque(&form.code)
        || form.redirect_uri != bridge.config.callback
        || !(43..=128).contains(&form.code_verifier.len())
        || !form
            .code_verifier
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c))
    {
        return Err(IdentityError::Invalid);
    }
    let verifier = oauth2::PkceCodeVerifier::new(form.code_verifier);
    let challenge = oauth2::PkceCodeChallenge::from_code_verifier_sha256(&verifier);
    let grant = random();
    let expiry = bridge
        .store
        .redeem(RedeemCode {
            code: hash(&form.code),
            callback: Callback(form.redirect_uri),
            challenge: Challenge(challenge.as_str().into()),
            grant: hash(&grant),
        })
        .await?;
    let expires_in = (expiry - Utc::now()).num_seconds().clamp(1, 28800);
    Ok(Json(
        serde_json::json!({"access_token":grant,"token_type":"Bearer","expires_in":expires_in}),
    )
    .into_response())
}
async fn identity(State(bridge): State<Arc<Bridge>>, request: Request) -> IdentityResult<Response> {
    Ok(Json(bridge.store.identity(&bearer(request.headers())?).await?).into_response())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    user: Uuid,
    after: Uuid,
    client: String,
}
fn encode_cursor(bridge: &Bridge, user: Uuid, after: Uuid) -> IdentityResult<String> {
    let payload = serde_json::to_vec(&Cursor {
        user,
        after,
        client: "attribution".into(),
    })
    .map_err(|_| IdentityError::Unavailable)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&bridge.config.cursor_key)
        .map_err(|_| IdentityError::Unavailable)?;
    mac.update(&payload);
    Ok(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    ))
}
fn decode_cursor(bridge: &Bridge, user: Uuid, cursor: &str) -> IdentityResult<Uuid> {
    if cursor.len() > 512 {
        return Err(IdentityError::Invalid);
    }
    let (payload, signature) = cursor.split_once('.').ok_or(IdentityError::Invalid)?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| IdentityError::Invalid)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| IdentityError::Invalid)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&bridge.config.cursor_key)
        .map_err(|_| IdentityError::Unavailable)?;
    mac.update(&payload);
    mac.verify_slice(&signature)
        .map_err(|_| IdentityError::Invalid)?;
    let cursor: Cursor = serde_json::from_slice(&payload).map_err(|_| IdentityError::Invalid)?;
    if cursor.user != user || cursor.client != "attribution" || cursor.after.is_nil() {
        return Err(IdentityError::Invalid);
    }
    Ok(cursor.after)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    limit: Option<u16>,
    cursor: Option<String>,
}
async fn companies(
    State(bridge): State<Arc<Bridge>>,
    request: Request,
) -> IdentityResult<Response> {
    let grant = bearer(request.headers())?;
    let identity = bridge.store.identity(&grant).await?;
    let query = Query::<PageQuery>::try_from_uri(request.uri())
        .map_err(|_| IdentityError::Invalid)?
        .0;
    let after = query
        .cursor
        .as_deref()
        .map(|value| decode_cursor(&bridge, identity.user_id, value))
        .transpose()?;
    let page = bridge
        .store
        .companies(CompanyPageRequest {
            grant,
            after,
            limit: query.limit.unwrap_or(25),
        })
        .await?;
    let next = if page.more {
        page.items
            .last()
            .map(|item| encode_cursor(&bridge, identity.user_id, item.company_id))
            .transpose()?
    } else {
        None
    };
    Ok(Json(serde_json::json!({"items":page.items,"next_cursor":next})).into_response())
}
async fn company_access(
    State(bridge): State<Arc<Bridge>>,
    Path(company): Path<Uuid>,
    request: Request,
) -> IdentityResult<Response> {
    Ok(Json(
        bridge
            .store
            .access(&bearer(request.headers())?, company)
            .await?,
    )
    .into_response())
}
async fn company_status(
    State(bridge): State<Arc<Bridge>>,
    Path(company): Path<Uuid>,
    request: Request,
) -> IdentityResult<Response> {
    client(&bridge, request.headers())?;
    Ok(Json(bridge.store.status(company).await?).into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusWrite {
    state: CompanyState,
    expected_version: i64,
}
async fn set_status(
    State(bridge): State<Arc<Bridge>>,
    Path(company): Path<Uuid>,
    request: Request,
) -> IdentityResult<Response> {
    if request.headers().contains_key(header::AUTHORIZATION) {
        return Err(IdentityError::Unauthorized);
    }
    // The existing browser security layer checks Origin/Referer. An Authorization
    // header must never bypass that layer for this cookie-only route.
    let actor = bridge
        .sessions
        .user_from_headers(request.headers())
        .await
        .map_err(|_| IdentityError::Unavailable)?
        .ok_or(IdentityError::Unauthorized)?;
    let bytes = to_bytes(request.into_body(), 4096)
        .await
        .map_err(|_| IdentityError::TooLarge)?;
    let payload: StatusWrite =
        serde_json::from_slice(&bytes).map_err(|_| IdentityError::Invalid)?;
    Ok(Json(
        bridge
            .store
            .set_status(StatusChange {
                actor,
                company_id: company,
                expected_version: payload.expected_version,
                state: payload.state,
            })
            .await?,
    )
    .into_response())
}
async fn revoke(State(bridge): State<Arc<Bridge>>, request: Request) -> IdentityResult<StatusCode> {
    bridge.store.revoke(&bearer(request.headers())?).await?;
    Ok(StatusCode::NO_CONTENT)
}
