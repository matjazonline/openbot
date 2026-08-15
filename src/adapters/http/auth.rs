use axum::{
    body::Body,
    extract::FromRequestParts,
    http::{Request, StatusCode, request::Parts},
    middleware::Next,
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
    Unauthorized,
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
            AuthError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "Authentication required").into_response()
            }
        }
    }
}

fn authenticated_user(headers: &axum::http::HeaderMap) -> Option<AuthenticatedUser> {
    let jar = CookieJar::from_headers(headers);
    let id = Uuid::parse_str(jar.get("user_id")?.value()).ok()?;
    Some(AuthenticatedUser { id })
}

fn auth_error(path: &str, is_htmx: bool) -> AuthError {
    if path.starts_with("/api/") {
        AuthError::Unauthorized
    } else if is_htmx {
        AuthError::HtmxRedirectToLogin
    } else {
        AuthError::RedirectToLogin
    }
}

pub async fn require_auth(request: Request<Body>, next: Next) -> Response {
    if authenticated_user(request.headers()).is_some() {
        return next.run(request).await;
    }

    let is_htmx = request
        .headers()
        .get("HX-Request")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "true");

    auth_error(request.uri().path(), is_htmx).into_response()
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

        if let Some(user) = authenticated_user(&parts.headers) {
            return Ok(user);
        }

        Err(auth_error(parts.uri.path(), is_htmx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, middleware, routing::get};
    use tower::ServiceExt;

    #[tokio::test]
    async fn auth_error_redirects_to_login() {
        let response = AuthError::RedirectToLogin.into_response();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response
                .headers()
                .get("location")
                .unwrap()
                .to_str()
                .unwrap(),
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

    #[test]
    fn api_auth_error_is_unauthorized() {
        let response = auth_error("/api/companies", true).into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get("location").is_none());
        assert!(response.headers().get("HX-Redirect").is_none());
    }

    #[tokio::test]
    async fn middleware_redirects_pages_and_rejects_apis() {
        let app = Router::new()
            .route("/companies", get(|| async { StatusCode::OK }))
            .route("/api/companies", get(|| async { StatusCode::OK }))
            .route_layer(middleware::from_fn(require_auth));

        let page_response = app
            .clone()
            .oneshot(Request::get("/companies").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(page_response.status(), StatusCode::SEE_OTHER);
        assert_eq!(page_response.headers().get("location").unwrap(), "/login");

        let api_response = app
            .oneshot(Request::get("/api/companies").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(api_response.status(), StatusCode::UNAUTHORIZED);
        assert!(api_response.headers().get("location").is_none());
    }
}
