use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

use crate::{
    adapters::http::{
        app_state::AppState, auth::AuthenticatedUser, pages, session::SessionAuthority,
    },
    app_error::AppResult,
    use_cases::{
        company::CompanyUseCases,
        company_invite::CompanyInviteUseCases,
        user::{RegistrationOutcome, UserUseCases},
    },
};

/// Where a browser goes the moment it holds a session: the mailbox, not the sign-in form.
///
/// Named once so registration and sign-in cannot drift to different destinations.
const SIGNED_IN_LANDING: &str = "/ui";

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
}

pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
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

    if invite_use_cases
        .has_pending_user_invites(&account.email)
        .await?
    {
        return Ok(Redirect::temporary("/ui/invites"));
    }

    let destination = match company_use_cases.list_user_companies(user.id).await {
        Ok(companies) if companies.is_empty() => "/onboarding",
        _ => "/companies",
    };
    Ok(Redirect::temporary(destination))
}

async fn logout(
    _user: AuthenticatedUser,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
) -> impl IntoResponse {
    let jar = sessions
        .cleared_cookies()
        .into_iter()
        .fold(jar, |jar, cookie| jar.remove(cookie));

    (jar, Redirect::to("/login"))
}

async fn login_page() -> impl IntoResponse {
    Html(pages::login_page())
}

