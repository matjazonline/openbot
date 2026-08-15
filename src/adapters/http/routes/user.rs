use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::AppResult,
    use_cases::{company::CompanyUseCases, user::UserUseCases},
};

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_page).post(login_form))
        .route("/register", get(register_page).post(register_form))
        .route("/api/user/register", post(register_form))
        .route("/api/user/login", post(login_form))
        .route("/api/json/user/register", post(register_json))
        .route("/api/json/user/login", post(login_json))
}

pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/logout", post(logout))
}

async fn index(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
) -> impl IntoResponse {
    let destination = match company_use_cases.list_user_companies(user.id).await {
        Ok(companies) if companies.is_empty() => "/onboarding",
        _ => "/companies",
    };
    Redirect::temporary(destination)
}

async fn logout(_user: AuthenticatedUser, jar: CookieJar) -> impl IntoResponse {
    let cookie = axum_extra::extract::cookie::Cookie::build(("user_id", ""))
        .path("/")
        .build();

    (jar.remove(cookie), Redirect::to("/login"))
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

#[derive(Debug, Clone, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub username: String,
    pub email: String,
}

/// Handles HTMX form submission for user registration.
#[instrument(skip(user_use_cases, form))]
async fn register_form(
    State(user_use_cases): State<Arc<UserUseCases>>,
    Form(form): Form<RegisterForm>,
) -> impl IntoResponse {
    if form.username.trim().is_empty() || form.email.trim().is_empty() {
        return Html(pages::error_alert("Username and email are required."));
    }

    if let Some(confirm) = &form.confirm_password {
        if form.password.expose_secret() != confirm.expose_secret() {
            return Html(pages::error_alert("Passwords do not match."));
        }
    }

    match user_use_cases
        .add(&form.username, &form.email, &form.password)
        .await
    {
        Ok(_) => Html(pages::success_alert(
            "Account created successfully!",
            Some(("/login", "Sign in now")),
        )),
        Err(err) => Html(pages::error_alert(&format!("Registration failed: {err}"))),
    }
}

use axum_extra::extract::CookieJar;

/// Handles HTMX form submission for user login.
#[instrument(skip(user_use_cases, jar, form))]
async fn login_form(
    State(user_use_cases): State<Arc<UserUseCases>>,
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
            let cookie =
                axum_extra::extract::cookie::Cookie::build(("user_id", user.id.to_string()))
                    .path("/")
                    .http_only(true)
                    .build();
            let updated_jar = jar.add(cookie);

            let alert = pages::success_alert(
                &format!(
                    "Welcome back, {}! Authentication successful.",
                    user.username
                ),
                Some(("/", "Continue")),
            );

            (updated_jar, [("HX-Redirect", "/")], Html(alert)).into_response()
        }
        Err(_) => (
            jar,
            Html(pages::error_alert("Invalid username/email or password.")),
        )
            .into_response(),
    }
}

/// JSON endpoint for registration (API compatibility).
#[instrument(skip(user_use_cases, payload))]
async fn register_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    Json(payload): Json<RegisterPayload>,
) -> AppResult<impl IntoResponse> {
    info!("Register JSON API called");
    user_use_cases
        .add(&payload.username, &payload.email, &payload.password)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            success: true,
            message: "User registered successfully".to_string(),
        }),
    ))
}

/// JSON endpoint for login (API compatibility).
#[instrument(skip(user_use_cases, jar, payload))]
async fn login_json(
    State(user_use_cases): State<Arc<UserUseCases>>,
    jar: CookieJar,
    Json(payload): Json<LoginForm>,
) -> AppResult<impl IntoResponse> {
    info!("Login JSON API called");
    let user = user_use_cases
        .login(&payload.email_or_username, &payload.password)
        .await?;

    let cookie = axum_extra::extract::cookie::Cookie::build(("user_id", user.id.to_string()))
        .path("/")
        .http_only(true)
        .build();
    let updated_jar = jar.add(cookie);

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

    #[tokio::test]
    async fn logout_clears_cookie_and_redirects_to_login() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            format!("user_id={}", uuid::Uuid::nil()).parse().unwrap(),
        );
        let jar = CookieJar::from_headers(&headers);

        let response = logout(
            AuthenticatedUser {
                id: uuid::Uuid::nil(),
            },
            jar,
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get("location").unwrap(), "/login");
        let set_cookie = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(set_cookie.starts_with("user_id="));
        assert!(set_cookie.contains("Max-Age=0"));
        assert!(set_cookie.contains("Path=/"));
    }
}
