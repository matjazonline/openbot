use super::crypto;
use crate::adapters::html::escape;
use crate::{application::identity::*, infra::identity::IdentityConfig};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::time::Instant;
use uuid::Uuid;
const SESSION: &str = "__Host-attribution";
const LOGIN: &str = "__Host-attribution-login";
#[derive(Clone)]
pub struct IdentityHttp {
    pub service: Arc<IdentityService>,
    pub config: Arc<IdentityConfig>,
    admission: Arc<Mutex<LoginAdmission>>,
}
struct LoginAdmission {
    tokens: f64,
    updated: Instant,
}
impl IdentityHttp {
    pub fn new(service: Arc<IdentityService>, config: Arc<IdentityConfig>) -> Self {
        Self {
            service,
            config,
            admission: Arc::new(Mutex::new(LoginAdmission {
                tokens: 16.0,
                updated: Instant::now(),
            })),
        }
    }
}
pub fn router(state: IdentityHttp) -> Router<crate::adapters::http::HttpState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
        .route("/v1/session", get(session))
        .route("/v1/companies", get(companies))
        .route("/internal/v1/company-lifecycle", post(lifecycle))
        .route("/", get(shell))
        .route("/settings", get(shell))
        .route(
            "/assets/setup.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../../assets/setup.js"),
                )
            }),
        )
        .route(
            "/assets/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../../assets/app.js"),
                )
            }),
        )
        .layer(middleware::from_fn_with_state(state.clone(), admission))
        .with_state(state)
}
async fn admission(State(state): State<IdentityHttp>, request: Request, next: Next) -> Response {
    if request.uri().path().starts_with("/auth/") {
        let permitted = {
            let mut budget = state.admission.lock().unwrap();
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
            return Failure::RateLimited.into_response();
        }
    }
    if request.uri().query().is_some_and(|s| s.len() > 4096) {
        return Failure::Invalid.into_response();
    }
    next.run(request).await
}
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::TooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Request body exceeds limit",
            ),
            Self::Invalid => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid request",
            ),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", "Sign in required"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden", "Access denied"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found", "Resource not found"),
            Self::Conflict => (StatusCode::CONFLICT, "conflict", "Conflicting state"),
            Self::Gone => (
                StatusCode::GONE,
                "expired",
                "Resource or retry window expired",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                "Authority or storage unavailable",
            ),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Try again shortly",
            ),
        };
        crate::adapters::http::problem(status, code, message, Uuid::new_v4())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginQuery {
    return_to: Option<String>,
}
async fn login(State(state): State<IdentityHttp>, request: Request) -> Result<Response> {
    let query = Query::<LoginQuery>::try_from_uri(request.uri())
        .map_err(|_| Failure::Invalid)?
        .0;
    let return_path = crypto::local_return(query.return_to.as_deref().unwrap_or("/"))?;
    let (challenge, verifier) = oauth2::PkceCodeChallenge::new_random_sha256();
    let transaction = LoginTransaction {
        id: Uuid::new_v4(),
        state: LoginState(crypto::random()),
        browser: BrowserBinding(crypto::random()),
        pkce: PkceVerifier(verifier.secret().to_owned()),
        return_path,
    };
    state.service.sessions.begin_login(&transaction).await?;
    let mut authorize = state
        .config
        .openbot_origin
        .join("api/attribution/v1/authorize")
        .map_err(|_| Failure::Unavailable)?;
    authorize
        .query_pairs_mut()
        .append_pair("client_id", "attribution")
        .append_pair(
            "redirect_uri",
            &format!("{}auth/callback", state.config.public_origin),
        )
        .append_pair("response_type", "code")
        .append_pair("state", &transaction.state.0)
        .append_pair("code_challenge", challenge.as_str())
        .append_pair("code_challenge_method", "S256");
    let mut response = redirect(authorize.as_str())?;
    response.headers_mut().append(
        header::SET_COOKIE,
        cookie(LOGIN, &transaction.browser.0, 600)?,
    );
    Ok(response)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallbackQuery {
    code: String,
    state: String,
}
async fn callback(State(state): State<IdentityHttp>, request: Request) -> Result<Response> {
    let query = Query::<CallbackQuery>::try_from_uri(request.uri())
        .map_err(|_| Failure::Invalid)?
        .0;
    if !opaque(&query.code) || !opaque(&query.state) {
        return Err(Failure::Invalid);
    }
    let browser =
        BrowserBinding(read_cookie(request.headers(), LOGIN)?.ok_or(Failure::Unauthorized)?);
    let completed = state
        .service
        .sessions
        .consume_login(&LoginState(query.state), &browser)
        .await?;
    let grant = state
        .service
        .authority
        .exchange(&Code(query.code), &completed.pkce)
        .await?;
    let identity = state.service.authority.identity(&grant).await?;
    if identity.user_id.0.is_nil()
        || identity.expires_at <= Utc::now()
        || identity.expires_at > Utc::now() + chrono::Duration::hours(8)
    {
        return Err(Failure::Unauthorized);
    }
    let session = NewSession {
        id: SessionId(Uuid::new_v4()),
        token: SessionToken(crypto::random()),
        user_id: identity.user_id,
        grant,
        csrf: CsrfToken(crypto::random()),
        expires_at: identity.expires_at,
    };
    let old = read_cookie(request.headers(), SESSION)?.map(SessionToken);
    state
        .service
        .sessions
        .create_session(&session, old.as_ref())
        .await?;
    let mut response = redirect(&crypto::local_return(&completed.return_path)?)?;
    response.headers_mut().append(
        header::SET_COOKIE,
        cookie(
            SESSION,
            &session.token.0,
            (identity.expires_at - Utc::now()).num_seconds().min(28800),
        )?,
    );
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie(LOGIN, "", 0)?);
    Ok(response)
}
pub fn session_token(headers: &HeaderMap) -> Result<SessionToken> {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(Failure::Unauthorized);
    }
    Ok(SessionToken(
        read_cookie(headers, SESSION)?.ok_or(Failure::Unauthorized)?,
    ))
}
fn read_cookie(headers: &HeaderMap, name: &str) -> Result<Option<String>> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        let value = value.to_str().map_err(|_| Failure::Invalid)?;
        if value.len() > 8192 {
            return Err(Failure::Invalid);
        }
        for item in value.split(';') {
            if let Some((key, value)) = item.trim().split_once('=')
                && key == name
            {
                if found.is_some() || !opaque(value) {
                    return Err(Failure::Unauthorized);
                }
                found = Some(value.to_owned());
            }
        }
    }
    Ok(found)
}
fn opaque(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn cookie(name: &str, value: &str, age: i64) -> Result<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={age}"
    ))
    .map_err(|_| Failure::Unavailable)
}
fn redirect(location: &str) -> Result<Response> {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(location).map_err(|_| Failure::Invalid)?,
    );
    Ok(response)
}
pub fn csrf<'a>(config: &IdentityConfig, headers: &'a HeaderMap) -> Result<&'a str> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok())
        .ok_or(Failure::Forbidden)?;
    if origin != config.public_origin.origin().ascii_serialization() {
        return Err(Failure::Forbidden);
    }
    headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() <= 64)
        .ok_or(Failure::Forbidden)
}
async fn logout(State(state): State<IdentityHttp>, request: Request) -> Result<Response> {
    let token = session_token(request.headers())?;
    let supplied = csrf(&state.config, request.headers())?;
    state.service.logout(&token, supplied).await?;
    let mut response = redirect("/")?;
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie(SESSION, "", 0)?);
    Ok(response)
}
#[derive(Serialize)]
struct SessionInfo {
    user_id: UserId,
    display_name: String,
    csrf_token: String,
    expires_at: chrono::DateTime<Utc>,
}
async fn session(State(state): State<IdentityHttp>, request: Request) -> Result<Response> {
    let principal = state
        .service
        .authenticate(&session_token(request.headers())?)
        .await?;
    Ok(Json(SessionInfo {
        user_id: principal.identity.user_id,
        display_name: principal.identity.display_name,
        csrf_token: principal.session.csrf.0,
        expires_at: principal
            .session
            .expires_at
            .min(principal.identity.expires_at),
    })
    .into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    limit: Option<u16>,
    cursor: Option<String>,
}
async fn companies(State(state): State<IdentityHttp>, request: Request) -> Result<Response> {
    let principal = state
        .service
        .authenticate(&session_token(request.headers())?)
        .await?;
    let query = Query::<PageQuery>::try_from_uri(request.uri())
        .map_err(|_| Failure::Invalid)?
        .0;
    let page = state
        .service
        .authority
        .companies(
            &principal.session.grant,
            query.limit.unwrap_or(25),
            query.cursor.as_deref(),
        )
        .await?;
    Ok(Json(page).into_response())
}
async fn lifecycle(State(state): State<IdentityHttp>, request: Request) -> Result<Response> {
    let mut auth = request.headers().get_all(header::AUTHORIZATION).iter();
    let value = auth
        .next()
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(Failure::Unauthorized)?;
    if auth.next().is_some() || !state.config.lifecycle_secret.accepts(value) {
        return Err(Failure::Unauthorized);
    }
    let bytes = to_bytes(request.into_body(), 4096)
        .await
        .map_err(|_| Failure::TooLarge)?;
    let status: CompanyStatus = serde_json::from_slice(&bytes).map_err(|_| Failure::Invalid)?;
    if status.company_id.0.is_nil() || status.version < 1 {
        return Err(Failure::Invalid);
    }
    state
        .service
        .tenants
        .observe(&status, None, None, None)
        .await?;
    Ok(Json(status).into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShellQuery {
    company_id: Option<Uuid>,
    cursor: Option<String>,
}
async fn shell(State(state): State<IdentityHttp>, request: Request) -> Result<Response> {
    let token = match session_token(request.headers()) {
        Ok(token) => token,
        Err(Failure::Unauthorized) => return sign_in(),
        Err(error) => return Err(error),
    };
    let query = Query::<ShellQuery>::try_from_uri(request.uri())
        .map_err(|_| Failure::Invalid)?
        .0;
    let data = match load_shell(&state, &token, query).await {
        Ok(data) => data,
        Err(Failure::Unauthorized) => return sign_in(),
        Err(error) => return Err(error),
    };
    Ok(render_shell(&state.config, data))
}
fn sign_in() -> Result<Response> {
    let mut response = Html("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Attribution</title><link rel=\"stylesheet\" href=\"/assets/app.css\"></head><body><main><h1>Attribution</h1><p>Measure visits and sales across your stores.</p><a href=\"/auth/login\">Sign in with Openbot</a></main></body></html>").into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie(SESSION, "", 0)?);
    Ok(response)
}
struct ShellData {
    principal: Principal,
    active: Option<CompanyAccess>,
    page: CompanyPage,
}
async fn load_shell(
    state: &IdentityHttp,
    token: &SessionToken,
    query: ShellQuery,
) -> Result<ShellData> {
    let (principal, active) = match query.company_id {
        Some(id) if !id.is_nil() => {
            let tenant = state
                .service
                .company(token, CompanyId(id), Permission::Read)
                .await?;
            (tenant.principal, Some(tenant.access))
        }
        Some(_) => return Err(Failure::Invalid),
        None => (state.service.authenticate(token).await?, None),
    };
    let page = state
        .service
        .authority
        .companies(&principal.session.grant, 25, query.cursor.as_deref())
        .await?;
    Ok(ShellData {
        principal,
        active,
        page,
    })
}
fn render_shell(config: &IdentityConfig, data: ShellData) -> Response {
    let ShellData {
        principal,
        active,
        page,
    } = data;
    let mut navigation = String::new();
    for company in page.items {
        navigation.push_str(&format!(
            "<li><a href=\"/?company_id={}\">{}</a></li>",
            company.company_id.0,
            escape(&company.name)
        ));
    }
    if let Some(cursor) = page.next_cursor {
        let query: String = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("cursor", &cursor)
            .finish();
        navigation.push_str(&format!(
            "<li><a href=\"/?{}\">More companies</a></li>",
            escape(&query)
        ));
    }
    let heading = active
        .as_ref()
        .map(|c| c.name.as_str())
        .unwrap_or("Choose a company");
    let settings = active
        .as_ref()
        .map(|c| format!("/settings?company_id={}", c.company_id.0))
        .unwrap_or_else(|| "/settings".into());
    let reports = active
        .as_ref()
        .map(|c| {
            format!(
                "<a href=\"/reports?company_id={}\">Reports</a>",
                c.company_id.0
            )
        })
        .unwrap_or_default();
    let setup = active.as_ref().map(|company| format!(
        "<section id=\"setup\" data-company=\"{}\" data-csrf=\"{}\" data-manage=\"{}\">{}</section><script src=\"/assets/setup.js\" defer></script>",
        company.company_id.0, escape(&principal.session.csrf.0), company.permissions.contains(&Permission::ManageIntegrations), include_str!("../../../assets/setup.html")
    )).unwrap_or_default();
    Html(format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Attribution</title><link rel=\"stylesheet\" href=\"/assets/app.css\"><script src=\"/assets/app.js\" defer></script></head><body><header><a href=\"/\">Attribution</a><nav aria-label=\"Main\">{reports}<a href=\"{settings}\">Settings</a><button id=\"logout\" data-csrf=\"{}\">Sign out</button></nav></header><main><h1>{}</h1><p>Signed in as {}</p><ul aria-label=\"Companies\">{navigation}</ul><section><h2>Settings</h2><p>Manage your account and company in Openbot.</p><a href=\"{}ui/profile\" target=\"_blank\" rel=\"noopener noreferrer\">Account settings</a> <a href=\"{}ui/companies\" target=\"_blank\" rel=\"noopener noreferrer\">Company settings</a></section>{setup}<p id=\"status\" role=\"status\"></p></main></body></html>", escape(&principal.session.csrf.0), escape(heading), escape(&principal.identity.display_name), escape(config.openbot_origin.as_str()), escape(config.openbot_origin.as_str()))).into_response()
}
