use std::sync::Arc;

use axum::{
    body::Body,
    extract::{FromRequestParts, State},
    http::{Request, StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, session::SessionAuthority},
    entities::{user::Viewer, value_objects::EmailAddress},
};

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

/// Who a request is signed in as.
///
/// The answer comes from [`SessionAuthority`] and nowhere else: a cookie is believed because it
/// carries a signature this deployment produced, never because it parses as a user id.
fn authenticated_user(
    sessions: &SessionAuthority,
    headers: &axum::http::HeaderMap,
) -> Option<AuthenticatedUser> {
    sessions
        .user_from_headers(headers)
        .map(|id| AuthenticatedUser { id })
}

fn is_htmx_request(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get("HX-Request")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "true")
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

/// How *this* request should be told it is not signed in.
///
/// A page gets a redirect, an htmx fragment gets `HX-Redirect`, and only an API caller gets a bare
/// 401 — a browser on `/ui` must never be left staring at the words "Authentication required".
fn auth_error_for(parts: &Parts) -> AuthError {
    auth_error(parts.uri.path(), is_htmx_request(&parts.headers))
}

pub async fn require_auth(
    State(sessions): State<Arc<SessionAuthority>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if authenticated_user(&sessions, request.headers()).is_some() {
        return next.run(request).await;
    }

    let is_htmx = is_htmx_request(request.headers());

    auth_error(request.uri().path(), is_htmx).into_response()
}

impl FromRequestParts<AppState> for AuthenticatedUser {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(user) = authenticated_user(&state.sessions, &parts.headers) {
            return Ok(user);
        }

        Err(auth_error_for(parts))
    }
}

/// The signed-in account together with its address, for the guards written in terms of who a
/// channel's participants are.
///
/// An extractor rather than a lookup inside each handler, so a read guard cannot be reached with
/// somebody else's address: the address comes from the session's own account, every time.
impl FromRequestParts<AppState> for Viewer {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;

        // A signed cookie whose account no longer exists is a signed-out browser, not an API
        // client: send it back through the login page rather than answering a page request with
        // bare 401 text.
        let account = state
            .user_use_cases
            .get_user_by_id(user.id)
            .await
            .ok()
            .flatten()
            .ok_or_else(|| auth_error_for(parts))?;

        Ok(Viewer {
            user_id: user.id,
            email: EmailAddress::from(account.email.as_str()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, middleware, routing::get};
    use tower::ServiceExt;

    use crate::infra::config::AppConfig;

    fn sessions() -> Arc<SessionAuthority> {
        Arc::new(SessionAuthority::new(&AppConfig::for_test()))
    }

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

    fn parts_for(uri: &str, is_htmx: bool) -> Parts {
        let mut builder = Request::get(uri);
        if is_htmx {
            builder = builder.header("HX-Request", "true");
        }
        builder.body(Body::empty()).unwrap().into_parts().0
    }

    #[test]
    fn a_ui_page_is_sent_to_login_rather_than_told_it_is_unauthorized() {
        let response = auth_error_for(&parts_for("/ui/dashboard", false)).into_response();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get("location").unwrap(), "/login");

        let fragment = auth_error_for(&parts_for("/ui/dashboard", true)).into_response();
        assert_eq!(fragment.headers().get("HX-Redirect").unwrap(), "/login");

        let api = auth_error_for(&parts_for("/api/companies", false)).into_response();
        assert_eq!(api.status(), StatusCode::UNAUTHORIZED);
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
            .route_layer(middleware::from_fn_with_state(sessions(), require_auth));

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

    #[tokio::test]
    async fn a_signed_session_gets_in_and_a_typed_user_id_does_not() {
        let sessions = sessions();
        let app = Router::new()
            .route("/companies", get(|| async { StatusCode::OK }))
            .route_layer(middleware::from_fn_with_state(
                sessions.clone(),
                require_auth,
            ));

        let signed_in = app
            .clone()
            .oneshot(
                Request::get("/companies")
                    .header(
                        "cookie",
                        format!("session={}", sessions.cookie(Uuid::new_v4()).value()),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(signed_in.status(), StatusCode::OK);

        // What the old cookie looked like. It is now worth exactly nothing.
        let forged = app
            .oneshot(
                Request::get("/companies")
                    .header("cookie", format!("user_id={}", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(forged.status(), StatusCode::SEE_OTHER);
    }
}