async fn register_page() -> impl IntoResponse {
    Html(pages::register_page())
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
#[instrument(skip(user_use_cases, sessions, jar, form))]
async fn register_form(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
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

    if let Some(confirm) = &form.confirm_password {
        if form.password.expose_secret() != confirm.expose_secret() {
            return (jar, Html(pages::error_alert("Passwords do not match."))).into_response();
        }
    }

    match user_use_cases
        .register(&form.username, &form.email, &form.password)
        .await
    {
        Ok(RegistrationOutcome::Created(user)) => {
            let updated_jar = jar.add(sessions.cookie(user.id));

            let alert = pages::success_alert(
                &format!("Welcome, {}! Your account is ready.", user.username),
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
            jar.add(sessions.cookie(user.id)),
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
#[instrument(skip(user_use_cases, sessions, jar, form))]
async fn login_form(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
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
        .login(&form.email_or_username, &form.password)
        .await
    {
        Ok(user) => {
            let updated_jar = jar.add(sessions.cookie(user.id));

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
#[instrument(skip(user_use_cases, sessions, jar, payload))]
async fn register_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
    Json(payload): Json<RegisterPayload>,
) -> AppResult<axum::response::Response> {
    info!("Register JSON API called");
    let outcome = user_use_cases
        .register(&payload.username, &payload.email, &payload.password)
        .await?;
    Ok(match outcome {
        RegistrationOutcome::Created(user) => (
            jar.add(sessions.cookie(user.id)),
            (
                StatusCode::CREATED,
                Json(RegisterResponse {
                    success: true,
                    message: "User registered successfully".into(),
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
        jar.add(sessions.cookie(user.id)),
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
#[instrument(skip(user_use_cases, sessions, jar, payload))]
async fn login_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(sessions): State<Arc<SessionAuthority>>,
    jar: CookieJar,
    Json(payload): Json<LoginForm>,
) -> AppResult<impl IntoResponse> {
    info!("Login JSON API called");
    let user = user_use_cases
        .login(&payload.email_or_username, &payload.password)
        .await?;

    let updated_jar = jar.add(sessions.cookie(user.id));

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
mod tests {
    use axum_extra::extract::cookie::Cookie;

    use super::*;

    #[tokio::test]
    async fn login_page_contains_htmx_attributes() {
        let html = pages::login_page();
        assert!(html.contains("htmx.org"));
        assert!(html.contains("hx-post=\"/api/user/login\""));
        assert!(html.contains("hx-target=\"#response-message\""));
        assert!(!html.contains(">Companies</a>"));
        assert!(!html.contains(">My Invites</a>"));
    }

    #[tokio::test]
    async fn register_page_contains_htmx_attributes() {
        let html = pages::register_page();
        assert!(html.contains("htmx.org"));
        assert!(html.contains("hx-post=\"/api/user/register\""));
        assert!(html.contains("hx-target=\"#response-message\""));
        assert!(!html.contains(">Companies</a>"));
        assert!(!html.contains(">My Invites</a>"));
    }

    #[test]
    fn alerts_render_correct_html() {
        let success = pages::success_alert("Done!", Some(("/login", "Login")));
        assert!(success.contains("Done!"));
        assert!(success.contains("href=\"/login\""));

        let error = pages::error_alert("Something went wrong");
        assert!(error.contains("Something went wrong"));
    }

    /// A persistence that hands back the account it was asked to create, so a handler that signs
    /// the new user in has an id to sign in.
    struct StubUserPersistence {
        id: uuid::Uuid,
    }

    #[async_trait::async_trait]
    impl crate::use_cases::user::UserPersistence for StubUserPersistence {
        async fn create_user(
            &self,
            username: &str,
            email: &str,
            password_hash: &str,
        ) -> AppResult<crate::entities::user::User> {
            Ok(crate::entities::user::User {
                id: self.id,
                username: username.to_string(),
                email: email.to_string(),
                password_hash: password_hash.to_string(),
                avatar_url: None,
                created_at: chrono::Utc::now(),
            })
        }

        async fn get_by_email(&self, _: &str) -> AppResult<Option<crate::entities::user::User>> {
            Ok(None)
        }

        async fn get_by_username(&self, _: &str) -> AppResult<Option<crate::entities::user::User>> {
            Ok(None)
        }

        async fn get_by_id(&self, _: uuid::Uuid) -> AppResult<Option<crate::entities::user::User>> {
            Ok(None)
        }

        async fn update_avatar_url(
            &self,
            _: uuid::Uuid,
            _: Option<&crate::entities::value_objects::AvatarUrl>,
        ) -> AppResult<Option<crate::entities::user::User>> {
            Ok(None)
        }

        async fn update_profile(
            &self,
            _: uuid::Uuid,
            _: crate::use_cases::user::ProfileUpdate<'_>,
        ) -> AppResult<Option<crate::entities::user::User>> {
            Ok(None)
        }

        async fn update_password_hash(
            &self,
            _: uuid::Uuid,
            _: &str,
        ) -> AppResult<Option<crate::entities::user::User>> {
            Ok(None)
        }
    }

    struct StubHasher;

    impl crate::use_cases::user::UserCredentialsHasher for StubHasher {
        fn hash_password(&self, password: &str) -> AppResult<String> {
            Ok(format!("{password}_hash"))
        }

        fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool> {
            Ok(hash == format!("{password}_hash"))
        }
    }

    fn registering(id: uuid::Uuid) -> Arc<UserUseCases> {
        Arc::new(UserUseCases::new(
            Arc::new(StubHasher),
            Arc::new(StubUserPersistence { id }),
        ))
    }

    fn registration(password: &str, confirm: Option<&str>) -> RegisterForm {
        RegisterForm {
            username: "newcomer".to_string(),
            email: "newcomer@example.com".to_string(),
            password: password.into(),
            confirm_password: confirm.map(Into::into),
        }
    }

    /// Registering is signing in: the response carries a session this deployment will believe,
    /// naming the account that was just created.
    #[tokio::test]
    async fn registering_signs_the_new_account_in() {
        let id = uuid::Uuid::new_v4();
        let sessions = Arc::new(SessionAuthority::new(
            &crate::infra::config::AppConfig::for_test(),
        ));

        let response = register_form(
            State(registering(id)),
            State(sessions.clone()),
            CookieJar::new(),
            Form(registration("hunter2", Some("hunter2"))),
        )
        .await
        .into_response();

        let session = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|value| value.to_str().expect("a printable cookie"))
            .find_map(|value| Cookie::parse(value.to_string()).ok())
            .filter(|cookie| cookie.name() == crate::adapters::http::session::SESSION_COOKIE)
            .expect("registration issues a session cookie");

        assert_eq!(sessions.verify(session.value()), Some(id));
    }

    #[tokio::test]
    async fn registering_lands_on_the_mailbox() {
        let sessions = Arc::new(SessionAuthority::new(
            &crate::infra::config::AppConfig::for_test(),
        ));

        let response = register_form(
            State(registering(uuid::Uuid::new_v4())),
            State(sessions),
            CookieJar::new(),
            Form(registration("hunter2", Some("hunter2"))),
        )
        .await
        .into_response();

        assert_eq!(response.headers().get("HX-Redirect").unwrap(), "/ui");
    }

    /// A rejected registration must not hand out a session for an account that was never created.
    #[tokio::test]
    async fn mismatched_passwords_issue_no_session() {
        let sessions = Arc::new(SessionAuthority::new(
            &crate::infra::config::AppConfig::for_test(),
        ));

        let response = register_form(
            State(registering(uuid::Uuid::new_v4())),
            State(sessions),
            CookieJar::new(),
            Form(registration("hunter2", Some("hunter3"))),
        )
        .await
        .into_response();

        assert!(response.headers().get("HX-Redirect").is_none());
        assert!(
            response
                .headers()
                .get_all("set-cookie")
                .iter()
                .next()
                .is_none()
        );
    }

    #[tokio::test]
    async fn logout_clears_both_cookies_and_redirects_to_login() {
        let sessions = Arc::new(SessionAuthority::new(
            &crate::infra::config::AppConfig::for_test(),
        ));

        // A browser part-way through the change carries both: the session it signed in with, and
        // the plaintext cookie left over from before.
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            format!(
                "session={}; user_id={}",
                sessions.cookie(uuid::Uuid::nil()).value(),
                uuid::Uuid::nil()
            )
            .parse()
            .unwrap(),
        );
        let jar = CookieJar::from_headers(&headers);

        let response = logout(
            AuthenticatedUser {
                id: uuid::Uuid::nil(),
            },
            State(sessions),
            jar,
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get("location").unwrap(), "/login");

        let cleared: Vec<&str> = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|value| value.to_str().expect("a printable cookie"))
            .collect();

        // The session goes, and so does the plaintext cookie it replaced -- a browser still
        // holding one of those should stop offering it.
        assert!(
            cleared.iter().any(|cookie| cookie.starts_with("session=")),
            "{cleared:?}"
        );
        assert!(
            cleared.iter().any(|cookie| cookie.starts_with("user_id=")),
            "{cleared:?}"
        );
        assert!(
            cleared
                .iter()
                .all(|cookie| cookie.contains("Max-Age=0") && cookie.contains("Path=/")),
            "{cleared:?}"
        );
    }
}
