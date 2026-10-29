use std::sync::Arc;

use axum::{
    Extension, Form, Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    adapters::http::{
        app_state::AppState, auth::AuthenticatedUser, auth_admission::AuthClientSource, pages,
        session::SessionAuthority,
    },
    app_error::{AppError, AppResult},
    entities::value_objects::EmailAddress,
    infra::config::{AppConfig, AppleOAuthConfig, GoogleOAuthConfig},
    use_cases::{
        company::CompanyUseCases,
        company_invite::CompanyInviteUseCases,
        user::{
            AuthWorkLifetime, ExternalIdentity, LoginMethod, RegistrationOutcome, UserUseCases,
        },
    },
};

/// Where a browser goes the moment it holds a session.
///
/// The authenticated root is the post-authentication gateway: it checks pending invitations
/// before deciding between onboarding and the mailbox. Named once so every sign-in method uses
/// the same decision path.
pub(super) const SIGNED_IN_LANDING: &str = "/";
const AUTH_BUSY_MESSAGE: &str = "Authentication is busy. Try again shortly.";
const GOOGLE_FLOW_COOKIE: &str = "google_oauth_flow";

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_page).post(login_form))
        .route("/register", get(register_page).post(register_form))
        .route("/api/user/register", post(register_form))
        .route(
            "/api/user/register/confirm",
            post(confirm_registration_form),
        )
        .route("/api/user/login", post(login_form))
        .route("/api/json/user/register", post(register_json))
        .route(
            "/api/json/user/register/confirm",
            post(confirm_registration_json),
        )
        .route("/api/json/user/login", post(login_json))
        .route("/auth/google/register", get(start_google_registration))
        .route("/auth/google/login", get(start_google_login))
        .route("/auth/google/callback", get(google_callback))
}

pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/auth/google/connect", get(start_google_connection))
        .route("/logout", post(logout))
}

async fn index(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(invite_use_cases): State<Arc<CompanyInviteUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
) -> AppResult<Redirect> {
    let account = user_use_cases
        .get_user_by_id(user.id)
        .await?
        .ok_or_else(|| crate::app_error::AppError::NotFound("User not found".into()))?;

    if invite_use_cases.has_pending_user_invites(&account).await? {
        return Ok(Redirect::temporary("/ui/invites"));
    }

    let companies = company_use_cases.list_user_companies(user.id).await?;
    let destination = if companies.is_empty() {
        "/ui/onboarding"
    } else {
        "/ui"
    };
    Ok(Redirect::temporary(destination))
}

async fn logout(
    Extension(session): Extension<crate::application::session::VerifiedSession>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
) -> AppResult<impl IntoResponse> {
    sessions.revoke(session).await?;
    let jar = sessions
        .cleared_cookies()
        .into_iter()
        .fold(jar, |jar, cookie| jar.remove(cookie));

    Ok((jar, Redirect::to("/login")))
}

async fn login_page() -> impl IntoResponse {
    Html(pages::login_page(
        GoogleOAuthConfig::from_env().is_some(),
        AppleOAuthConfig::from_env().is_some(),
    ))
}

async fn register_page() -> impl IntoResponse {
    Html(pages::register_page(
        GoogleOAuthConfig::from_env().is_some(),
        AppleOAuthConfig::from_env().is_some(),
    ))
}

#[derive(Clone, Copy)]
enum GoogleFlow {
    Register,
    Login,
    Connect,
}

impl GoogleFlow {
    fn as_str(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Login => "login",
            Self::Connect => "connect",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "register" => Some(Self::Register),
            "login" => Some(Self::Login),
            "connect" => Some(Self::Connect),
            _ => None,
        }
    }
}

async fn start_google_registration(
    State(config): State<Arc<AppConfig>>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
) -> impl IntoResponse {
    start_google_flow(&config, &sessions, jar, GoogleFlow::Register, None)
}

async fn start_google_login(
    State(config): State<Arc<AppConfig>>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
) -> impl IntoResponse {
    start_google_flow(&config, &sessions, jar, GoogleFlow::Login, None)
}

async fn start_google_connection(
    State(config): State<Arc<AppConfig>>,
    State(sessions): State<Arc<SessionAuthority>>,
    user: AuthenticatedUser,
    jar: CookieJar,
) -> impl IntoResponse {
    start_google_flow(&config, &sessions, jar, GoogleFlow::Connect, Some(user.id))
}

