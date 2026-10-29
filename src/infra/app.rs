use std::time::Duration;

use axum::{Router, extract::MatchedPath, http, middleware};
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use uuid::Uuid;

use crate::adapters::{
    self,
    http::{app_state::AppState, security},
};

pub fn create_app(app_state: AppState) -> Router {
    let auth_admission = app_state.auth_admission.clone();
    let allowed_origins: Vec<http::HeaderValue> = app_state
        .config
        .cors_allowed_origins
        .iter()
        .map(|origin| {
            origin.parse().unwrap_or_else(|_| {
                panic!("CORS_ALLOWED_ORIGINS contains an invalid origin: {origin}")
            })
        })
        .collect();

    // Credentials are allowed, so the origin can never be a wildcard; every
    // accepted origin has to be listed explicitly.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods([http::Method::POST, http::Method::GET])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION])
        .allow_credentials(true);

    let security_config = app_state.config.clone();
    Router::new()
        .merge(adapters::http::routes::router(app_state.sessions.clone()))
        .with_state(app_state)
        .layer(middleware::from_fn_with_state(
            auth_admission,
            adapters::http::auth_admission::admit,
        ))
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            security_config,
            security::browser_security,
        ))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(request_span)
                .on_response(request_finished),
        )
}

fn request_span<B>(request: &http::Request<B>) -> tracing::Span {
    // Keep this allowlist in sync with docs/request-trace-redaction.md. In particular, do not
    // fall back to the raw URI when a router has no matched route template.
    let request_id = Uuid::new_v4();
    if let Some(route) = request.extensions().get::<MatchedPath>() {
        tracing::info_span!(
            "http-request",
            method = %request.method(),
            version = ?request.version(),
            request_id = %request_id,
            route = route.as_str()
        )
    } else {
        tracing::info_span!(
            "http-request",
            method = %request.method(),
            version = ?request.version(),
            request_id = %request_id
        )
    }
}

