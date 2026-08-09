use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use crate::adapters::http::app_state::AppState;

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: Uuid,
}

pub enum AuthError {
    RedirectToLogin,
    HtmxRedirectToLogin,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            AuthError::RedirectToLogin => Redirect::to("/login").into_response(),
            AuthError::HtmxRedirectToLogin => (
                StatusCode::OK,
                [("HX-Redirect", "/login")],
                "Redirecting to login...",
            )
                .into_response(),
        }
    }
}

impl FromRequestParts<AppState> for AuthenticatedUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let is_htmx = parts
            .headers
            .get("HX-Request")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "true")
            .unwrap_or(false);

        let jar = CookieJar::from_headers(&parts.headers);

        if let Some(cookie) = jar.get("user_id") {
            if let Ok(user_id) = Uuid::parse_str(cookie.value()) {
                return Ok(AuthenticatedUser { id: user_id });
            }
        }

        if is_htmx {
            Err(AuthError::HtmxRedirectToLogin)
        } else {
            Err(AuthError::RedirectToLogin)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn auth_error_redirects_to_login() {
        let response = AuthError::RedirectToLogin.into_response();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get("location").unwrap().to_str().unwrap(),
            "/login"
        );
    }

    #[tokio::test]
    async fn auth_error_htmx_redirects_to_login() {
        let response = AuthError::HtmxRedirectToLogin.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("HX-Redirect")
                .unwrap()
                .to_str()
                .unwrap(),
            "/login"
        );
    }
}