fn start_google_flow(
    config: &AppConfig,
    sessions: &SessionAuthority,
    jar: CookieJar,
    flow: GoogleFlow,
    user_id: Option<Uuid>,
) -> axum::response::Response {
    let Some(google) = GoogleOAuthConfig::from_env() else {
        return (
            StatusCode::NOT_FOUND,
            Html(pages::error_alert(
                "Google authentication is not configured.",
            )),
        )
            .into_response();
    };
    let state = sessions.issue_oauth_state(
        "google",
        flow.as_str(),
        user_id,
        Uuid::new_v4().simple().to_string(),
    );
    let encoded_state = sessions.encode_oauth_state(&state);
    let cookie = Cookie::build((GOOGLE_FLOW_COOKIE, state.transaction))
        .path("/auth/google")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(config.secure_cookies)
        .max_age(time::Duration::minutes(10))
        .build();
    let redirect_uri = google_redirect_uri(config);
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &google.client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("state", &encoded_state)
        .finish();
    (
        jar.add(cookie),
        Redirect::temporary(&format!(
            "https://accounts.google.com/o/oauth2/v2/auth?{query}"
        )),
    )
        .into_response()
}

#[derive(Deserialize)]
struct GoogleCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct GoogleProfile {
    sub: String,
    email: String,
    #[serde(default)]
    email_verified: bool,
    name: Option<String>,
}

async fn google_callback(
    State(config): State<Arc<AppConfig>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    headers: HeaderMap,
    jar: CookieJar,
    Query(query): Query<GoogleCallbackQuery>,
) -> impl IntoResponse {
    let signed_in_user = match sessions.user_from_headers(&headers).await {
        Ok(session) => session.map(|s| s.user_id),
        Err(err) => return err.into_response(),
    };
    let result = complete_google_flow(
        &config,
        &user_use_cases,
        &sessions,
        &jar,
        signed_in_user,
        query,
    )
    .await;
    let cleared = jar.remove(
        Cookie::build(GOOGLE_FLOW_COOKIE)
            .path("/auth/google")
            .build(),
    );
    match result {
        Ok((user, GoogleFlow::Connect)) => (
            cleared.add(match sessions.cookie(&user).await {
                Ok(cookie) => cookie,
                Err(err) => return err.into_response(),
            }),
            Redirect::temporary("/ui/profile?connected=google"),
        )
            .into_response(),
        Ok((user, _)) => (
            cleared.add(match sessions.cookie(&user).await {
                Ok(cookie) => cookie,
                Err(err) => return err.into_response(),
            }),
            Redirect::temporary(SIGNED_IN_LANDING),
        )
            .into_response(),
        Err(error) => (
            cleared,
            (
                StatusCode::BAD_REQUEST,
                Html(pages::error_alert(&error.to_string())),
            ),
        )
            .into_response(),
    }
}

async fn complete_google_flow(
    config: &AppConfig,
    user_use_cases: &UserUseCases,
    sessions: &SessionAuthority,
    jar: &CookieJar,
    signed_in_user: Option<Uuid>,
    query: GoogleCallbackQuery,
) -> AppResult<(crate::entities::user::User, GoogleFlow)> {
    if query.error.is_some() {
        return Err(AppError::BadRequest(
            "Google authentication was cancelled.".into(),
        ));
    }
    let stored_transaction = jar.get(GOOGLE_FLOW_COOKIE).ok_or_else(|| {
        AppError::BadRequest("Google authentication expired. Please try again.".into())
    })?;
    let encoded_state = query
        .state
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Invalid Google authentication state.".into()))?;
    let state = sessions
        .verify_oauth_state(encoded_state)
        .filter(|state| state.provider == "google")
        .ok_or_else(|| AppError::BadRequest("Invalid Google authentication state.".into()))?;
    if stored_transaction.value() != state.transaction {
        return Err(AppError::BadRequest(
            "Invalid Google authentication state.".into(),
        ));
    }
    let flow = GoogleFlow::parse(&state.flow)
        .ok_or_else(|| AppError::BadRequest("Invalid Google authentication flow.".into()))?;
    let google = GoogleOAuthConfig::from_env()
        .ok_or_else(|| AppError::BadRequest("Google authentication is not configured.".into()))?;
    let code = query.code.ok_or_else(|| {
        AppError::BadRequest("Google did not return an authorization code.".into())
    })?;
    let profile = fetch_google_profile(config, &google, &code).await?;
    if !profile.email_verified {
        return Err(AppError::BadRequest(
            "Google has not verified this email address.".into(),
        ));
    }
    let email = EmailAddress::from(profile.email.as_str());
    let external = ExternalIdentity {
        provider: LoginMethod::Google,
        subject: &profile.sub,
        email: &email,
        email_verified: profile.email_verified,
        display_name: profile.name.as_deref(),
    };
    let user = match flow {
        GoogleFlow::Register => user_use_cases.register_external(external).await,
        GoogleFlow::Login => user_use_cases.login_external(external).await,
        GoogleFlow::Connect => {
            let initiating_user = state
                .user_id
                .ok_or_else(|| AppError::BadRequest("Invalid Google connection account.".into()))?;
            if signed_in_user != Some(initiating_user) {
                return Err(AppError::BadRequest(
                    "Google connection expired. Please sign in and try again.".into(),
                ));
            }
            user_use_cases
                .link_external(initiating_user, external)
                .await
        }
    }?;
    Ok((user, flow))
}