fn request_finished<B>(response: &http::Response<B>, latency: Duration, _: &tracing::Span) {
    tracing::info!(
        status = response.status().as_u16(),
        latency_ms = u64::try_from(latency.as_millis()).unwrap_or(u64::MAX),
        "request completed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::Path,
        routing::{get, post},
    };
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };
    use tower::ServiceExt;
    use tracing_subscriber::layer::SubscriberExt;

    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn security_auth_limits_callback_uri_is_not_traced() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer({
                let output = output.clone();
                move || Capture(output.clone())
            })
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let request = http::Request::builder()
            .method(http::Method::GET)
            .uri("/auth/google/callback?code=secret-code-marker&state=secret-state-marker")
            .body(())
            .unwrap();
        request_span(&request).in_scope(|| tracing::info!("request rejected"));
        let log = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(
            log.contains("request_id=") && log.contains("method=GET"),
            "{log}"
        );
        assert!(
            !log.contains("secret-code-marker") && !log.contains("secret-state-marker"),
            "{log}"
        );
        assert!(!log.contains("/auth/google/callback?"), "{log}");
    }

    #[test]
    fn security_trace_redaction_request_span_has_no_raw_target_fallback() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer({
                let output = output.clone();
                move || Capture(output.clone())
            })
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        for (method, uri) in [
            (
                http::Method::GET,
                "/auth/google/callback?code=google-code-marker&state=oauth-state-marker",
            ),
            (
                http::Method::POST,
                "/webhooks/email/resend_api/webhook-token-marker?source=query-marker",
            ),
            (
                http::Method::GET,
                "/ui/skill-library?trail=user-search-marker",
            ),
            (
                http::Method::GET,
                "/unknown/signed-link-marker?proof=query-proof-marker",
            ),
        ] {
            let request = http::Request::builder()
                .method(method)
                .uri(uri)
                .header("host", "host-marker.example")
                .header("authorization", "Bearer auth-header-marker")
                .header("cookie", "session=cookie-marker")
                .body("request-body-marker")
                .unwrap();
            request_span(&request).in_scope(|| tracing::info!(status = 400, "request rejected"));
        }

        let log = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(
            log.contains("request_id=")
                && log.contains("method=GET")
                && log.contains("method=POST")
                && log.contains("version=")
                && log.contains("status=400"),
            "{log}"
        );
        for secret in [
            "google-code-marker",
            "oauth-state-marker",
            "webhook-token-marker",
            "query-marker",
            "user-search-marker",
            "signed-link-marker",
            "query-proof-marker",
            "host-marker.example",
            "auth-header-marker",
            "cookie-marker",
            "request-body-marker",
            "/auth/google/callback",
            "/webhooks/email/resend_api",
            "/unknown/",
        ] {
            assert!(!log.contains(secret), "{secret} leaked: {log}");
        }
    }

    #[tokio::test]
    async fn security_trace_redaction_middleware_keeps_status_without_callback_or_webhook_secret() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer({
                let output = output.clone();
                move || Capture(output.clone())
            })
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let app = Router::new()
            .route(
                "/auth/google/callback",
                get(|| async { http::StatusCode::BAD_REQUEST }),
            )
            .route(
                "/webhooks/email/resend_api/{token}",
                post(|Path(token): Path<String>| async move {
                    assert_eq!(token, "webhook-token-marker");
                    http::StatusCode::UNAUTHORIZED
                }),
            )
            .route(
                "/provider/fail",
                get(|| async {
                    Err::<(), _>(crate::app_error::AppError::Internal(
                        "provider-response-marker".into(),
                    ))
                }),
            )
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(request_span)
                    .on_response(request_finished),
            );

        let callback = http::Request::builder()
            .uri("/auth/google/callback?code=oauth-code-marker&state=oauth-state-marker")
            .header("host", "host-marker.example")
            .header("cookie", "session=cookie-marker")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(callback).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);

        let webhook = http::Request::builder()
            .method(http::Method::POST)
            .uri("/webhooks/email/resend_api/webhook-token-marker?proof=query-marker")
            .header("authorization", "Bearer auth-header-marker")
            .body(Body::from("body-marker"))
            .unwrap();
        let response = app.clone().oneshot(webhook).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::UNAUTHORIZED);

        let provider = http::Request::builder()
            .uri("/provider/fail?url=provider-url-marker")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(provider).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::INTERNAL_SERVER_ERROR);

        let log = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        for kept in [
            "request_id=",
            "method=GET",
            "method=POST",
            "status=400",
            "status=401",
            "status=500",
            "error_kind=\"internal\"",
            "latency_ms=",
        ] {
            assert!(log.contains(kept), "missing {kept}: {log}");
        }
        for secret in [
            "oauth-code-marker",
            "oauth-state-marker",
            "webhook-token-marker",
            "query-marker",
            "host-marker.example",
            "cookie-marker",
            "auth-header-marker",
            "body-marker",
            "provider-response-marker",
            "provider-url-marker",
        ] {
            assert!(!log.contains(secret), "{secret} leaked: {log}");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn security_trace_redaction_production_filter_keeps_routes_without_secret_markers() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(
                "mail_agents=info,tower_http=info,sqlx=warn",
            ))
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_target(false)
                    .with_level(true)
                    .pretty()
                    .with_writer({
                        let output = output.clone();
                        move || Capture(output.clone())
                    }),
            );
        let _guard = tracing::subscriber::set_default(subscriber);
        let app = Router::new()
            .route("/health", get(|| async { http::StatusCode::OK }))
            .route(
                "/auth/google/callback",
                get(|| async {
                    Err::<(), _>(crate::app_error::AppError::BadRequest(
                        "provider-body-marker".into(),
                    ))
                }),
            )
            .route(
                "/webhooks/email/resend_api/{token}",
                post(|Path(_): Path<String>| async { http::StatusCode::UNAUTHORIZED }),
            )
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(request_span)
                    .on_response(request_finished),
            );

        for (uri, method, expected) in [
            ("/health?search=search-marker", http::Method::GET, 200),
            (
                "/auth/google/callback?code=oauth-code-marker&state=oauth-state-marker",
                http::Method::GET,
                400,
            ),
            (
                "/webhooks/email/resend_api/webhook-token-marker?proof=query-marker",
                http::Method::POST,
                401,
            ),
            (
                "/unknown/signed-link-marker?proof=unknown-query-marker",
                http::Method::GET,
                404,
            ),
        ] {
            let request = http::Request::builder()
                .method(method)
                .uri(uri)
                .header("host", "host-marker.example")
                .header("authorization", "Bearer auth-marker")
                .header("cookie", "session=cookie-marker")
                .body(Body::from("request-body-marker"))
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status().as_u16(), expected);
        }

        let log = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        for kept in [
            "request_id",
            "method",
            "/auth/google/callback",
            "/webhooks/email/resend_api/{token}",
            "status",
            "latency_ms",
            "200",
            "400",
            "401",
            "404",
        ] {
            assert!(log.contains(kept), "missing {kept}: {log}");
        }
        for marker in [
            "search-marker",
            "oauth-code-marker",
            "oauth-state-marker",
            "provider-body-marker",
            "webhook-token-marker",
            "query-marker",
            "signed-link-marker",
            "unknown-query-marker",
            "host-marker.example",
            "auth-marker",
            "cookie-marker",
            "request-body-marker",
        ] {
            assert!(!log.contains(marker), "{marker} leaked: {log}");
        }
    }
}