async fn fetch_google_profile(
    config: &AppConfig,
    google: &GoogleOAuthConfig,
    code: &str,
) -> AppResult<GoogleProfile> {
    let client = reqwest::Client::new();
    let redirect_uri = google_redirect_uri(config);
    let token = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", google.client_id.as_str()),
            ("client_secret", google.client_secret.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|_| AppError::BadRequest("Could not contact Google.".into()))?
        .error_for_status()
        .map_err(|_| AppError::BadRequest("Google rejected the authorization code.".into()))?
        .json::<GoogleTokenResponse>()
        .await
        .map_err(|_| AppError::BadRequest("Google returned an invalid token response.".into()))?;
    client
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(token.access_token)
        .send()
        .await
        .map_err(|_| AppError::BadRequest("Could not load the Google profile.".into()))?
        .error_for_status()
        .map_err(|_| AppError::BadRequest("Google rejected the access token.".into()))?
        .json::<GoogleProfile>()
        .await
        .map_err(|_| AppError::BadRequest("Google returned an invalid profile.".into()))
}

fn google_redirect_uri(config: &AppConfig) -> String {
    let domain = config.app_domain_name.trim_end_matches('/');
    if domain.starts_with("http://") || domain.starts_with("https://") {
        format!("{domain}/auth/google/callback")
    } else {
        let scheme = if domain.starts_with("localhost") || domain.starts_with("127.0.0.1") {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{domain}/auth/google/callback")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterForm {
    pub username: String,
    pub email: String,
    pub password: SecretString,
    pub confirm_password: Option<SecretString>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginForm {
    pub email_or_username: String,
    pub password: SecretString,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterPayload {
    pub username: String,
    pub email: String,
    pub password: SecretString,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegisterResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfirmRegistration {
    pub email: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub username: String,
    pub email: String,
}

/// Handles HTMX form submission for user registration.
///
/// A successful registration signs the new account in on the spot: it leaves with the same session
/// cookie [`login_form`] would have minted, so the browser lands on [`SIGNED_IN_LANDING`] already
/// authenticated instead of being sent back to the sign-in form to retype what it just chose.
#[instrument(skip_all)]
async fn register_form(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    work: Extension<AuthWorkLifetime>,
    jar: CookieJar,
    Form(form): Form<RegisterForm>,
) -> impl IntoResponse {
    if form.username.trim().is_empty() || form.email.trim().is_empty() {
        return (
            jar,
            Html(pages::error_alert("Username and email are required.")),
        )
            .into_response();
    }

    if let Some(confirm) = &form.confirm_password
        && form.password.expose_secret() != confirm.expose_secret()
    {
        return (jar, Html(pages::error_alert("Passwords do not match."))).into_response();
    }

    match user_use_cases
        .register(&form.username, &form.email, &form.password, Some(work.0))
        .await
    {
        Ok(RegistrationOutcome::CreatedUnverified(user)) => {
            let updated_jar = jar.add(match sessions.cookie(&user).await {
                Ok(cookie) => cookie,
                Err(err) => return err.into_response(),
            });

            let alert = pages::success_alert(
                &format!(
                    "Welcome, {}! Your account is ready with an unverified email. Email-based privileges are unavailable until the address is verified.",
                    user.username
                ),
                Some((SIGNED_IN_LANDING, "Continue")),
            );

            (
                updated_jar,
                [("HX-Redirect", SIGNED_IN_LANDING)],
                Html(alert),
            )
                .into_response()
        }
        Ok(RegistrationOutcome::ConfirmationSent) => {
            (jar, Html(pages::confirmation_form(&form.email))).into_response()
        }
        Err(AppError::ServiceUnavailable(_)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            jar,
            Html(pages::error_alert(AUTH_BUSY_MESSAGE)),
        )
            .into_response(),
        Err(err) => (
            jar,
            Html(pages::error_alert(&format!("Registration failed: {err}"))),
        )
            .into_response(),
    }
}

async fn confirm_registration_form(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
    Form(form): Form<ConfirmRegistration>,
) -> impl IntoResponse {
    match user_use_cases
        .confirm_registration(&form.email, &form.code)
        .await
    {
        Ok(user) => (
            jar.add(match sessions.cookie(&user).await {
                Ok(cookie) => cookie,
                Err(err) => return err.into_response(),
            }),
            [("HX-Redirect", SIGNED_IN_LANDING)],
            Html(pages::success_alert(
                "Email confirmed. Your account is ready!",
                Some((SIGNED_IN_LANDING, "Continue")),
            )),
        )
            .into_response(),
        Err(err) => (
            jar,
            Html(pages::error_alert(&format!("Confirmation failed: {err}"))),
        )
            .into_response(),
    }
}

/// Handles HTMX form submission for user login.
#[instrument(skip_all)]
async fn login_form(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    source: Option<Extension<AuthClientSource>>,
    work: Extension<AuthWorkLifetime>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    if form.email_or_username.trim().is_empty() {
        return (
            jar,
            Html(pages::error_alert("Email or username is required.")),
        )
            .into_response();
    }

    match user_use_cases
        .login(
            &form.email_or_username,
            &form.password,
            source.map(|value| value.0.0),
            Some(work.0),
        )
        .await
    {
        Ok(user) => {
            let updated_jar = jar.add(match sessions.cookie(&user).await {
                Ok(cookie) => cookie,
                Err(err) => return err.into_response(),
            });

            let alert = pages::success_alert(
                &format!(
                    "Welcome back, {}! Authentication successful.",
                    user.username
                ),
                Some((SIGNED_IN_LANDING, "Continue")),
            );

            (
                updated_jar,
                [("HX-Redirect", SIGNED_IN_LANDING)],
                Html(alert),
            )
                .into_response()
        }
        Err(AppError::ServiceUnavailable(_)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            jar,
            Html(pages::error_alert(AUTH_BUSY_MESSAGE)),
        )
            .into_response(),
        Err(_) => (
            jar,
            Html(pages::error_alert("Invalid username/email or password.")),
        )
            .into_response(),
    }
}

/// JSON endpoint for registration (API compatibility).
///
/// Answers with the session cookie as well as the confirmation, so an API client is signed in by
/// registering exactly as a browser is.
#[instrument(skip_all)]
async fn register_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    work: Extension<AuthWorkLifetime>,
    jar: CookieJar,
    Json(payload): Json<RegisterPayload>,
) -> AppResult<axum::response::Response> {
    info!("Register JSON API called");
    let outcome = user_use_cases
        .register(
            &payload.username,
            &payload.email,
            &payload.password,
            Some(work.0),
        )
        .await?;
    Ok(match outcome {
        RegistrationOutcome::CreatedUnverified(user) => (
            jar.add(sessions.cookie(&user).await?),
            (
                StatusCode::CREATED,
                Json(RegisterResponse {
                    success: true,
                    message: "User registered with an unverified email; email-based privileges are unavailable until the address is verified".into(),
                }),
            ),
        )
            .into_response(),
        RegistrationOutcome::ConfirmationSent => (
            jar,
            (
                StatusCode::ACCEPTED,
                Json(RegisterResponse {
                    success: true,
                    message: "Confirmation code sent to the registered email".into(),
                }),
            ),
        )
            .into_response(),
    })
}

async fn confirm_registration_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
    Json(payload): Json<ConfirmRegistration>,
) -> AppResult<impl IntoResponse> {
    let user = user_use_cases
        .confirm_registration(&payload.email, &payload.code)
        .await?;
    Ok((
        jar.add(sessions.cookie(&user).await?),
        (
            StatusCode::CREATED,
            Json(RegisterResponse {
                success: true,
                message: "Email confirmed and user registered successfully".into(),
            }),
        ),
    ))
}

/// JSON endpoint for login (API compatibility).
#[instrument(skip_all)]
async fn login_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    source: Option<Extension<AuthClientSource>>,
    work: Extension<AuthWorkLifetime>,
    jar: CookieJar,
    Json(payload): Json<LoginForm>,
) -> AppResult<impl IntoResponse> {
    info!("Login JSON API called");
    let user = user_use_cases
        .login(
            &payload.email_or_username,
            &payload.password,
            source.map(|value| value.0.0),
            Some(work.0),
        )
        .await?;

    let updated_jar = jar.add(sessions.cookie(&user).await?);

    Ok((
        updated_jar,
        (
            StatusCode::OK,
            Json(LoginResponse {
                success: true,
                username: user.username,
                email: user.email,
            }),
        ),
    ))
}

#[cfg(test)]
#[path = "user_tests.rs"]
mod tests;
